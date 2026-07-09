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
    session: Option<Session>,
    spirc: Option<Spirc>,
    spirc_task: Option<tokio::task::JoinHandle<()>>,
    initial_volume: u16,
}

/// Convert a volume percentage (0–100) to the u16 range librespot expects (0–65535).
pub fn pct_to_librespot_volume(volume_pct: u8) -> u16 {
    (volume_pct.min(100) as u32 * u16::MAX as u32 / 100) as u16
}

impl SpotifyConnect {
    pub fn new(device_name: String) -> Self {
        Self {
            device_name,
            access_token: None,
            session: None,
            spirc: None,
            spirc_task: None,
            initial_volume: u16::MAX / 2,
        }
    }

    pub async fn initialize_with_token(
        &mut self,
        access_token: String,
    ) -> Result<()> {
        self.access_token = Some(access_token);
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
            normalisation: true,
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

    /// Stop the Spotify Connect service and wait for it to fully shut down.
    /// Does NOT reinitialize — call `initialize_with_token` afterwards.
    pub async fn stop_and_wait(&mut self) -> Result<()> {
        info!("Stopping Spotify Connect...");

        self.stop().await?;

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        info!("Spotify Connect stopped, ready for re-initialization");
        Ok(())
    }

    /// Returns true only when the session exists AND is still valid.
    ///
    /// When the session is `None` (never initialized), this returns false.
    pub fn is_initialized(&self) -> bool {
        self.session.as_ref().map(|s| !s.is_invalid()).unwrap_or(false)
    }

    /// Returns true only when the session exists BUT has become invalid
    /// (i.e. it was previously initialized but is now stale).
    ///
    /// When the session is `None` (never initialized), this also returns
    /// false. This is intentional: the app loop checks `!is_initialized()`
    /// first, then `needs_reinit()` to decide whether to clean up before
    /// re-initializing. A session that was never created needs neither
    /// cleanup nor re-init detection — it simply needs initialization.
    pub fn needs_reinit(&self) -> bool {
        self.session.as_ref().map(|s| s.is_invalid()).unwrap_or(false)
    }

    /// Sets the initial volume (0–65535) used when the next session is created.
    pub fn set_initial_volume(&mut self, volume: u16) {
        self.initial_volume = volume;
    }

    /// Sets the playback volume as a percentage (0–100) of the full u16 range.
    pub fn set_volume(&self, volume_pct: u8) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.set_volume(pct_to_librespot_volume(volume_pct)).context("Failed to set volume")
    }

    /// Loads and starts playing a playlist identified by its Spotify URI.
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

    /// Sends a play command to the active Spirc session.
    pub fn play(&self) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.play().context("Failed to send play command")
    }

    /// Sends a pause command to the active Spirc session.
    pub fn pause(&self) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.pause().context("Failed to send pause command")
    }

    /// Skips to the next track in the current playback context.
    pub fn next(&self) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.next().context("Failed to send next command")
    }

    /// Skips to the previous track in the current playback context.
    pub fn previous(&self) -> Result<()> {
        let spirc = self.spirc.as_ref().context("Spirc not initialized")?;
        spirc.prev().context("Failed to send prev command")
    }

    /// Convenience method that stops the Spotify Connect service and then
    /// re-initializes it with the given access token.
    pub async fn restart_with_token(
        &mut self,
        access_token: String,
    ) -> Result<()> {
        self.stop_and_wait().await?;
        self.initialize_with_token(access_token).await
    }
}

impl Drop for SpotifyConnect {
    fn drop(&mut self) {
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
