use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

// Hardware configuration constants
pub const BUTTON_PIN: u8 = 27;
pub const LED_PIN: u8 = 22;

// Timing constants
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);

// Button press duration thresholds
pub const MANUAL_RESET_THRESHOLD: Duration = Duration::from_secs(5);
pub const PREVIOUS_TRACK_THRESHOLD: Duration = Duration::from_secs(1);

/// Spotify configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

/// GPIO configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpioConfig {
    pub button_pin: u8,
    pub led_pin: u8,
    pub rfid_reset_pin: u8,
    pub rfid_sda_pin: u8,
}

/// Settings configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsConfig {
    pub poll_interval: u64,
    pub default_volume: u8,
    pub absence_threshold: u8,
}

/// Audio configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub startup_sound: String,
    pub audio_player: String,
}

/// Logging configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file: Option<String>,
}

/// Advanced configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedConfig {
    pub scopes: Vec<String>,
    pub spotifyd_service: String,
    pub spotifyd_user_service: bool,
    pub max_heartbeat_retries: u32,
    pub retry_delay_multiplier: f64,
}

/// Main configuration structure for the Turny application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnyConfig {
    pub spotify: SpotifyConfig,
    pub gpio: GpioConfig,
    pub settings: SettingsConfig,
    pub playlists: HashMap<String, String>,
    pub audio: AudioConfig,
    pub logging: LoggingConfig,
    pub advanced: AdvancedConfig,
}

impl TurnyConfig {
    /// Load configuration from a TOML file
    pub fn from_file(path: &str) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path))?;

        let config: TurnyConfig = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path))?;

        Ok(config)
    }

    /// Load configuration from environment variables with fallback to defaults
    pub fn from_env_or_default() -> Self {
        let mut config = Self::default();

        // Override with environment variables if present
        if let Ok(client_id) = std::env::var("SPOTIFY_CLIENT_ID") {
            config.spotify.client_id = client_id;
        }

        if let Ok(client_secret) = std::env::var("SPOTIFY_CLIENT_SECRET") {
            config.spotify.client_secret = client_secret;
        }

        if let Ok(redirect_uri) = std::env::var("SPOTIFY_REDIRECT_URI") {
            config.spotify.redirect_uri = redirect_uri;
        }

        config
    }

    /// Save configuration to a TOML file
    pub fn save_to_file(&self, path: &str) -> Result<()> {
        let contents =
            toml::to_string_pretty(self).context("Failed to serialize config to TOML")?;

        std::fs::write(path, contents)
            .with_context(|| format!("Failed to write config file: {}", path))?;

        Ok(())
    }

    /// Add a new RFID card to playlist mapping
    pub fn add_card_mapping(&mut self, card_id: String, playlist_uri: String) {
        self.playlists.insert(card_id, playlist_uri);
    }

    /// Remove a card mapping
    pub fn remove_card_mapping(&mut self, card_id: &str) -> Option<String> {
        self.playlists.remove(card_id)
    }

    /// Get playlist URI for a given card ID
    pub fn get_playlist_for_card(&self, card_id: &str) -> Option<&String> {
        self.playlists.get(card_id)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.spotify.client_id.is_empty() {
            return Err(anyhow::anyhow!("Spotify client ID is required"));
        }

        if self.spotify.client_secret.is_empty() {
            return Err(anyhow::anyhow!("Spotify client secret is required"));
        }

        if self.spotify.redirect_uri.is_empty() {
            return Err(anyhow::anyhow!("Spotify redirect URI is required"));
        }

        // Validate redirect URI format
        if !self.spotify.redirect_uri.starts_with("http://") && !self.spotify.redirect_uri.starts_with("https://") {
            return Err(anyhow::anyhow!(
                "Redirect URI must be a valid HTTP/HTTPS URL"
            ));
        }

        Ok(())
    }
}

impl Default for TurnyConfig {
    fn default() -> Self {
        let mut playlists = HashMap::new();

        // Default mapping for testing
        playlists.insert(
            "383951559086".to_string(),
            "spotify:playlist:4Y6ZFtrQX7vuKVGLbNQ5sN".to_string(),
        );

        Self {
            spotify: SpotifyConfig {
                client_id: "6408760457ed45538740a3f13f369722".to_string(),
                client_secret: "72ad08a2fe204c8894bdb1a7a8c9a866".to_string(),
                redirect_uri: "https://jokke.space/callback".to_string(),
            },
            gpio: GpioConfig {
                button_pin: 27,
                led_pin: 22,
                rfid_reset_pin: 25,
                rfid_sda_pin: 8,
            },
            settings: SettingsConfig {
                poll_interval: 50,
                default_volume: 70,
                absence_threshold: 3,
            },
            playlists,
            audio: AudioConfig {
                startup_sound: "startup.wav".to_string(),
                audio_player: "aplay".to_string(),
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                file: None,
            },
            advanced: AdvancedConfig {
                scopes: vec![
                    "user-read-playback-state".to_string(),
                    "user-modify-playback-state".to_string(),
                ],
                spotifyd_service: "spotifyd".to_string(),
                spotifyd_user_service: true,
                max_heartbeat_retries: 10,
                retry_delay_multiplier: 0.5,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = TurnyConfig::default();
        assert!(!config.spotify.client_id.is_empty());
        assert!(!config.spotify.client_secret.is_empty());
        assert!(!config.spotify.redirect_uri.is_empty());
        assert!(!config.playlists.is_empty());
        assert_eq!(config.audio.startup_sound, "startup.wav");
    }

    #[test]
    fn test_config_validation() {
        let config = TurnyConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid_config = config.clone();
        invalid_config.spotify.client_id = String::new();
        assert!(invalid_config.validate().is_err());

        let mut invalid_uri_config = config.clone();
        invalid_uri_config.spotify.redirect_uri = "not-a-url".to_string();
        assert!(invalid_uri_config.validate().is_err());
    }

    #[test]
    fn test_card_mapping() {
        let mut config = TurnyConfig::default();
        let card_id = "test_card".to_string();
        let playlist_uri = "spotify:playlist:test".to_string();

        config.add_card_mapping(card_id.clone(), playlist_uri.clone());
        assert_eq!(config.get_playlist_for_card(&card_id), Some(&playlist_uri));

        let removed = config.remove_card_mapping(&card_id);
        assert_eq!(removed, Some(playlist_uri));
        assert_eq!(config.get_playlist_for_card(&card_id), None);
    }
}
