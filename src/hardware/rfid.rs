use anyhow::{Context, Result};
use embedded_hal_bus::spi::ExclusiveDevice;
use linux_embedded_hal as hal;
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::{Delay, SpidevBus};
use log::{debug, info};
use mfrc522::comm::blocking::spi::{DummyDelay, SpiInterface};
use mfrc522::{Initialized, Mfrc522};
use rppal::gpio::{Gpio, OutputPin};
use std::time::{Duration, Instant};

/// Time to hold the RST pin low/high during MFRC522 hardware reset.
const MFRC522_RESET_DELAY: Duration = Duration::from_millis(50);

/// Wrapper around rppal's OutputPin that implements embedded-hal 1.0 traits.
/// Replaces the deprecated sysfs GPIO interface (SysfsPin) which doesn't work
/// with newer kernels that use dynamic gpiochip base offsets.
struct CsPin(OutputPin);

impl embedded_hal::digital::ErrorType for CsPin {
    type Error = std::convert::Infallible;
}

impl embedded_hal::digital::OutputPin for CsPin {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0.set_low();
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0.set_high();
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
    _rst_pin: OutputPin,
    last_read_time: Option<Instant>,
    last_card_id: Option<String>,
    read_cooldown: Duration,
}

impl Mfrc522RfidReader {
    /// Create a new MFRC522 RFID reader
    pub fn new(rst_pin_num: u8, sda_pin_num: u8, read_cooldown: Duration) -> Result<Self> {
        info!("Initializing MFRC522 RFID reader...");

        // Initialize SPI
        let mut spi = SpidevBus::open("/dev/spidev0.0")
            .context("Failed to open SPI device")?;
        let options = SpidevOptions::new()
            .max_speed_hz(1_000_000)
            .mode(SpiModeFlags::SPI_MODE_0 | SpiModeFlags::SPI_NO_CS)
            .build();
        spi.configure(&options)
            .context("Failed to configure SPI")?;

        // Setup chip select pin (matching SimpleMFRC522 standard)
        // Using rppal which accesses /dev/gpiomem directly with BCM numbering,
        // avoiding the deprecated sysfs GPIO export that breaks on newer kernels.
        let gpio = Gpio::new().context("Failed to initialize GPIO")?;
        let mut cs_pin = gpio
            .get(sda_pin_num)
            .with_context(|| format!("Failed to get GPIO pin {}", sda_pin_num))?
            .into_output();
        cs_pin.set_high();

        // Bring MFRC522 out of reset (RST pin, active high)
        let mut rst_pin = gpio
            .get(rst_pin_num)
            .with_context(|| format!("Failed to get GPIO pin {}", rst_pin_num))?
            .into_output();
        rst_pin.set_low();
        std::thread::sleep(MFRC522_RESET_DELAY);
        rst_pin.set_high();
        std::thread::sleep(MFRC522_RESET_DELAY);

        // Create SPI device and MFRC522 interface, init once
        let spi = ExclusiveDevice::new(spi, CsPin(cs_pin), Delay)?;
        let itf = SpiInterface::new(spi);

        info!("MFRC522 SPI interface initialized (RST on GPIO {})", rst_pin_num);

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
            Ok(atqa) => {
                match self.mfrc522.select(&atqa) {
                    Ok(uid) => {
                        let uid_bytes = uid.as_bytes();
                        let uid_string = uid_bytes.iter()
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
                }
            }
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


