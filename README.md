# Turny - Rust Spotify RFID Controller

A Rust implementation of a Spotify RFID controller for Raspberry Pi that allows you to control music playback by placing RFID cards on a reader.

## Features

- **RFID-based Playlist Control**: Place RFID cards to start playing specific Spotify playlists
- **Spotify Connect Integration**: Uses librespot for direct Spotify Connect playback
- **Button Controls**:
  - Short press: Next track
  - Long press (1+ seconds): Previous track
  - Very long press (5+ seconds): Manual system reset
- **LED Feedback**: Visual indicators for playback status and system state
- **Automatic Pause**: Removes RFID card to pause playback
- **Web UI**: SvelteKit frontend for configuration, card management, and player control via WebSocket
- **OAuth Authentication**: Spotify OAuth flow handled via web UI with token persisted in SQLite

## Hardware Requirements

- Raspberry Pi (any model with GPIO and SPI support)
- MFRC522 RFID Reader Module
- LED connected to GPIO pin 22
- Button connected to GPIO pin 27
- MFRC522 RST connected to GPIO pin 25
- MFRC522 SDA connected to GPIO pin 8 (SPI CE0)
- RFID cards/tags
- Speakers or audio output device

## Software Requirements

- Rust 1.70+
- OpenSSL development libraries (for native-tls)
- ALSA development libraries (for librespot audio output)
- libudev development libraries (for GPIO access)
- GPIO and SPI access permissions

```bash
# Ubuntu/Debian
sudo apt install build-essential pkg-config libssl-dev libasound2-dev libudev-dev
```

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

### 3. Clone and Build

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

Copy the example config and update with your Spotify credentials:

```bash
cp config.toml.example config.toml
```

Then edit `config.toml`:

```toml
[spotify]
client_id = "your_spotify_client_id"
client_secret = "your_spotify_client_secret"
redirect_uri = "https://repomaa.github.io/turny/auth-proxy/"

[gpio]
button_pin = 27
led_pin = 22
rfid_reset_pin = 25
rfid_sda_pin = 8

[settings]
default_volume = 70

[playlists]
# Card mappings are managed via the web UI and stored in SQLite.
# Entries here are migrated to the database on first run only.
"your_rfid_card_id" = "spotify:playlist:your_playlist_id"
```

Alternatively, set environment variables: `SPOTIFY_CLIENT_ID`, `SPOTIFY_CLIENT_SECRET`, `SPOTIFY_REDIRECT_URI`.

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
# Development mode (hardware mode, requires Raspberry Pi)
cargo run

# Production mode
./target/release/turny

# With logging
RUST_LOG=info ./target/release/turny

# Web server only (no hardware required)
./target/release/turny web

# CLI commands
./target/release/turny auth login      # Authenticate with Spotify
./target/release/turny auth status     # Check auth status
./target/release/turny config show     # Show configuration
./target/release/turny cards list      # List card mappings
./target/release/turny status          # Show application status
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

1. **TurnyConfig**: Configuration struct with Spotify credentials, GPIO pins, and card mappings (loaded from `config.toml`)
2. **StateManager**: Thread-safe state management with Arc/Mutex
3. **TurnyApp**: Main controller struct managing hardware, Spotify Connect, and web integration
4. **GPIO Control**: Direct hardware control via rppal crate
5. **Spotify Integration**: librespot crate for Spotify Connect playback
6. **Web Server**: axum-based REST API and WebSocket server with embedded SvelteKit frontend
7. **Database**: SQLite (rusqlite) for card mappings, token storage, and settings

### Dependencies

- `librespot`: Spotify Connect integration (playback via ALSA)
- `rppal`: Raspberry Pi GPIO access
- `mfrc522`: RFID reader communication via SPI
- `axum`: Web server with WebSocket support
- `rusqlite`: SQLite database for card mappings and token storage
- `reqwest`: HTTP client for Spotify Web API
- `tokio`: Async runtime
- `anyhow`: Error handling
- `rodio`: Audio playback (startup sound)
- `rust-embed`: Embedded frontend assets

## Differences from Python Version

### Advantages

- **Performance**: Significantly faster startup and execution
- **Memory Usage**: Lower memory footprint
- **Reliability**: Compile-time error checking prevents many runtime issues
- **Concurrency**: Better async/await support with Tokio

### Current Limitations

- **Hardware Required**: Full playback mode requires Raspberry Pi GPIO and SPI
- **Web-only Mode**: `turny web` starts the server without hardware for configuration and auth

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

1. **Multiple Devices**: Support for multiple Spotify devices
2. **Interrupt-driven RFID**: Use the IRQ pin for interrupt-driven card detection instead of polling
3. **Playlist Browsing**: Enhanced web UI for browsing and selecting playlists

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
- Verify Spotify app credentials in `config.toml`
- Check network connectivity
- Authenticate via the web UI at `http://<pi-ip>:8080`
- Check logs for token refresh errors

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
