use anyhow::{Context, Result};
use librespot::core::{
    authentication::Credentials,
    config::SessionConfig,
    session::Session,
    spotify_id::SpotifyId,
};
use librespot::connect::{
    config::ConnectConfig,
    spirc::{Spirc, SpircLoadCommand},
};
use librespot::discovery::DeviceType;
use librespot::metadata::{Playlist, Metadata};
use librespot::playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig, Bitrate},
    player::Player,
    mixer,
};
use librespot::protocol::spirc::TrackRef;
use log::{info, warn};

pub struct SpotifyConnect {
    device_name: String,
    access_token: Option<String>,
    refresh_token: Option<String>,
    session: Option<Session>,
    spirc: Option<Spirc>,
    spirc_task: Option<tokio::task::JoinHandle<()>>,
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
        let mixer = mixer_builder(MixerConfig::default());

        let player = Player::new(
            player_config,
            session.clone(),
            mixer.get_soft_volume(),
            move || backend(None, audio_format),
        );

        let connect_config = ConnectConfig {
            name: self.device_name.clone(),
            device_type: DeviceType::Speaker,
            initial_volume: Some(50),
            has_volume_ctrl: false,
            is_group: false,
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

    pub async fn load_track(&self, playlist_uri: &str) -> Result<()> {
        let spirc = self
            .spirc
            .as_ref()
            .context("Spirc not initialized")?;

        let session = self
            .session
            .as_ref()
            .context("Session not available")?;

        let playlist_id = SpotifyId::from_uri(playlist_uri)
            .context("Invalid playlist URI")?;

        let tracks = Playlist::get(session, &playlist_id)
            .await
            .context("Failed to get playlist")?;

        let track_refs: Vec<TrackRef> = tracks
            .tracks()
            .map(|track_id| {
                let mut track_ref = TrackRef::new();
                track_ref.set_gid(track_id.to_raw().to_vec());
                track_ref
            })
            .collect();

        if track_refs.is_empty() {
            return Err(anyhow::anyhow!("Playlist is empty"));
        }

        let command = SpircLoadCommand {
            context_uri: playlist_uri.to_string(),
            start_playing: true,
            shuffle: false,
            repeat: false,
            playing_track_index: 0,
            tracks: track_refs,
        };

        spirc.load(command).context("Failed to load playlist via Spirc")?;

        info!("Loaded playlist via Spirc: {}", playlist_uri);
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
        SpotifyId::from_uri(playlist_uri).is_ok()
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
