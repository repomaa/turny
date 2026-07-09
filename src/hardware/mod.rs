pub mod gpio;
pub mod rfid;

use anyhow::Result;
use log::{info, warn};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::time::timeout;

// Re-export commonly used types
pub use gpio::{ButtonEvent, ButtonReader, LedController};
pub use rfid::RfidReader;

/// How long to wait for a single RFID SPI poll before giving up.
/// The actual SPI transfer is sub-millisecond, so this is very generous.
const RFID_READ_TIMEOUT: Duration = Duration::from_millis(500);

/// After this many consecutive failures, drop the stuck reader and
/// create a fresh one (which hardware-resets the MFRC522 via the RST pin).
const RFID_REINIT_THRESHOLD: u32 = 3;

/// Hardware manager that coordinates all hardware components
pub struct HardwareManager {
    pub button: Box<dyn ButtonReader + Send>,
    pub led: Box<dyn LedController + Send>,
    /// The RFID reader is stored in an `Option` inside a mutex.  The poll
    /// **checks out** the reader (sets the slot to `None`), does the blocking
    /// SPI read on a `spawn_blocking` thread, then **checks it back in**.
    ///
    /// If the SPI call hangs, the timeout fires but the thread (and the
    /// reader it holds) is leaked.  The slot stays `None`, so subsequent
    /// polls return immediately.  After `RFID_REINIT_THRESHOLD` failures
    /// a brand-new `Mfrc522RfidReader` is created — its `new()` toggles
    /// the hardware RST pin and re-initialises SPI, recovering the reader
    /// without ever blocking the async main loop.
    rfid_reader: Arc<StdMutex<Option<Box<dyn RfidReader + Send>>>>,
    rfid_failures: Arc<AtomicU32>,
}

impl HardwareManager {
    /// Initialize all hardware components
    pub fn new() -> Result<Self> {
        let button = Box::new(gpio::GpioButtonReader::new(crate::config::BUTTON_PIN)?);
        let led = Box::new(gpio::GpioLedController::new(crate::config::LED_PIN)?);
        let rfid_reader: Box<dyn RfidReader + Send> =
            Box::new(rfid::Mfrc522RfidReader::new()?);

        Ok(Self {
            button,
            led,
            rfid_reader: Arc::new(StdMutex::new(Some(rfid_reader))),
            rfid_failures: Arc::new(AtomicU32::new(0)),
        })
    }

    /// Read RFID card ID if available.
    ///
    /// Uses the checkout/checkin pattern so a stuck SPI call never blocks
    /// the main loop or prevents reinitialisation. See [`HardwareManager`]
    /// field docs for details.
    pub async fn read_rfid_card(&self) -> Option<String> {
        // --- Check out the reader ---------------------------------------
        let reader = {
            let mut guard = self.rfid_reader.lock().ok()?;
            guard.take()
        };

        let reader = match reader {
            Some(r) => r,
            None => {
                // The slot is empty: either a previous poll's thread is
                // still stuck holding the old reader, or a reinit is in
                // progress.  Count this as a failure and maybe reinit.
                let failures = self.rfid_failures.fetch_add(1, Ordering::Relaxed) + 1;
                if failures >= RFID_REINIT_THRESHOLD {
                    warn!(
                        "RFID reader stuck ({} consecutive failures), reinitialising...",
                        failures
                    );
                    self.reinit_rfid_reader().await;
                    self.rfid_failures.store(0, Ordering::Relaxed);
                }
                return None;
            }
        };

        // --- Poll on a blocking thread with timeout ---------------------
        let slot = self.rfid_reader.clone();
        let result = timeout(
            RFID_READ_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let mut reader = reader; // moved into the closure
                let card_id = reader.read_card_id();
                // Check reader back in — but only if nobody has installed
                // a new reader in the meantime (e.g. after a reinit).
                if let Ok(mut guard) = slot.lock() {
                    if guard.is_none() {
                        *guard = Some(reader);
                    }
                }
                card_id
            }),
        )
        .await;

        match result {
            Ok(Ok(card_id)) => {
                self.rfid_failures.store(0, Ordering::Relaxed);
                card_id
            }
            Ok(Err(e)) => {
                warn!("RFID polling thread panicked: {}", e);
                self.rfid_failures.fetch_add(1, Ordering::Relaxed);
                None
            }
            Err(_) => {
                warn!(
                    "RFID poll timed out after {:?}, reader may be stuck",
                    RFID_READ_TIMEOUT
                );
                self.rfid_failures.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Create a fresh `Mfrc522RfidReader` and install it in the slot.
    ///
    /// `Mfrc522RfidReader::new()` toggles the hardware RST pin (GPIO 25)
    /// and re-initialises the SPI bus, which recovers the reader from any
    /// stuck state.  The old reader (held by a leaked thread) is simply
    /// abandoned — it will be cleaned up when the process exits.
    async fn reinit_rfid_reader(&self) {
        let slot = self.rfid_reader.clone();
        let result = tokio::task::spawn_blocking(move || match rfid::Mfrc522RfidReader::new() {
            Ok(new_reader) => {
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(Box::new(new_reader));
                }
                info!("RFID reader reinitialised successfully");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to reinitialise RFID reader: {}", e);
                Err(e)
            }
        })
        .await;

        if let Err(e) = result {
            warn!("RFID reinit thread panicked: {}", e);
        }
    }

    /// Check for button events
    pub fn check_button(&mut self) -> Option<ButtonEvent> {
        self.button.check_event()
    }

    /// Turn LED on
    pub fn led_on(&mut self) -> Result<()> {
        self.led.turn_on()
    }

    /// Turn LED off
    pub fn led_off(&mut self) -> Result<()> {
        self.led.turn_off()
    }

    /// Blink LED for specified duration
    pub async fn blink_led(&mut self, duration: Duration) -> Result<()> {
        use crate::hardware::gpio::blink_led;
        blink_led(&mut *self.led, duration).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Mock implementations for testing
    struct MockButtonReader {
        events: Vec<ButtonEvent>,
        index: usize,
    }

    impl MockButtonReader {
        fn new(events: Vec<ButtonEvent>) -> Self {
            Self { events, index: 0 }
        }
    }

    impl ButtonReader for MockButtonReader {
        fn check_event(&mut self) -> Option<ButtonEvent> {
            if self.index < self.events.len() {
                let event = self.events[self.index].clone();
                self.index += 1;
                Some(event)
            } else {
                None
            }
        }
    }

    struct MockLedController {
        is_on: bool,
    }

    impl MockLedController {
        fn new() -> Self {
            Self { is_on: false }
        }
    }

    impl LedController for MockLedController {
        fn turn_on(&mut self) -> Result<()> {
            self.is_on = true;
            Ok(())
        }

        fn turn_off(&mut self) -> Result<()> {
            self.is_on = false;
            Ok(())
        }

        fn is_on(&self) -> bool {
            self.is_on
        }
    }

    struct MockRfidReader {
        card_id: Option<String>,
    }

    impl MockRfidReader {
        fn new(card_id: Option<String>) -> Self {
            Self { card_id }
        }
    }

    impl RfidReader for MockRfidReader {
        fn read_card_id(&mut self) -> Option<String> {
            self.card_id.clone()
        }
    }

    fn make_hardware(card_id: Option<String>) -> HardwareManager {
        let button = Box::new(MockButtonReader::new(vec![]));
        let led = Box::new(MockLedController::new());
        let rfid_reader: Box<dyn RfidReader + Send> = Box::new(MockRfidReader::new(card_id));
        HardwareManager {
            button,
            led,
            rfid_reader: Arc::new(StdMutex::new(Some(rfid_reader))),
            rfid_failures: Arc::new(AtomicU32::new(0)),
        }
    }

    #[test]
    fn test_hardware_manager_creation() {
        let _hardware = make_hardware(None);
        assert!(true);
    }

    #[tokio::test]
    async fn test_led_control() {
        let mut hardware = make_hardware(None);
        hardware.led_on().unwrap();
        hardware.led_off().unwrap();
        assert!(true);
    }

    #[test]
    fn test_button_events() {
        let events = vec![
            ButtonEvent::Pressed,
            ButtonEvent::Released(Duration::from_secs(1)),
        ];
        let button = Box::new(MockButtonReader::new(events));
        let led = Box::new(MockLedController::new());
        let rfid_reader: Box<dyn RfidReader + Send> = Box::new(MockRfidReader::new(None));

        let mut hardware = HardwareManager {
            button,
            led,
            rfid_reader: Arc::new(StdMutex::new(Some(rfid_reader))),
            rfid_failures: Arc::new(AtomicU32::new(0)),
        };

        assert!(matches!(hardware.check_button(), Some(ButtonEvent::Pressed)));
        assert!(matches!(hardware.check_button(), Some(ButtonEvent::Released(_))));
        assert!(hardware.check_button().is_none());
    }

    #[tokio::test]
    async fn test_rfid_reading() {
        let card_id = "test_card_123".to_string();
        let hardware = make_hardware(Some(card_id.clone()));
        assert_eq!(hardware.read_rfid_card().await, Some(card_id));
    }
}
