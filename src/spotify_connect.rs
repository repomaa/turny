use anyhow::{Context, Result};
use librespot::core::{
    authentication::Credentials,
    cache::Cache,
    config::SessionConfig,
    session::Session,
};
use librespot::connect::{
    config::ConnectConfig,
    spirc::Spirc,
};
use librespot::discovery::DeviceType;
use librespot::playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig, Bitrate},
    player::Player,
    mixer::{self, NoOpVolume},
};
use log::info;

pub struct SpotifyConnect {
    device_name: String,
    access_token: Option<String>,
    refresh_token: Option<String>,
    _session: Option<Session>,
    _spirc_task: Option<tokio::task::JoinHandle<()>>,
}

impl SpotifyConnect {
    pub fn new(
        device_name: String,
    ) -> Self {
        Self {
            device_name,
            access_token: None,
            refresh_token: None,
            _session: None,
            _spirc_task: None,
        }
    }

    pub async fn initialize_with_token(&mut self, access_token: String, refresh_token: Option<String>) -> Result<()> {
        self.access_token = Some(access_token);
        self.refresh_token = refresh_token;
        self.initialize().await
    }

    pub async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Spotify Connect with librespot...");

        // Create session config
        let session_config = SessionConfig::default();
        
        // Create cache directory
        let cache_dir = std::path::PathBuf::from("/tmp/librespot_cache");
        std::fs::create_dir_all(&cache_dir).context("Failed to create cache directory")?;
        
        // Create cache with proper type annotations
        let cache = Cache::new(
            None::<std::path::PathBuf>, 
            None::<std::path::PathBuf>, 
            None::<std::path::PathBuf>, 
            None::<u64>
        ).context("Failed to create cache")?;

        // Create credentials with OAuth token
        let credentials = if let Some(token) = &self.access_token {
            Credentials::with_access_token(token)
        } else {
            return Err(anyhow::anyhow!("No access token available. Please authenticate first."));
        };

        // Create session
        let session = Session::new(session_config, Some(cache));
        
        // Connect to Spotify
        session.connect(credentials, false).await
            .context("Failed to connect to Spotify")?;

        // Create audio backend (ALSA for Raspberry Pi)
        let backend = audio_backend::find(Some("alsa".to_string()))
            .context("Failed to find ALSA audio backend")?;

        // Create player config with minimal settings
        let player_config = PlayerConfig {
            bitrate: Bitrate::Bitrate160,
            gapless: true,
            ..Default::default()
        };

        // Create audio format
        let audio_format = AudioFormat::default();

        // Create player with NoOpVolume
        let player = Player::new(
            player_config,
            session.clone(),
            Box::new(NoOpVolume),
            move || backend(None, audio_format),
        );

        // Create connect config
        let connect_config = ConnectConfig {
            name: self.device_name.clone(),
            device_type: DeviceType::Speaker,
            initial_volume: Some(50),
            has_volume_ctrl: false, // Disable volume control for simplicity
            is_group: false,
        };

        // Create Spirc (Spotify Connect controller)
        let (_spirc, spirc_task) = Spirc::new(
            connect_config,
            session.clone(),
            // Empty credentials for now - this might need to be the same as session credentials
            Credentials::with_access_token(self.access_token.as_ref().unwrap()),
            player,
            // Simple mixer that doesn't do anything
            mixer::find(Some("softvol")).unwrap()(Default::default()),
        ).await.context("Failed to create Spirc")?;

        // Store session and spawn the spirc task
        self._session = Some(session);
        self._spirc_task = Some(tokio::spawn(spirc_task));

        info!("Spotify Connect initialized successfully");
        Ok(())
    }

    pub async fn start(&mut self) -> Result<()> {
        // With the new API, starting happens automatically when Spirc is created
        info!("Spotify Connect service started");
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(task) = self._spirc_task.take() {
            info!("Stopping Spotify Connect service...");
            task.abort();
            info!("Spotify Connect service stopped");
        }
        
        self._session = None;
        Ok(())
    }

    pub async fn restart(&mut self) -> Result<()> {
        info!("Restarting Spotify Connect...");
        
        // Stop current instance
        self.stop().await?;
        
        // Wait a moment
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        
        // Reinitialize and start
        self.initialize().await?;
        self.start().await?;
        
        info!("Spotify Connect restarted successfully");
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self._session.is_some()
    }

    /// Get the current session for use with other librespot components
    pub fn get_session(&self) -> Option<&Session> {
        self._session.as_ref()
    }


}

impl Drop for SpotifyConnect {
    fn drop(&mut self) {
        if let Some(task) = &self._spirc_task {
            info!("Spotify Connect service shutting down...");
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spotify_connect_creation() {
        let spotify_connect = SpotifyConnect::new(
            "Test Device".to_string(),
        );
        
        assert_eq!(spotify_connect.device_name, "Test Device");
        assert!(!spotify_connect.is_initialized());
    }
}