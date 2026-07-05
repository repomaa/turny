//! Turny Music Player Library
//!
//! A Rust library for controlling Spotify playback using RFID cards on Raspberry Pi.
//!
//! This library provides modular components for:
//! - Hardware control (GPIO, RFID, LED)
//! - Spotify Connect (librespot) integration
//! - OAuth authentication
//! - Application state management
//! - Configuration management

pub mod app;
pub mod audio;
pub mod auth;
pub mod config;
pub mod hardware;
pub mod spotify_connect;
pub mod state;
pub mod web;

// Re-export commonly used types for convenience
pub use app::TurnyApp;
pub use auth::{AuthManager, TokenInfo};
pub use config::TurnyConfig;
pub use hardware::{ButtonEvent, HardwareManager};
pub use spotify_connect::SpotifyConnect;
pub use state::{StateManager, TurnyState};

/// Common result type used throughout the library
pub type Result<T> = anyhow::Result<T>;

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default configuration file path
pub const DEFAULT_CONFIG_PATH: &str = "config.toml";

/// Default OAuth callback port
pub const DEFAULT_OAUTH_PORT: u16 = 8080;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_config_creation() {
        let config = TurnyConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_hardware_manager_creation() {
        // This test may fail without actual hardware
        let _ = HardwareManager::new();
    }
}
