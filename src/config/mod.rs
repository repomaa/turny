use anyhow::{Context, Result};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// Hardware configuration constants
pub const BUTTON_PIN: u8 = 27;
pub const LED_PIN: u8 = 22;
pub const DEVICE_ID: &str = "d295ff8dc55fa0b2ec7f612119675301d38f802c";

// Timing constants
pub const POLL_INTERVAL: Duration = Duration::from_millis(50);

// Button press duration thresholds
pub const MANUAL_RESET_THRESHOLD: Duration = Duration::from_secs(5);
pub const PREVIOUS_TRACK_THRESHOLD: Duration = Duration::from_secs(1);

/// Main configuration structure for the Turny application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnyConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub device_id: String,
    pub playlist_map: HashMap<String, String>,
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
            config.client_id = client_id;
        }
        
        if let Ok(client_secret) = std::env::var("SPOTIFY_CLIENT_SECRET") {
            config.client_secret = client_secret;
        }
        
        if let Ok(redirect_uri) = std::env::var("SPOTIFY_REDIRECT_URI") {
            config.redirect_uri = redirect_uri;
        }
        
        if let Ok(device_id) = std::env::var("SPOTIFY_DEVICE_ID") {
            config.device_id = device_id;
        }
        
        config
    }

    /// Save configuration to a TOML file
    pub fn save_to_file(&self, path: &str) -> Result<()> {
        let contents = toml::to_string_pretty(self)
            .context("Failed to serialize config to TOML")?;
        
        std::fs::write(path, contents)
            .with_context(|| format!("Failed to write config file: {}", path))?;
        
        Ok(())
    }

    /// Add a new RFID card to playlist mapping
    pub fn add_card_mapping(&mut self, card_id: String, playlist_uri: String) {
        self.playlist_map.insert(card_id, playlist_uri);
    }

    /// Remove a card mapping
    pub fn remove_card_mapping(&mut self, card_id: &str) -> Option<String> {
        self.playlist_map.remove(card_id)
    }

    /// Get playlist URI for a given card ID
    pub fn get_playlist_for_card(&self, card_id: &str) -> Option<&String> {
        self.playlist_map.get(card_id)
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.client_id.is_empty() {
            return Err(anyhow::anyhow!("Spotify client ID is required"));
        }
        
        if self.client_secret.is_empty() {
            return Err(anyhow::anyhow!("Spotify client secret is required"));
        }
        
        if self.redirect_uri.is_empty() {
            return Err(anyhow::anyhow!("Spotify redirect URI is required"));
        }
        
        if self.device_id.is_empty() {
            return Err(anyhow::anyhow!("Spotify device ID is required"));
        }
        
        // Validate redirect URI format
        if !self.redirect_uri.starts_with("http://") && !self.redirect_uri.starts_with("https://") {
            return Err(anyhow::anyhow!("Redirect URI must be a valid HTTP/HTTPS URL"));
        }
        
        Ok(())
    }
}

impl Default for TurnyConfig {
    fn default() -> Self {
        let mut playlist_map = HashMap::new();
        
        // Default mapping for testing
        playlist_map.insert(
            "383951559086".to_string(),
            "spotify:playlist:4Y6ZFtrQX7vuKVGLbNQ5sN".to_string(),
        );

        Self {
            client_id: "6408760457ed45538740a3f13f369722".to_string(),
            client_secret: "72ad08a2fe204c8894bdb1a7a8c9a866".to_string(),
            redirect_uri: "https://jokke.space/callback".to_string(),
            device_id: DEVICE_ID.to_string(),
            playlist_map,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = TurnyConfig::default();
        assert!(!config.client_id.is_empty());
        assert!(!config.client_secret.is_empty());
        assert!(!config.redirect_uri.is_empty());
        assert!(!config.device_id.is_empty());
        assert!(!config.playlist_map.is_empty());
    }

    #[test]
    fn test_config_validation() {
        let config = TurnyConfig::default();
        assert!(config.validate().is_ok());
        
        let mut invalid_config = config.clone();
        invalid_config.client_id = String::new();
        assert!(invalid_config.validate().is_err());
        
        let mut invalid_uri_config = config.clone();
        invalid_uri_config.redirect_uri = "not-a-url".to_string();
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