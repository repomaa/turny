use anyhow::{Context, Result};
use embedded_hal_bus::spi::ExclusiveDevice;
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::{Delay, SpidevBus};
use linux_embedded_hal as hal;
use log::{debug, info};
use mfrc522::comm::blocking::spi::{DummyDelay, SpiInterface};
use mfrc522::{Initialized, Mfrc522};
use rppal::gpio::OutputPin;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

/// Time to hold the RST pin low/high during MFRC522 hardware reset.
const MFRC522_RESET_DELAY: Duration = Duration::from_millis(50);

/// Type alias for a GPIO output pin shared between the HardwareManager and
/// the current RFID reader.  By sharing ownership we ensure that the pin
/// survives even when a reader is abandoned inside a leaked `spawn_blocking`
/// thread (which happens when an SPI poll times out).  Without sharing,
/// `rppal` refuses to re-acquire the pin, making reinit impossible.
pub type SharedOutputPin = Arc<StdMutex<OutputPin>>;

/// Acquire a GPIO output pin and wrap it in a `SharedOutputPin`.
pub fn acquire_shared_output_pin(gpio: &rppal::gpio::Gpio, pin_num: u8) -> Result<SharedOutputPin> {
    let pin = gpio
        .get(pin_num)
        .with_context(|| format!("Failed to get GPIO pin {}", pin_num))?
        .into_output();
    Ok(Arc::new(StdMutex::new(pin)))
}

/// Wrapper around a `SharedOutputPin` that implements embedded-hal 1.0
/// `OutputPin` traits, so it can be used as the chip-select pin for
/// `ExclusiveDevice`.
struct CsPin(SharedOutputPin);

impl embedded_hal::digital::ErrorType for CsPin {
    type Error = std::convert::Infallible;
}

impl embedded_hal::digital::OutputPin for CsPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0.lock().unwrap().set_low();
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0.lock().unwrap().set_high();
        Ok(())
    }
}

/// Trait for RFID card readers
pub trait RfidReader {
    /// Read card ID if a card is present
    fn read_card_id(&mut self) -> Option<String>;
}

type Mfrc522Device =
    Mfrc522<SpiInterface<ExclusiveDevice<SpidevBus, CsPin, Delay>, DummyDelay>, Initialized>;

/// MFRC522 RFID reader implementation
pub struct Mfrc522RfidReader {
    mfrc522: Mfrc522Device,
    /// Kept alive so the RST pin stays high (MFRC522 out of reset) even
    /// after this reader is abandoned in a leaked thread.
    _rst_pin: SharedOutputPin,
    last_read_time: Option<Instant>,
    last_card_id: Option<String>,
    read_cooldown: Duration,
}

impl Mfrc522RfidReader {
    /// Create a new MFRC522 RFID reader using shared GPIO pins.
    ///
    /// `cs_pin` and `rst_pin` are `Arc<Mutex<OutputPin>>` so that the
    /// `HardwareManager` retains ownership and can reuse them when
    /// creating a fresh reader after a timeout — without needing to
    /// re-acquire the pins from `rppal` (which would fail because the
    /// abandoned reader in the leaked thread still holds them).
    pub fn new(
        cs_pin: SharedOutputPin,
        rst_pin: SharedOutputPin,
        read_cooldown: Duration,
    ) -> Result<Self> {
        info!("Initializing MFRC522 RFID reader...");

        // Initialize SPI — a fresh file descriptor each time; the kernel
        // allows multiple opens of /dev/spidev0.0.
        let mut spi = SpidevBus::open("/dev/spidev0.0").context("Failed to open SPI device")?;
        let options = SpidevOptions::new()
            .max_speed_hz(1_000_000)
            .mode(SpiModeFlags::SPI_MODE_0 | SpiModeFlags::SPI_NO_CS)
            .build();
        spi.configure(&options).context("Failed to configure SPI")?;

        // Ensure chip select is idle (high)
        cs_pin.lock().unwrap().set_high();

        // Hardware reset the MFRC522 via the shared RST pin
        {
            let mut rst = rst_pin.lock().unwrap();
            rst.set_low();
            std::thread::sleep(MFRC522_RESET_DELAY);
            rst.set_high();
            std::thread::sleep(MFRC522_RESET_DELAY);
        }

        // Create SPI device and MFRC522 interface, init once
        let spi = ExclusiveDevice::new(spi, CsPin(cs_pin), Delay)?;
        let itf = SpiInterface::new(spi);

        info!("MFRC522 SPI interface initialized");

        let mfrc522 = match Mfrc522::new(itf).init() {
            Ok(m) => {
                info!("MFRC522 initialized successfully");
                m
            }
            Err(e) => {
                return Err(anyhow::anyhow!("MFRC522 initialization failed: {:?}", e));
            }
        };

        Ok(Self {
            mfrc522,
            _rst_pin: rst_pin,
            last_read_time: None,
            last_card_id: None,
            read_cooldown,
        })
    }

    /// Check if enough time has passed since last read
    fn can_read(&self) -> bool {
        match self.last_read_time {
            Some(last_time) => last_time.elapsed() >= self.read_cooldown,
            None => true,
        }
    }

    /// Perform the actual card reading operation
    fn read_card_internal(&mut self) -> Result<Option<String>> {
        match self.mfrc522.wupa() {
            Ok(atqa) => match self.mfrc522.select(&atqa) {
                Ok(uid) => {
                    let uid_bytes = uid.as_bytes();
                    let uid_string = uid_bytes
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<Vec<_>>()
                        .join("");

                    debug!("RFID card detected: {}", uid_string);

                    if let Err(e) = self.mfrc522.hlta() {
                        debug!("MFRC522 hlta failed (non-critical): {:?}", e);
                    }

                    Ok(Some(uid_string))
                }
                Err(e) => {
                    debug!("RFID select failed: {:?}", e);
                    Ok(None)
                }
            },
            Err(e) => {
                debug!("RFID REQA failed: {:?}", e);
                Ok(None)
            }
        }
    }
}

impl RfidReader for Mfrc522RfidReader {
    fn read_card_id(&mut self) -> Option<String> {
        if !self.can_read() {
            return None;
        }

        debug!("RFID read cooldown elapsed, polling reader...");
        match self.read_card_internal() {
            Ok(card_id) => {
                self.last_read_time = Some(Instant::now());

                if let Some(ref id) = card_id {
                    if self.last_card_id.as_ref() != Some(id) {
                        info!("RFID card detected: {}", id);
                    }
                    self.last_card_id = card_id.clone();
                } else {
                    self.last_card_id = None;
                }

                card_id
            }
            Err(e) => {
                debug!("Failed to read RFID card: {}", e);
                self.last_card_id = None;
                None
            }
        }
    }
}
