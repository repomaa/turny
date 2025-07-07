# RFID Implementation Guide for Turny

This document explains how to implement the MFRC522 RFID reader hardware interface in the Turny Spotify controller.

## Current Implementation

The current implementation uses a trait-based approach that allows for easy switching between a mock RFID reader (for testing) and actual hardware implementation.

### Architecture

```rust
pub trait RfidReader {
    fn read_card_id(&mut self) -> Option<String>;
    fn is_available(&self) -> bool;
}
```

The system currently includes:
- `MockRfidReader`: A simulated RFID reader for testing
- `Mfrc522RfidReader`: A placeholder for the actual hardware implementation

### Hardware Setup

### Required Components

1. **MFRC522 RFID Reader Module**
2. **Raspberry Pi** (any model with SPI support)
3. **Jumper wires**
4. **RFID cards/tags** for testing

### Pin Configuration

The implementation uses the exact same pins as the reference Python code:

1. **LED on GPIO 22** (matches reference.py exactly)
2. **MFRC522 RST also on GPIO 22** (shared with LED, matches SimpleMFRC522 standard)
3. **All other MFRC522 pins use standard SPI mapping**

### Wiring Diagram

Connect the MFRC522 to your Raspberry Pi as follows:

```
MFRC522    Raspberry Pi
VCC   -->  3.3V (Pin 1)
GND   -->  GND (Pin 6)
MISO  -->  GPIO9 (Pin 21)
MOSI  -->  GPIO10 (Pin 19)
SCK   -->  GPIO11 (Pin 23)
SDA   -->  GPIO8 (Pin 24) [CE0]
RST   -->  GPIO22 (Pin 15) [Shared with LED]
IRQ   -->  Not connected
```

**Important Note:**
GPIO 22 is shared between the LED and MFRC522 RST pin, exactly matching the reference Python implementation. This works because the RST pin is only used during MFRC522 initialization, while the LED is used for status indication during operation.

### Enable SPI

Make sure SPI is enabled on your Raspberry Pi:

```bash
sudo raspi-config
# Navigate to: Interface Options -> SPI -> Enable
```

## Software Implementation

### Step 1: Add Dependencies

Add the necessary dependencies to your `Cargo.toml`:

```toml
[dependencies]
# ... existing dependencies ...

# RFID reader hardware support
linux-embedded-hal = "0.4"
embedded-hal = "1.0"
embedded-hal-bus = "0.2"
```

### Step 2: Implement the Hardware Interface

Replace the placeholder `Mfrc522RfidReader` implementation with actual hardware code:

```rust
use linux_embedded_hal as hal;
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use hal::spidev::{SpiModeFlags, SpidevOptions};
use hal::{Delay, SpidevBus, SysfsPin};
use mfrc522::comm::blocking::spi::SpiInterface;
use mfrc522::{Initialized, Mfrc522};

pub struct Mfrc522RfidReader {
    mfrc522: Mfrc522<SpiInterface<ExclusiveDevice<SpidevBus, SysfsPin, Delay>>, Initialized>,
}

impl Mfrc522RfidReader {
    pub fn new() -> Result<Self> {
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

        // Setup chip select pin (GPIO22)
        let pin = SysfsPin::new(22);
        pin.export().context("Failed to export RFID CS pin")?;
        
        // Wait for pin to be exported
        while !pin.is_exported() {}
        delay.delay_ms(500u32);
        
        let pin = pin.into_output_pin(embedded_hal::digital::PinState::High)
            .context("Failed to set RFID CS pin as output")?;

        // Create SPI device
        let spi = ExclusiveDevice::new(spi, pin, Delay)?;
        let itf = SpiInterface::new(spi);
        
        // Initialize MFRC522
        let mfrc522 = Mfrc522::new(itf).init()
            .map_err(|e| anyhow::anyhow!("Failed to initialize RFID reader: {:?}", e))?;

        let version = mfrc522.version()
            .map_err(|e| anyhow::anyhow!("Failed to read MFRC522 version: {:?}", e))?;
        
        info!("MFRC522 initialized successfully, version: 0x{:x}", version);
        
        Ok(Self { mfrc522 })
    }
}

impl RfidReader for Mfrc522RfidReader {
    fn read_card_id(&mut self) -> Option<String> {
        match self.mfrc522.reqa() {
            Ok(atqa) => {
                match self.mfrc522.select(&atqa) {
                    Ok(uid) => {
                        // Convert UID bytes to string
                        let uid_bytes = uid.as_bytes();
                        let uid_string = uid_bytes.iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<Vec<_>>()
                            .join("");
                        
                        info!("RFID card detected: {}", uid_string);
                        
                        // Halt the card to prevent repeated reads
                        let _ = self.mfrc522.hlta();
                        
                        Some(uid_string)
                    }
                    Err(_) => None,
                }
            }
            Err(_) => None,
        }
    }
    
    fn is_available(&self) -> bool {
        // Check if we can read the version register
        self.mfrc522.version().is_ok()
    }
}
```

### Step 3: Testing Your Implementation

1. **Test with Mock Reader**: The system starts with a mock reader that simulates cards every 10 seconds
2. **Test Hardware**: Once you've implemented the hardware interface, the system will automatically use it
3. **Debug**: Check logs for initialization messages and card detection events

### Step 4: Configuration

Add your RFID card IDs to the playlist mapping in `TurnyConfig`:

```rust
impl Default for TurnyConfig {
    fn default() -> Self {
        let mut playlist_map = HashMap::new();
        
        // Add your card IDs here (get them from the logs when cards are detected)
        playlist_map.insert(
            "your_card_id_here".to_string(),
            "spotify:playlist:your_playlist_id".to_string(),
        );
        
        // ... rest of implementation
    }
}
```

## Troubleshooting

### Common Issues

1. **SPI Not Enabled**
   ```bash
   sudo raspi-config
   # Enable SPI in Interface Options
   ```

2. **Permission Issues**
   ```bash
   sudo usermod -a -G spi,gpio $USER
   # Logout and login again
   ```

3. **Wiring Issues**
   - Double-check all connections
   - Ensure 3.3V power (NOT 5V)
   - Use a multimeter to verify connections
   - Verify RST is connected to GPIO 22 (same as LED)

4. **Card Not Detected**
   - Try different RFID cards/tags
   - Check if the card is compatible (ISO14443A/MIFARE)
   - Verify the card is held close enough to the reader

### Debug Commands

```bash
# Check SPI devices
ls /dev/spi*

# Check GPIO exports
ls /sys/class/gpio/

# Monitor system logs
sudo journalctl -f

# Test with your application
RUST_LOG=info cargo run
```

### Card ID Format

The system converts card UIDs to hexadecimal strings. For example:
- Card UID bytes: `[0x12, 0x34, 0x56, 0x78]`
- String format: `"12345678"`

## Advanced Configuration

### Custom Card Reading Logic

You can extend the `RfidReader` trait to add more sophisticated card reading:

```rust
pub trait RfidReader {
    fn read_card_id(&mut self) -> Option<String>;
    fn is_available(&self) -> bool;
    
    // Optional: Add methods for writing to cards
    fn write_card_data(&mut self, data: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("Write not supported"))
    }
    
    // Optional: Add authentication support
    fn authenticate_card(&mut self, key: &[u8]) -> Result<()> {
        Err(anyhow::anyhow!("Authentication not supported"))
    }
}
```

### Multiple Card Support

To support multiple cards simultaneously, you could extend the interface:

```rust
pub struct CardInfo {
    pub id: String,
    pub last_seen: Instant,
    pub signal_strength: u8,
}

impl RfidReader for Mfrc522RfidReader {
    fn read_all_cards(&mut self) -> Vec<CardInfo> {
        // Implementation for reading multiple cards
        Vec::new()
    }
}
```

## Security Considerations

1. **Card Authentication**: Consider implementing MIFARE authentication for sensitive applications
2. **Encryption**: Store sensitive data encrypted on the cards
3. **Access Control**: Implement user-specific card permissions
4. **Logging**: Log all card access attempts for security auditing

## Performance Tips

1. **Polling Rate**: Adjust `POLL_INTERVAL` based on your needs (current: 50ms)
2. **Power Management**: Consider powering down the RFID reader when not in use
3. **Caching**: Cache card IDs to reduce SPI communication overhead
4. **Interrupts**: Use the IRQ pin for interrupt-driven card detection instead of polling

## Further Resources

- [MFRC522 Datasheet](https://www.nxp.com/docs/en/data-sheet/MFRC522.pdf)
- [Raspberry Pi SPI Documentation](https://www.raspberrypi.org/documentation/hardware/raspberrypi/spi/README.md)
- [mfrc522 Rust Crate Documentation](https://docs.rs/mfrc522/)
- [embedded-hal Documentation](https://docs.rs/embedded-hal/)