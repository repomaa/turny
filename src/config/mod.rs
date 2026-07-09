use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Maximum BCM GPIO pin number on the Raspberry Pi 40-pin header.
const MAX_GPIO_PIN: u8 = 27;

/// Spotify configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpotifyConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

/// GPIO configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GpioConfig {
    pub button_pin: u8,
    pub led_pin: u8,
    pub rfid_reset_pin: u8,
    pub rfid_sda_pin: u8,
}

/// Settings configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsConfig {
    pub poll_interval: u64,
    pub default_volume: u8,
    pub absence_threshold: u8,
    pub manual_reset_threshold: Duration,
    pub previous_track_threshold: Duration,
}

/// Audio configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub startup_sound: String,
}

/// Web server configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    pub host: String,
    pub port: u16,
    pub external_url: Option<String>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            external_url: None,
        }
    }
}

/// Advanced configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedConfig {
    pub scopes: Vec<String>,
}

/// Main configuration structure for the Turny application
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnyConfig {
    pub spotify: SpotifyConfig,
    pub gpio: GpioConfig,
    pub settings: SettingsConfig,
    pub playlists: HashMap<String, String>,
    pub audio: AudioConfig,
    pub web: WebConfig,
    pub advanced: AdvancedConfig,
}

impl TurnyConfig {
    /// Load configuration from a TOML file, with defaults for missing sections
    pub async fn from_file(path: &str) -> Result<Self> {
        let contents = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read config file: {}", path))?;
        let config: TurnyConfig = toml::from_str(&contents)
            .context("Failed to parse config file")?;
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
    pub async fn save_to_file(&self, path: &str) -> Result<()> {
        let contents =
            toml::to_string_pretty(self).context("Failed to serialize config to TOML")?;

        tokio::fs::write(path, contents)
            .await
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

    /// Get the poll interval as a `Duration`
    pub fn poll_interval_duration(&self) -> Duration {
        Duration::from_millis(self.settings.poll_interval)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.spotify.client_id.is_empty(),
            "Spotify client ID is required. Set it via SPOTIFY_CLIENT_ID env var or config.toml"
        );

        ensure!(
            !self.spotify.client_secret.is_empty(),
            "Spotify client secret is required. Set it via SPOTIFY_CLIENT_SECRET env var or config.toml"
        );

        ensure!(
            !self.spotify.redirect_uri.is_empty(),
            "Spotify redirect URI is required"
        );

        ensure!(
            url::Url::parse(&self.spotify.redirect_uri).is_ok(),
            "Redirect URI must be a valid HTTP/HTTPS URL"
        );

        for (name, pin) in [
            ("button_pin", self.gpio.button_pin),
            ("led_pin", self.gpio.led_pin),
            ("rfid_reset_pin", self.gpio.rfid_reset_pin),
            ("rfid_sda_pin", self.gpio.rfid_sda_pin),
        ] {
            ensure!(pin <= MAX_GPIO_PIN, "GPIO {} must be <= {}", name, MAX_GPIO_PIN);
        }

        ensure!(
            self.settings.poll_interval > 0,
            "poll_interval must be greater than 0"
        );

        ensure!(
            self.settings.default_volume <= 100,
            "default_volume must be <= 100"
        );

        Ok(())
    }
}

impl Default for TurnyConfig {
    fn default() -> Self {
        Self {
            spotify: SpotifyConfig::default(),
            gpio: GpioConfig::default(),
            settings: SettingsConfig::default(),
            audio: AudioConfig::default(),
            web: WebConfig::default(),
            advanced: AdvancedConfig::default(),
            playlists: HashMap::new(),
        }
    }
}

impl Default for SpotifyConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            redirect_uri: "https://repomaa.github.io/turny/auth-proxy/".to_string(),
        }
    }
}

impl Default for GpioConfig {
    fn default() -> Self {
        Self {
            button_pin: 27,
            led_pin: 22,
            rfid_reset_pin: 25,
            rfid_sda_pin: 8,
        }
    }
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            poll_interval: 50,
            default_volume: 70,
            absence_threshold: 30,
            manual_reset_threshold: Duration::from_secs(5),
            previous_track_threshold: Duration::from_secs(1),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            startup_sound: "startup.wav".to_string(),
        }
    }
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            scopes: vec![
                "streaming".to_string(),
                "user-read-playback-state".to_string(),
                "user-modify-playback-state".to_string(),
                "user-read-currently-playing".to_string(),
                "playlist-read-private".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = TurnyConfig::default();
        assert!(config.spotify.client_id.is_empty());
        assert!(config.spotify.client_secret.is_empty());
        assert!(!config.spotify.redirect_uri.is_empty());
        assert!(config.playlists.is_empty());
        assert_eq!(config.audio.startup_sound, "startup.wav");
    }

    #[test]
    fn test_config_validation() {
        let config = TurnyConfig::default();
        assert!(config.validate().is_err()); // empty client_id/secret

        let mut valid_config = config.clone();
        valid_config.spotify.client_id = "test_id".to_string();
        valid_config.spotify.client_secret = "test_secret".to_string();
        assert!(valid_config.validate().is_ok());

        let mut invalid_uri_config = valid_config.clone();
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

    #[test]
    fn test_gpio_pin_validation() {
        let mut config = TurnyConfig::default();
        config.spotify.client_id = "test".to_string();
        config.spotify.client_secret = "test".to_string();

        config.gpio.button_pin = 28;
        assert!(config.validate().is_err());

        config.gpio.button_pin = 27;
        config.gpio.led_pin = 28;
        assert!(config.validate().is_err());

        config.gpio.led_pin = 22;
        assert!(config.validate().is_ok());
    }
}
