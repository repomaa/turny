use anyhow::{Context, Result};
use librespot::core::{session::Session, spotify_id::SpotifyId};
use librespot::metadata::{Playlist, Metadata};
use librespot::playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    player::{Player, PlayerEvent, PlayerEventChannel},
};
use log::{error, info};
use std::sync::Arc;
use tokio::sync::mpsc;


/// Simplified Spotify manager using librespot Player for most operations
pub struct SpotifyPlayerManager {
    player: Option<Arc<Player>>,
    event_channel: Option<PlayerEventChannel>,
    session: Option<Session>,
    current_track: Option<String>,
    is_playing: bool,
    event_sender: Option<mpsc::UnboundedSender<PlayerEvent>>,

}

impl SpotifyPlayerManager {
    /// Create a new Spotify player manager
    pub fn new() -> Self {
        let (event_sender, _) = mpsc::unbounded_channel();
        
        Self {
            player: None,
            event_channel: None,
            session: None,
            current_track: None,
            is_playing: false,
            event_sender: Some(event_sender),
        }
    }
    
    /// Initialize with a librespot session
    pub async fn initialize_with_session(&mut self, session: Session) -> Result<()> {
        info!("Initializing Spotify player with librespot session");
        
        // Store session
        self.session = Some(session.clone());
        
        // Create player config
        let player_config = PlayerConfig::default();
        
        // Create audio format
        let audio_format = AudioFormat::default();
        
        // Create audio backend
        let backend = audio_backend::find(Some("alsa".to_string()))
            .unwrap_or_else(|| audio_backend::find(None).unwrap());
        
        // Create player
        let player = Player::new(
            player_config,
            session,
            Box::new(librespot::playback::mixer::NoOpVolume),
            move || backend(None, audio_format),
        );
        
        // Get event channel
        let event_channel = player.get_player_event_channel();
        
        self.player = Some(player);
        self.event_channel = Some(event_channel);
        
        // Start event processing
        self.start_event_processing().await?;
        
        info!("Spotify player initialized successfully");
        Ok(())
    }
    
    /// Start processing player events
    async fn start_event_processing(&mut self) -> Result<()> {
        if let Some(mut event_channel) = self.event_channel.take() {
            let sender = self.event_sender.take().unwrap();
            
            tokio::spawn(async move {
                while let Some(event) = event_channel.recv().await {
                    if let Err(e) = sender.send(event) {
                        error!("Failed to send player event: {}", e);
                        break;
                    }
                }
            });
        }
        
        Ok(())
    }
    

    
    /// Start playback with a playlist URI
    pub async fn start_playback(&mut self, playlist_uri: &str) -> Result<()> {
        info!("Starting playback with playlist: {}", playlist_uri);
        
        let player = self.player.as_ref()
            .context("Player not initialized")?;
        
        let session = self.session.as_ref()
            .context("Session not available")?;
        
        // Parse playlist URI to get playlist ID
        let playlist_id = if playlist_uri.starts_with("spotify:playlist:") {
            SpotifyId::from_uri(playlist_uri)
                .context("Invalid playlist URI")?
        } else {
            return Err(anyhow::anyhow!("Unsupported URI format: {}", playlist_uri));
        };
        
        // Get playlist metadata
        let playlist = Playlist::get(session, &playlist_id).await
            .context("Failed to get playlist")?;
        
        // Start with first track if available
        if let Some(&track_id) = playlist.tracks().next() {
            player.load(track_id, true, 0);
            info!("Started playback with first track from playlist");
        } else {
            return Err(anyhow::anyhow!("Playlist is empty"));
        }
        
        Ok(())
    }
    
    /// Pause playback
    pub fn pause_playback(&self) -> Result<()> {
        info!("Pausing playback");
        
        let player = self.player.as_ref()
            .context("Player not initialized")?;
        
        player.pause();
        Ok(())
    }
    

    
    /// Stop playback
    pub fn stop_playback(&self) -> Result<()> {
        info!("Stopping playback");
        
        let player = self.player.as_ref()
            .context("Player not initialized")?;
        
        player.stop();
        Ok(())
    }
    
    /// Skip to next track (simplified - would need playlist management)
    pub fn next_track(&self) -> Result<()> {
        info!("Skipping to next track");
        // This would require playlist management logic
        // For now, just stop current track
        self.stop_playback()
    }
    
    /// Skip to previous track (simplified - would need playlist management)
    pub fn previous_track(&self) -> Result<()> {
        info!("Skipping to previous track");
        // This would require playlist management logic
        // For now, just restart current track
        let player = self.player.as_ref()
            .context("Player not initialized")?;
        
        player.seek(0);
        Ok(())
    }
    
    /// Get current track information
    pub fn get_current_track(&self) -> Option<String> {
        self.current_track.clone()
    }
    

    
    /// Check if currently playing
    pub fn is_playing(&self) -> bool {
        self.is_playing
    }
    
    /// Validate playlist URI using SpotifyId::from_uri
    pub fn validate_playlist(&self, playlist_uri: &str) -> bool {
        SpotifyId::from_uri(playlist_uri).is_ok()
    }
    

    

    
    /// Get playback summary
    pub async fn get_playback_summary(&self) -> Result<String> {
        let track = self.get_current_track().unwrap_or_else(|| "No track".to_string());
        let playing = if self.is_playing() { "Playing" } else { "Paused" };
        
        Ok(format!("Track: {} | Status: {}", track, playing))
    }
    

}

impl Drop for SpotifyPlayerManager {
    fn drop(&mut self) {
        if let Some(player) = &self.player {
            player.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_creation() {
        let player = SpotifyPlayerManager::new();
        assert!(!player.is_playing());
        assert!(player.get_current_track().is_none());
    }

    #[test]
    fn test_validate_playlist_uri() {
        let player = SpotifyPlayerManager::new();
        
        // Test with valid URI format
        let valid_uri = "spotify:playlist:4Y6ZFtrQX7vuKVGLbNQ5sN";
        assert!(player.validate_playlist(valid_uri));
        
        // Test with invalid URI format
        let invalid_uri = "invalid_uri";
        assert!(!player.validate_playlist(invalid_uri));
    }
}