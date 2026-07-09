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
pub mod cli;
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

/// Default OAuth callback port. Must match `WebConfig::default().port`.
pub const DEFAULT_OAUTH_PORT: u16 = 8080;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_config_without_credentials_is_invalid() {
        let config = TurnyConfig::default();
        assert!(config.validate().is_err()); // no credentials by default
    }

    #[test]
    fn test_hardware_manager_creation() {
        // This test validates that HardwareManager::new either succeeds (on Pi)
        // or fails with an error (on non-Pi platforms). Both outcomes are valid.
        let config = TurnyConfig::default();
        let result = HardwareManager::new(&config.gpio);
        // On non-Pi platforms, expect a failure but don't assert on error text,
        // as the exact message depends on the platform and rppal version.
        match result {
            Ok(_) => { /* hardware available */ }
            Err(_) => { /* expected on non-Pi platforms */ }
        }
    }
}
