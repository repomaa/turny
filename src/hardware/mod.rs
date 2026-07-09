pub mod gpio;
pub mod rfid;

use anyhow::Result;
use log::warn;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::timeout;

// Re-export commonly used types
pub use gpio::{ButtonEvent, ButtonReader, LedController};
pub use rfid::RfidReader;

/// Hardware manager that coordinates all hardware components
pub struct HardwareManager {
    pub button: Box<dyn ButtonReader + Send>,
    pub led: Box<dyn LedController + Send>,
    rfid_reader: Arc<Mutex<Box<dyn RfidReader + Send>>>,
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
            rfid_reader: Arc::new(Mutex::new(rfid_reader)),
        })
    }

    /// Read RFID card ID if available.
    ///
    /// The MFRC522 SPI bus can block indefinitely when the reader gets into a
    /// bad state. We run the poll on a blocking thread with a timeout so the
    /// async main loop never freezes.
    pub async fn read_rfid_card(&self) -> Option<String> {
        let reader = self.rfid_reader.clone();
        let result = timeout(
            Duration::from_secs(2),
            tokio::task::spawn_blocking(move || {
                let mut r = reader.blocking_lock();
                r.read_card_id()
            }),
        )
        .await;

        match result {
            Ok(Ok(card_id)) => card_id,
            Ok(Err(e)) => {
                warn!("RFID polling thread panicked: {}", e);
                None
            }
            Err(_) => {
                warn!("RFID poll timed out after 2s, reader may be stuck");
                None
            }
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

    #[test]
    fn test_hardware_manager_creation() {
        let button = Box::new(MockButtonReader::new(vec![]));
        let led = Box::new(MockLedController::new());
        let rfid_reader: Box<dyn RfidReader + Send> = Box::new(MockRfidReader::new(None));

        let _hardware = HardwareManager {
            button,
            led,
            rfid_reader: Arc::new(Mutex::new(rfid_reader)),
        };

        // Hardware manager created successfully
        assert!(true);
    }

    #[tokio::test]
    async fn test_led_control() {
        let button = Box::new(MockButtonReader::new(vec![]));
        let led = Box::new(MockLedController::new());
        let rfid_reader: Box<dyn RfidReader + Send> = Box::new(MockRfidReader::new(None));

        let mut hardware = HardwareManager {
            button,
            led,
            rfid_reader: Arc::new(Mutex::new(rfid_reader)),
        };

        // Test LED control through hardware manager
        hardware.led_on().unwrap();
        hardware.led_off().unwrap();
        // LED operations completed successfully
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
            rfid_reader: Arc::new(Mutex::new(rfid_reader)),
        };

        assert!(matches!(hardware.check_button(), Some(ButtonEvent::Pressed)));
        assert!(matches!(hardware.check_button(), Some(ButtonEvent::Released(_))));
        assert!(hardware.check_button().is_none());
    }

    #[tokio::test]
    async fn test_rfid_reading() {
        let card_id = "test_card_123".to_string();
        let button = Box::new(MockButtonReader::new(vec![]));
        let led = Box::new(MockLedController::new());
        let rfid_reader: Box<dyn RfidReader + Send> = Box::new(MockRfidReader::new(Some(card_id.clone())));

        let hardware = HardwareManager {
            button,
            led,
            rfid_reader: Arc::new(Mutex::new(rfid_reader)),
        };

        assert_eq!(hardware.read_rfid_card().await, Some(card_id));
        // RFID reading test completed successfully
        assert!(true);
    }
}