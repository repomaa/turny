use anyhow::{Context, Result};
use embedded_hal_bus::spi::ExclusiveDevice;
use linux_embedded_hal as hal;
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::{Delay, SpidevBus, SysfsPin};
use log::{debug, info};
use mfrc522::comm::blocking::spi::SpiInterface;
use mfrc522::Mfrc522;
use std::time::{Duration, Instant};
use embedded_hal::delay::DelayNs;

/// Trait for RFID card readers
pub trait RfidReader {
    /// Read card ID if a card is present
    fn read_card_id(&mut self) -> Option<String>;
}

/// MFRC522 RFID reader implementation
pub struct Mfrc522RfidReader {
    spi: ExclusiveDevice<SpidevBus, SysfsPin, Delay>,
    last_read_time: Option<Instant>,
    read_cooldown: Duration,
}

impl Mfrc522RfidReader {
    /// Create a new MFRC522 RFID reader
    pub fn new() -> Result<Self> {
        info!("Initializing MFRC522 RFID reader...");
        
        let mut delay = Delay;
        
        // Initialize SPI
        let mut spi = SpidevBus::open("/dev/spidev0.0")
            .context("Failed to open SPI device")?;
        let options = SpidevOptions::new()
            .max_speed_hz(1_000_000)
            .mode(SpiModeFlags::SPI_MODE_0 | SpiModeFlags::SPI_NO_CS)
            .build();
        spi.configure(&options)
            .context("Failed to configure SPI")?;

        // Setup chip select pin (GPIO8 - CE0, matching SimpleMFRC522 standard)
        let cs_pin = SysfsPin::new(8);
        cs_pin.export().context("Failed to export RFID CS pin")?;
        
        // Wait for pin to be exported
        while !cs_pin.is_exported() {}
        delay.delay_ns(500_000_000u32); // 500ms in nanoseconds
        
        let cs_pin = cs_pin.into_output_pin(embedded_hal::digital::PinState::High)
            .context("Failed to set RFID CS pin as output")?;

        // Create SPI device
        let spi = ExclusiveDevice::new(spi, cs_pin, Delay)?;
        
        info!("MFRC522 SPI interface initialized (RST shared with LED on GPIO 22)");
        
        Ok(Self {
            spi,
            last_read_time: None,
            read_cooldown: Duration::from_millis(500), // Prevent rapid re-reads
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
        // Create a simplified MFRC522 interface for each read
        let itf = SpiInterface::new(&mut self.spi);
        match Mfrc522::new(itf).init() {
            Ok(mut mfrc522) => {
                match mfrc522.reqa() {
                    Ok(atqa) => {
                        match mfrc522.select(&atqa) {
                            Ok(uid) => {
                                // Convert UID bytes to string
                                let uid_bytes = uid.as_bytes();
                                let uid_string = uid_bytes.iter()
                                    .map(|b| format!("{:02x}", b))
                                    .collect::<Vec<_>>()
                                    .join("");
                                
                                debug!("RFID card detected: {}", uid_string);
                                
                                // Halt the card to prevent repeated reads
                                let _ = mfrc522.hlta();
                                
                                Ok(Some(uid_string))
                            }
                            Err(_) => Ok(None),
                        }
                    }
                    Err(_) => Ok(None),
                }
            }
            Err(_) => Ok(None),
        }
    }
    

}

impl RfidReader for Mfrc522RfidReader {
    fn read_card_id(&mut self) -> Option<String> {
        // Check cooldown period
        if !self.can_read() {
            return None;
        }
        
        match self.read_card_internal() {
            Ok(card_id) => {
                self.last_read_time = Some(Instant::now());
                
                if let Some(ref id) = card_id {
                    info!("RFID card detected: {}", id);
                }
                
                card_id
            }
            Err(e) => {
                debug!("Failed to read RFID card: {}", e);
                None
            }
        }
    }
    

}

/// Create a mock RFID reader for testing
#[cfg(test)]
pub struct MockRfidReader {
    cards: Vec<String>,
    current_index: usize,
    available: bool,
}

#[cfg(test)]
impl MockRfidReader {
    pub fn new() -> Self {
        Self {
            cards: vec![],
            current_index: 0,
            available: true,
        }
    }
    
    pub fn with_cards(cards: Vec<String>) -> Self {
        Self {
            cards,
            current_index: 0,
            available: true,
        }
    }
    
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }
    
    pub fn add_card(&mut self, card_id: String) {
        self.cards.push(card_id);
    }
    
    pub fn clear_cards(&mut self) {
        self.cards.clear();
        self.current_index = 0;
    }
}

#[cfg(test)]
impl RfidReader for MockRfidReader {
    fn read_card_id(&mut self) -> Option<String> {
        if !self.available || self.cards.is_empty() {
            return None;
        }
        
        let card_id = self.cards[self.current_index].clone();
        self.current_index = (self.current_index + 1) % self.cards.len();
        Some(card_id)
    }
    

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_rfid_reader_creation() {
        let reader = MockRfidReader::new();
        assert!(reader.available);
        assert!(reader.cards.is_empty());
    }

    #[test]
    fn test_mock_rfid_reader_with_cards() {
        let cards = vec![
            "card1".to_string(),
            "card2".to_string(),
            "card3".to_string(),
        ];
        let mut reader = MockRfidReader::with_cards(cards.clone());
        
        assert_eq!(reader.read_card_id(), Some("card1".to_string()));
        assert_eq!(reader.read_card_id(), Some("card2".to_string()));
        assert_eq!(reader.read_card_id(), Some("card3".to_string()));
        // Should cycle back to the first card
        assert_eq!(reader.read_card_id(), Some("card1".to_string()));
    }

    #[test]
    fn test_mock_rfid_reader_availability() {
        let mut reader = MockRfidReader::new();
        reader.add_card("test_card".to_string());
        
        assert_eq!(reader.read_card_id(), Some("test_card".to_string()));
        
        reader.set_available(false);
        assert_eq!(reader.read_card_id(), None);
    }

    #[test]
    fn test_mock_rfid_reader_empty_cards() {
        let mut reader = MockRfidReader::new();
        assert_eq!(reader.read_card_id(), None);
    }

    #[test]
    fn test_mock_rfid_reader_add_clear_cards() {
        let mut reader = MockRfidReader::new();
        
        reader.add_card("card1".to_string());
        reader.add_card("card2".to_string());
        
        assert_eq!(reader.read_card_id(), Some("card1".to_string()));
        assert_eq!(reader.read_card_id(), Some("card2".to_string()));
        
        reader.clear_cards();
        assert_eq!(reader.read_card_id(), None);
    }

    #[test]
    fn test_rfid_reader_trait() {
        let mut reader: Box<dyn RfidReader> = Box::new(MockRfidReader::with_cards(vec![
            "test_card".to_string(),
        ]));
        
        assert_eq!(reader.read_card_id(), Some("test_card".to_string()));
    }
}