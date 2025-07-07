#!/bin/bash

# Turny Spotify RFID Controller Setup Script
# This script will install and configure the Turny application

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
USER=$(whoami)
INSTALL_DIR="/home/$USER/turny"
SERVICE_NAME="turny"

# Helper functions
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_root() {
    if [[ $EUID -eq 0 ]]; then
        print_error "This script should not be run as root"
        exit 1
    fi
}

check_raspberry_pi() {
    if ! grep -q "Raspberry Pi" /proc/cpuinfo; then
        print_warning "This doesn't appear to be a Raspberry Pi"
        read -p "Continue anyway? (y/N): " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
}

install_system_deps() {
    print_status "Installing system dependencies..."
    
    # Update package list
    sudo apt update
    
    # Install required packages
    sudo apt install -y \
        build-essential \
        pkg-config \
        libssl-dev \
        libudev-dev \
        git \
        curl \
        alsa-utils \
        systemd
    
    print_success "System dependencies installed"
}

install_rust() {
    if command -v rustc &> /dev/null; then
        print_status "Rust is already installed ($(rustc --version))"
        return
    fi
    
    print_status "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source ~/.cargo/env
    
    # Add cargo to PATH for current session
    export PATH="$HOME/.cargo/bin:$PATH"
    
    print_success "Rust installed successfully"
}

setup_gpio_permissions() {
    print_status "Setting up GPIO permissions..."
    
    # Add user to gpio group
    sudo usermod -a -G gpio $USER
    
    # Create udev rules for GPIO access
    sudo tee /etc/udev/rules.d/99-gpio.rules > /dev/null << EOF
SUBSYSTEM=="gpio", GROUP="gpio", MODE="0660"
SUBSYSTEM=="spidev", GROUP="gpio", MODE="0660"
EOF
    
    # Reload udev rules
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    
    print_success "GPIO permissions configured"
}

install_spotifyd() {
    if command -v spotifyd &> /dev/null; then
        print_status "Spotifyd is already installed"
        return
    fi
    
    print_status "Installing Spotifyd..."
    
    # Install spotifyd from GitHub releases
    SPOTIFYD_VERSION=$(curl -s https://api.github.com/repos/Spotifyd/spotifyd/releases/latest | grep -oP '"tag_name": "\K(.*)(?=")')
    SPOTIFYD_URL="https://github.com/Spotifyd/spotifyd/releases/download/${SPOTIFYD_VERSION}/spotifyd-linux-armhf-slim.tar.gz"
    
    # Download and install
    cd /tmp
    curl -L $SPOTIFYD_URL -o spotifyd.tar.gz
    tar -xzf spotifyd.tar.gz
    sudo mv spotifyd /usr/local/bin/
    sudo chmod +x /usr/local/bin/spotifyd
    
    print_success "Spotifyd installed successfully"
}

build_project() {
    print_status "Building Turny application..."
    
    # Ensure we're in the project directory
    cd $INSTALL_DIR
    
    # Build in release mode
    cargo build --release
    
    print_success "Turny application built successfully"
}

create_config() {
    print_status "Creating configuration file..."
    
    if [[ ! -f "$INSTALL_DIR/config.toml" ]]; then
        cp "$INSTALL_DIR/config.toml.example" "$INSTALL_DIR/config.toml"
        print_warning "Configuration template created at $INSTALL_DIR/config.toml"
        print_warning "Please edit this file with your Spotify credentials and RFID card mappings"
    else
        print_status "Configuration file already exists"
    fi
}

install_service() {
    print_status "Installing systemd service..."
    
    # Update service file with correct paths
    sed "s|/home/pi/turny|$INSTALL_DIR|g" "$INSTALL_DIR/turny.service" > /tmp/turny.service
    sed -i "s|User=pi|User=$USER|g" /tmp/turny.service
    
    # Install service
    sudo mv /tmp/turny.service /etc/systemd/system/
    sudo systemctl daemon-reload
    sudo systemctl enable $SERVICE_NAME
    
    print_success "Systemd service installed"
}

create_startup_sound() {
    print_status "Creating startup sound..."
    
    # Create a simple beep sound if aplay is available
    if command -v speaker-test &> /dev/null; then
        timeout 1 speaker-test -t sine -f 1000 -l 1 2>/dev/null | head -n 1 > /dev/null || true
    fi
    
    # Create a simple wav file using sox if available
    if command -v sox &> /dev/null; then
        sox -n -r 44100 -c 2 "$INSTALL_DIR/startup.wav" synth 0.5 sine 800 fade 0.1 0.5 0.1 2>/dev/null || true
        print_success "Startup sound created"
    else
        print_warning "Sox not available, startup sound not created"
    fi
}

setup_spotifyd_config() {
    print_status "Setting up Spotifyd configuration..."
    
    SPOTIFYD_CONFIG_DIR="$HOME/.config/spotifyd"
    mkdir -p "$SPOTIFYD_CONFIG_DIR"
    
    if [[ ! -f "$SPOTIFYD_CONFIG_DIR/spotifyd.conf" ]]; then
        cat > "$SPOTIFYD_CONFIG_DIR/spotifyd.conf" << EOF
[global]
# Your Spotify username
username = "your_spotify_username"

# Your Spotify password
password = "your_spotify_password"

# A command that gets executed and can be used to
# retrieve your password.
# The command should return the password on stdout.
#
# This is an alternative to the password field. Both
# password and password_cmd are optional.
# password_cmd = "pass spotify"

# If set to true, audio data does not get cached.
no_audio_cache = false

# If set to true, enables volume normalisation between songs.
volume_normalisation = true

# The normalisation pregain that is applied for each song.
normalisation_pregain = -10

# The port on which the server will listen for incoming audio.
# port = 5353

# The name that gets displayed under the connect tab on
# official clients.
device_name = "Turny-Pi"

# The audio backend used to play the your music.
backend = "alsa"

# The alsa audio mixer control.
mixer = "PCM"

# The alsa audio device to stream audio to.
device = "default"

# The alsa audio control device.
control = "default"

# The displayed device type in Spotify clients.
device_type = "computer"

# The initial volume in percent.
initial_volume = "70"

# If set to true, enables shuffle mode.
shuffle = false

# If set to true, enables repeat mode.
repeat = false

# The bitrate that gets used for playback.
bitrate = 320

# If set to true, the volume set by the software mixer will make
# use of the full range of the hardware mixer.
volume_ctrl = "linear"

# A command that gets executed in your shell after each song changes.
# on_song_change_hook = "command_to_run_on_song_change"

# If set to true, `spotifyd` tries to look up the current song on the
# internet and play the found version instead of the one provided by
# Spotify's CDN.
use_mpris = false

# If set to true, `spotifyd` will use the dbus service to communicate
# with the system.
use_keyring = false

# If set to true, `spotifyd` will use the cache to store the current
# and previous song(s) data.
cache_path = "/tmp/spotifyd"

# If set to true, `spotifyd` will use the cache to store the current
# and previous song(s) data.
no_audio_cache = false

# If set to true, `spotifyd` will use the cache to store the current
# and previous song(s) data.
max_cache_size = 1000000000

# If set to true, `spotifyd` will use the dbus service to communicate
# with the system.
autoplay = false
EOF
        
        print_warning "Spotifyd configuration created at $SPOTIFYD_CONFIG_DIR/spotifyd.conf"
        print_warning "Please edit this file with your Spotify credentials"
    else
        print_status "Spotifyd configuration already exists"
    fi
}

show_completion_message() {
    print_success "Installation completed successfully!"
    echo
    echo "Next steps:"
    echo "1. Edit the configuration file: $INSTALL_DIR/config.toml"
    echo "2. Edit the Spotifyd configuration: $HOME/.config/spotifyd/spotifyd.conf"
    echo "3. Add your Spotify credentials to both files"
    echo "4. Map your RFID cards to playlists in config.toml"
    echo "5. You may need to log out and log back in for GPIO permissions to take effect"
    echo
    echo "To start the service:"
    echo "  sudo systemctl start $SERVICE_NAME"
    echo
    echo "To view logs:"
    echo "  sudo journalctl -u $SERVICE_NAME -f"
    echo
    echo "To test the application:"
    echo "  cd $INSTALL_DIR && cargo run"
    echo
    print_warning "Remember to configure your Spotify app at https://developer.spotify.com/dashboard"
    print_warning "Add your redirect URI to the app settings"
}

main() {
    echo "================================"
    echo "Turny Spotify RFID Controller"
    echo "Setup Script"
    echo "================================"
    echo
    
    check_root
    check_raspberry_pi
    
    print_status "Starting installation..."
    
    install_system_deps
    install_rust
    setup_gpio_permissions
    install_spotifyd
    build_project
    create_config
    setup_spotifyd_config
    install_service
    create_startup_sound
    
    show_completion_message
}

# Run main function
main "$@"