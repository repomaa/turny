use anyhow::{Context, Result};
use librespot::core::{
    authentication::Credentials,
    config::SessionConfig,
    session::Session,
    spotify_uri::SpotifyUri,
};
use librespot::connect::{
    LoadContextOptions, LoadRequest, LoadRequestOptions, Options,
    ConnectConfig, Spirc,
};
use librespot::discovery::DeviceType;
use librespot::metadata::{Playlist, Metadata};
use librespot::playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig, Bitrate},
    player::Player,
    mixer,
};
use log::{info, warn};

pub struct SpotifyConnect {
    device_name: String,
    access_token: Option<String>,
    refresh_token: Option<String>,
    session: Option<Session>,
    spirc: Option<Spirc>,
    spirc_task: Option<tokio::task::JoinHandle<()>>,
    initial_volume: u16,
}

impl SpotifyConnect {
    pub fn new(device_name: String) -> Self {
        Self {
            device_name,
            access_token: None,
            refresh_token: None,
            session: None,
            spirc: None,
            spirc_task: None,
            initial_volume: u16::MAX / 2,
        }
    }

    pub async fn initialize_with_token(
        &mut self,
        access_token: String,
        refresh_token: Option<String>,
    ) -> Result<()> {
        self.access_token = Some(access_token);
        self.refresh_token = refresh_token;
        self.initialize().await
    }

    pub async fn initialize(&mut self) -> Result<()> {
        use librespot::playback::mixer::MixerConfig;
        info!("Initializing Spotify Connect with librespot...");

        let session_config = SessionConfig::default();
        let session = Session::new(session_config, None);

        let credentials = if let Some(token) = &self.access_token {
            Credentials::with_access_token(token)
        } else {
            return Err(anyhow::anyhow!(
                "No access token available. Please authenticate first."
            ));
        };

        let backend = audio_backend::find(Some("alsa".to_string()))
            .context("Failed to find ALSA audio backend")?;

        let player_config = PlayerConfig {
            bitrate: Bitrate::Bitrate160,
            gapless: true,
            ..Default::default()
        };

        let audio_format = AudioFormat::default();

        let mixer_builder =
            mixer::find(Some("softvol")).context("Failed to find softvol mixer")?;
        let mixer = mixer_builder(MixerConfig::default())
            .context("Failed to create mixer")?;

        let player = Player::new(
            player_config,
            session.clone(),
            mixer.get_soft_volume(),
            move || backend(None, audio_format),
        );

        let connect_config = ConnectConfig {
            name: self.device_name.clone(),
            device_type: DeviceType::Speaker,
            initial_volume: self.initial_volume,
            is_group: false,
            ..Default::default()
        };

        let (spirc, spirc_task) = Spirc::new(
            connect_config,
            session.clone(),
            credentials,
            player,
            mixer,
        )
        .await
        .context("Failed to create Spirc")?;

        self.session = Some(session);
        self.spirc = Some(spirc);
        self.spirc_task = Some(tokio::spawn(spirc_task));

        info!("Spotify Connect initialized successfully");
        Ok(())
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Spotify Connect service started");
        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(spirc) = self.spirc.take() {
            if let Err(e) = spirc.shutdown() {
                warn!("Spirc shutdown error: {}", e);
            }
        }

        if let Some(task) = self.spirc_task.take() {
            info!("Stopping Spotify Connect service...");
            task.abort();
            let _ = task.await;
            info!("Spotify Connect service stopped");
        }

        self.session = None;
        Ok(())
    }

    pub async fn restart(&mut self) -> Result<()> {
        info!("Restarting Spotify Connect...");

        self.stop().await?;

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        self.initialize().await?;
        self.start().await?;

        info!("Spotify Connect restarted successfully");
        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self.session.is_some()
    }

    pub fn set_initial_volume(&mut self, volume: u16) {
        self.initial_volume = volume;
    }

    pub fn set_volume(&self, volume_pct: u8) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        let volume = (volume_pct as u32 * u16::MAX as u32 / 100) as u16;
        spirc.set_volume(volume).context("Failed to set volume")
    }

    pub async fn load_track(&self, playlist_uri: &str) -> Result<()> {
        let spirc = self
            .spirc
            .as_ref()
            .context("Spirc not initialized")?;

        let session = self
            .session
            .as_ref()
            .context("Session not available")?;

        let uri = SpotifyUri::from_uri(playlist_uri)
            .context("Invalid playlist URI")?;

        let playlist = Playlist::get(session, &uri)
            .await
            .context("Failed to get playlist")?;

        let track_count = playlist.tracks().count();

        if track_count == 0 {
            return Err(anyhow::anyhow!("Playlist is empty"));
        }

        let request = LoadRequest::from_context_uri(
            playlist_uri.to_string(),
            LoadRequestOptions {
                start_playing: true,
                context_options: Some(LoadContextOptions::Options(Options {
                    repeat: true,
                    ..Default::default()
                })),
                ..Default::default()
            },
        );

        spirc.activate().context("Failed to activate Spirc")?;
        spirc.load(request).context("Failed to load playlist via Spirc")?;

        info!("Loaded playlist via Spirc: {} ({} tracks)", playlist_uri, track_count);
        Ok(())
    }

    pub fn play(&self) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.play().context("Failed to send play command")
    }

    pub fn pause(&self) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.pause().context("Failed to send pause command")
    }

    pub fn next(&self) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.next().context("Failed to send next command")
    }

    pub fn previous(&self) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.prev().context("Failed to send prev command")
    }

    pub fn validate_playlist(&self, playlist_uri: &str) -> bool {
        SpotifyUri::from_uri(playlist_uri).is_ok()
    }
}

impl Drop for SpotifyConnect {
    fn drop(&mut self) {
        if let Some(spirc) = self.spirc.take() {
            if let Err(e) = spirc.shutdown() {
                warn!("Spirc shutdown error during drop: {}", e);
            }
        }
        if let Some(task) = self.spirc_task.take() {
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
        let spotify_connect = SpotifyConnect::new("Test Device".to_string());

        assert_eq!(spotify_connect.device_name, "Test Device");
        assert!(!spotify_connect.is_initialized());
    }
}
