# Turny - Rust Spotify RFID Controller

A Rust implementation of a Spotify RFID controller for Raspberry Pi that allows you to control music playback by placing RFID cards on a reader.

## Features

- **RFID-based Playlist Control**: Place RFID cards to start playing specific Spotify playlists
- **Button Controls**:
  - Short press: Next track
  - Long press (1+ seconds): Previous track
  - Very long press (5+ seconds): Manual system reset
- **LED Feedback**: Visual indicators for playback status and system state
- **Automatic Pause**: Removes RFID card to pause playback
- **Heartbeat Monitoring**: Monitors Spotify device availability
- **Spotifyd Integration**: Automatically restarts spotifyd service when needed

## Hardware Requirements

- Raspberry Pi (any model with GPIO)
- MFRC522 RFID Reader Module
- LED connected to GPIO pin 22
- Button connected to GPIO pin 27
- RFID cards/tags
- Speakers or audio output device

## Software Requirements

- Rust 1.70+
- Spotifyd (Spotify daemon for Linux)
- OpenSSL development libraries (if using native-tls)
- GPIO access permissions

## Installation

### 1. Install System Dependencies

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install build-essential pkg-config libssl-dev

# Or for rustls (recommended to avoid OpenSSL issues)
# No additional system dependencies needed
```

### 2. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### 3. Setup Spotifyd

Follow the [Spotifyd installation guide](https://github.com/Spotifyd/spotifyd) to install and configure the Spotify daemon.

### 4. Clone and Build

```bash
git clone <repository-url>
cd turny
cargo build --release
```

## Configuration

### Spotify API Setup

1. Create a Spotify app at https://developer.spotify.com/dashboard
2. Note your Client ID and Client Secret
3. Add your redirect URI to the app settings

### Code Configuration

Update the configuration in `src/main.rs`:

```rust
impl Default for TurnyConfig {
    fn default() -> Self {
        let mut playlist_map = HashMap::new();
        // Add your RFID card IDs and corresponding playlist URIs
        playlist_map.insert(
            "YOUR_RFID_CARD_ID".to_string(),
            "spotify:playlist:YOUR_PLAYLIST_ID".to_string(),
        );

        Self {
            client_id: "YOUR_CLIENT_ID".to_string(),
            client_secret: "YOUR_CLIENT_SECRET".to_string(),
            redirect_uri: "YOUR_REDIRECT_URI".to_string(),
            playlist_map,
        }
    }
}
```

### Hardware Connections

```
MFRC522 RFID Reader:
- VCC -> 3.3V
- GND -> GND
- RST -> GPIO 25
- SDA -> GPIO 8 (SPI CE0)
- SCK -> GPIO 11 (SPI CLK)
- MOSI -> GPIO 10 (SPI MOSI)
- MISO -> GPIO 9 (SPI MISO)

LED:
- Anode -> GPIO 22
- Cathode -> GND (via resistor)

Button:
- One side -> GPIO 27
- Other side -> GND
```

## Usage

### Running the Application

```bash
# Development mode
cargo run

# Production mode
./target/release/turny

# With logging
RUST_LOG=info ./target/release/turny
```

### Setting up as a Service

Create a systemd service file:

```bash
sudo nano /etc/systemd/system/turny.service
```

```ini
[Unit]
Description=Turny Spotify RFID Controller
After=network.target

[Service]
Type=simple
User=pi
WorkingDirectory=/home/pi/turny
ExecStart=/home/pi/turny/target/release/turny
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

Enable and start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable turny
sudo systemctl start turny
```

## Implementation Details

### Architecture

The Rust implementation follows a similar structure to the Python version but with some key differences:

- **Memory Safety**: Rust's ownership system prevents common memory issues
- **Performance**: Compiled binary with zero-cost abstractions
- **Error Handling**: Comprehensive error handling with `Result` types
- **Async/Await**: Tokio runtime for asynchronous operations
- **Type Safety**: Strong typing prevents runtime errors

### Key Components

1. **TurnyConfig**: Configuration struct with Spotify credentials and playlist mappings
2. **TurnyState**: Thread-safe state management with Arc<Mutex<>>
3. **Turny**: Main controller struct managing hardware and Spotify integration
4. **GPIO Control**: Direct hardware control via rppal crate
5. **Spotify Integration**: rspotify crate for Spotify Web API

### Dependencies

- `rspotify`: Spotify Web API client
- `rppal`: Raspberry Pi GPIO access
- `mfrc522`: RFID reader communication (placeholder - needs SPI setup)
- `tokio`: Async runtime
- `anyhow`: Error handling
- `log`: Logging framework

## Differences from Python Version

### Advantages

- **Performance**: Significantly faster startup and execution
- **Memory Usage**: Lower memory footprint
- **Reliability**: Compile-time error checking prevents many runtime issues
- **Concurrency**: Better async/await support with Tokio

### Current Limitations

- **RFID Implementation**: Currently simulated - needs proper SPI setup for MFRC522
- **OAuth Flow**: Simplified - needs full OAuth implementation for production
- **Button Monitoring**: Removed from main loop - needs proper async implementation

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Check for errors
cargo check

# Run tests
cargo test
```

### Adding Features

1. **RFID Integration**: Implement proper MFRC522 SPI communication
2. **OAuth Flow**: Add complete Spotify OAuth authorization flow
3. **Configuration File**: Add external config file support
4. **Web Interface**: Add web-based configuration interface
5. **Multiple Devices**: Support for multiple Spotify devices

### Troubleshooting

#### Permission Issues
```bash
# Add user to gpio group
sudo usermod -a -G gpio $USER

# Reboot or logout/login
```

#### Compilation Issues
```bash
# Install required development packages
sudo apt install build-essential pkg-config libssl-dev

# For GPIO access issues
sudo apt install libudev-dev
```

#### Spotify Connection Issues
- Verify Spotify app credentials
- Check network connectivity
- Ensure Spotifyd is running and configured correctly
- Verify device ID matches your Spotifyd configuration

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- Original Python implementation inspiration
- Spotify Web API
- Rust community for excellent crates
- Raspberry Pi Foundation
