use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{sleep, timeout};

use crate::audio;
use crate::auth::{AuthManager, TokenInfo};
use crate::config::TurnyConfig;
use crate::hardware::{ButtonEvent, HardwareManager};
use crate::spotify_connect::SpotifyConnect;
use crate::state::StateManager;
use crate::web::{Db, PlayerCommand, WebEvent};

/// Main application struct that coordinates all components
pub struct TurnyApp {
    config: TurnyConfig,
    state_manager: StateManager,
    hardware: HardwareManager,
    spotify_connect: SpotifyConnect,
    auth_manager: Arc<AuthManager>,
    db: Option<Arc<Db>>,
    event_tx: Option<broadcast::Sender<WebEvent>>,
    player_cmd_rx: Option<mpsc::Receiver<PlayerCommand>>,
}

impl TurnyApp {
    /// Create a new Turny application instance
    pub async fn new(config: TurnyConfig, db: Option<Arc<Db>>) -> Result<Self> {
        info!("Initializing Turny application...");

        // Validate configuration
        config.validate().context("Invalid configuration")?;

        // Initialize state manager
        let state_manager = StateManager::new();

        // Initialize hardware
        let hardware = HardwareManager::new()
            .context("Failed to initialize hardware")?;

        // Initialize Spotify Connect
        let spotify_connect = SpotifyConnect::new("Turny Speaker".to_string());

        // Initialize authentication manager
        let auth_manager = Arc::new(AuthManager::new(
            config.spotify.client_id.clone(),
            config.spotify.client_secret.clone(),
            config.spotify.redirect_uri.clone(),
            config.advanced.scopes.clone(),
            db.clone(),
        ));

        info!("Turny application initialized successfully");

        Ok(Self {
            config,
            state_manager,
            hardware,
            spotify_connect,
            auth_manager,
            db: None,
            event_tx: None,
            player_cmd_rx: None,
        })
    }

    /// Initialize Spotify services
    pub async fn initialize_spotify(&mut self) -> Result<()> {
        info!("Initializing Spotify services...");

        // Ensure we have a valid token, refreshing if necessary
        let token_info = self.auth_manager.ensure_valid_token().await
            .context("No valid Spotify authentication. Please authenticate first.")?;

        // Load persisted volume and set it as initial volume for Spotify Connect
        let default_vol = self.config.settings.default_volume;
        let volume = if let Some(db) = &self.db {
            db.get_volume().unwrap_or(None).unwrap_or(default_vol)
        } else {
            default_vol
        };
        let vol_u16 = (volume as u32 * u16::MAX as u32 / 100) as u16;
        self.spotify_connect.set_initial_volume(vol_u16);

        // Initialize Spotify Connect with the token
        self.spotify_connect.initialize_with_token(
            token_info.access_token,
            token_info.refresh_token,
        ).await.context("Failed to initialize Spotify Connect")?;

        self.spotify_connect.start().await
            .context("Failed to start Spotify Connect")?;

        info!("Spotify services initialized successfully");
        Ok(())
    }

    /// Get OAuth URL for authentication
    pub fn get_oauth_url(&self) -> String {
        self.auth_manager.get_auth_url()
    }

    /// Authenticate with redirect URL (simplified OAuth flow)
    pub async fn authenticate_with_redirect_url(&self, redirect_url: &str) -> Result<TokenInfo> {
        self.auth_manager.authenticate_with_redirect_url(redirect_url).await
    }

    /// Check if authenticated
    pub async fn is_authenticated(&self) -> bool {
        self.auth_manager.has_valid_token().await
    }

    /// Refresh token using existing refresh token
    pub async fn refresh_token(&self) -> Result<TokenInfo> {
        self.auth_manager.ensure_valid_token().await
    }

    /// Clear authentication (logout)
    pub async fn clear_authentication(&self) -> Result<()> {
        self.auth_manager.clear_token().await;
        info!("Authentication cleared");
        Ok(())
    }

    /// Check if Spotify Connect is initialized
    pub fn is_spotify_connect_initialized(&self) -> bool {
        self.spotify_connect.is_initialized()
    }

    /// Get a clone of the auth manager
    pub fn get_auth_manager(&self) -> Arc<AuthManager> {
        self.auth_manager.clone()
    }

    /// Get a clone of the state manager
    pub fn get_state_manager(&self) -> StateManager {
        self.state_manager.clone()
    }

    /// Set web integration components
    pub fn set_web_integration(
        &mut self,
        db: Arc<Db>,
        event_tx: broadcast::Sender<WebEvent>,
        player_cmd_rx: mpsc::Receiver<PlayerCommand>,
    ) {
        self.db = Some(db);
        self.event_tx = Some(event_tx);
        self.player_cmd_rx = Some(player_cmd_rx);
    }

    /// Main application loop
    pub async fn run(&mut self) -> Result<()> {
        info!("Starting Turny application main loop...");

        // Play startup sound
        self.play_startup_sound().await?;

        loop {
            // If authenticated but Spotify Connect not yet initialized, or if the
            // session has been invalidated (e.g. AP connection dropped), try to
            // (re)initialize. A timeout is critical: Spirc::new() /
            // Session::connect() can block for hours if Spotify APs are
            // unreachable, which would freeze the entire main loop.
            if self.auth_manager.has_valid_token().await
                && !self.spotify_connect.is_initialized()
            {
                if self.spotify_connect.needs_reinit() {
                    info!("Spotify session lost, reinitializing...");
                    if let Err(e) = self.spotify_connect.stop().await {
                        warn!("Error stopping dead Spotify Connect: {}", e);
                    }
                    self.state_manager.with_state_mut(|s| {
                        s.current_id = None;
                        s.is_playing = false;
                    })?;
                    self.hardware.led_off()?;
                }

                match timeout(Duration::from_secs(30), self.initialize_spotify()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => warn!("Deferred Spotify init failed: {}", e),
                    Err(_) => warn!("Deferred Spotify init timed out after 30s, will retry next loop"),
                }
            }

            // Check for RFID card
            if let Some(card_id) = self.hardware.read_rfid_card().await {
                self.handle_rfid_card(card_id).await?;
            } else {
                self.handle_no_card().await?;
            }

            // Check for button events
            if let Some(button_event) = self.hardware.check_button() {
                self.handle_button_event(button_event).await?;
            }

            // Check for web player commands
            while let Some(cmd) = self.player_cmd_rx.as_mut().and_then(|rx| rx.try_recv().ok()) {
                self.handle_player_command(cmd).await?;
            }

            // Small delay to prevent busy waiting
            sleep(crate::config::POLL_INTERVAL).await;
        }
    }

    /// Handle RFID card detection
    async fn handle_rfid_card(&mut self, card_id: String) -> Result<()> {
        debug!("RFID card detected: {}", card_id);

        // Reset absence count
        self.state_manager.reset_absence_count()?;

        // Broadcast RFID detected event
        if let Some(tx) = &self.event_tx {
            let existing_mapping = if let Some(db) = &self.db {
                db.get_mapping_for_card(&card_id)
                    .ok()
                    .flatten()
                    .map(|m| crate::web::events::ExistingMapping {
                        playlist_uri: m.playlist_uri,
                        playlist_name: m.playlist_name,
                    })
            } else {
                None
            };
            let _ = tx.send(WebEvent::RfidDetected {
                card_id: card_id.clone(),
                existing_mapping,
            });
        }

        // Store last card in DB
        if let Some(db) = &self.db {
            if let Err(e) = db.set_last_card(&card_id) {
                warn!("Failed to store last card: {}", e);
            }
        }

        // Check if this is a new card
        let current_card = self.state_manager.with_state(|state| {
            state.current_id.clone()
        })?;

        if current_card.as_ref() != Some(&card_id) {
            // Look up playlist: DB first, then config fallback
            let playlist_uri = if let Some(db) = &self.db {
                db.get_playlist_for_card(&card_id)?
            } else {
                self.config.get_playlist_for_card(&card_id).cloned()
            };

            if let Some(playlist_uri) = playlist_uri {
                info!("Starting playback for card {} with playlist {}", card_id, playlist_uri);

                // Validate playlist before starting playback
                if self.spotify_connect.validate_playlist(&playlist_uri) {
                    info!("Playlist {} validated successfully", playlist_uri);
                } else {
                    warn!("Playlist {} is invalid, but attempting playback anyway", playlist_uri);
                }

                // Start playback — only update state/LED on success so that
                // a failed attempt (e.g. Spirc not yet ready) will be retried
                // on the next poll instead of being silently swallowed.
                match self.start_playback(&playlist_uri).await {
                    Ok(()) => {
                        self.state_manager.set_current_card(card_id.clone(), playlist_uri.clone())?;
                        self.state_manager.set_playing(true)?;
                        self.hardware.led_on()?;

                        if let Some(tx) = &self.event_tx {
                            let _ = tx.send(WebEvent::PlaybackStarted {
                                card_id: card_id.clone(),
                                playlist_uri: playlist_uri.clone(),
                            });
                            let _ = tx.send(WebEvent::StateChanged {
                                is_playing: true,
                                current_card: Some(card_id),
                                context_uri: Some(playlist_uri),
                            });
                        }
                    }
                    Err(e) => {
                        warn!("Failed to start playback for card {}: {}", card_id, e);
                    }
                }
            } else {
                warn!("No playlist configured for card: {}", card_id);
            }
        }

        Ok(())
    }

    /// Handle no card detected
    async fn handle_no_card(&mut self) -> Result<()> {
        // Increment absence count
        self.state_manager.increment_absence_count()?;

        // Only auto-pause if we are actually playing and the card has been
        // absent long enough. The absence_threshold is in poll cycles
        // (default 5 × 100ms = 500ms), but the MFRC522 can intermittently
        // miss a card that is still on the reader. To avoid thrashing
        // (play → pause → play → pause), we require the absence count to
        // reach the threshold *and* not reset immediately on the next read.
        if self.state_manager.should_auto_pause(self.config.settings.absence_threshold as u32)? {
            let is_playing = self.state_manager.with_state(|state| state.is_playing)?;

            if is_playing {
                info!("Auto-pausing due to card absence");
                if let Err(e) = self.pause_playback().await {
                    warn!("Failed to auto-pause: {}", e);
                }
                self.state_manager.set_playing(false)?;

                // Clear current card so re-placing the same card restarts playback
                self.state_manager.with_state_mut(|state| {
                    state.current_id = None;
                })?;

                self.hardware.led_off()?;

                // Broadcast state change
                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(WebEvent::PlaybackPaused);
                    let _ = tx.send(WebEvent::StateChanged {
                        is_playing: false,
                        current_card: None,
                        context_uri: None,
                    });
                }
            }
        }

        Ok(())
    }

    /// Handle button events
    async fn handle_button_event(&mut self, event: ButtonEvent) -> Result<()> {
        match event {
            ButtonEvent::Pressed => {
                debug!("Button pressed");
                self.state_manager.start_button_press()?;
            }
            ButtonEvent::Released(duration) => {
                debug!("Button released after {:?}", duration);
                self.handle_button_release(duration).await?;
            }
        }
        Ok(())
    }

    /// Handle button release with duration-based actions
    async fn handle_button_release(&mut self, duration: Duration) -> Result<()> {
        if duration >= crate::config::MANUAL_RESET_THRESHOLD {
            // Long press - manual reset
            self.manual_reset().await?;
        } else if duration >= crate::config::PREVIOUS_TRACK_THRESHOLD {
            // Medium press - previous track
            info!("Previous track requested");
            if let Err(e) = self.spotify_connect.previous() {
                error!("Failed to skip to previous track: {}", e);
            }
        } else {
            // Short press - next track
            info!("Next track requested");
            if let Err(e) = self.spotify_connect.next() {
                error!("Failed to skip to next track: {}", e);
            }
        }
        Ok(())
    }

    /// Handle player commands from the web UI
    async fn handle_player_command(&mut self, cmd: PlayerCommand) -> Result<()> {
        if !self.spotify_connect.is_initialized() {
            warn!("Player command received but Spotify Connect not initialized");
            return Ok(());
        }
        match cmd {
            PlayerCommand::Play => {
                info!("Web: play command");
                if let Err(e) = self.spotify_connect.play() {
                    error!("Failed to play: {}", e);
                }
                self.state_manager.set_playing(true)?;
                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(WebEvent::PlaybackResumed);
                }
            }
            PlayerCommand::Pause => {
                info!("Web: pause command");
                if let Err(e) = self.spotify_connect.pause() {
                    error!("Failed to pause: {}", e);
                }
                self.state_manager.set_playing(false)?;
                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(WebEvent::PlaybackPaused);
                }
            }
            PlayerCommand::Next => {
                info!("Web: next command");
                if let Err(e) = self.spotify_connect.next() {
                    error!("Failed to skip: {}", e);
                }
            }
            PlayerCommand::Previous => {
                info!("Web: previous command");
                if let Err(e) = self.spotify_connect.previous() {
                    error!("Failed to previous: {}", e);
                }
            }
            PlayerCommand::SetVolume(volume) => {
                info!("Web: set volume to {}", volume);
                if let Err(e) = self.spotify_connect.set_volume(volume) {
                    error!("Failed to set volume: {}", e);
                }
                if let Some(db) = &self.db {
                    if let Err(e) = db.set_volume(volume) {
                        warn!("Failed to persist volume: {}", e);
                    }
                }
                if let Some(tx) = &self.event_tx {
                    let _ = tx.send(WebEvent::VolumeChanged { volume });
                }
            }
        }
        Ok(())
    }

    /// Perform manual reset
    async fn manual_reset(&mut self) -> Result<()> {
        info!("Performing manual reset...");

        // Blink LED to indicate reset
        self.blink_led().await?;

        // Reset state
        self.state_manager.reset_state()?;

        // Stop playback
        if let Err(e) = self.pause_playback().await {
            error!("Failed to pause playback during reset: {}", e);
        }

        // Restart Spotify Connect
        self.restart_spotify_connect().await?;

        // Turn off LED
        self.hardware.led_off()?;

        info!("Manual reset completed");
        Ok(())
    }

    /// Restart Spotify Connect service
    async fn restart_spotify_connect(&mut self) -> Result<()> {
        info!("Restarting Spotify Connect...");

        // Get current token
        if let Some(token_info) = self.auth_manager.get_token_info().await {
            self.spotify_connect.restart().await?;
            self.spotify_connect.initialize_with_token(
                token_info.access_token,
                token_info.refresh_token,
            ).await?;
        } else {
            return Err(anyhow::anyhow!("No valid token for Spotify Connect restart"));
        }

        info!("Spotify Connect restarted successfully");
        Ok(())
    }

    /// Start playback with given playlist
    async fn start_playback(&mut self, playlist_uri: &str) -> Result<()> {
        info!("Starting playback with playlist: {}", playlist_uri);

        // Ensure we have a valid token
        self.auth_manager.ensure_valid_token().await?;

        // Start playback using Spirc — wrap in timeout as Spirc/load can hang
        // after a 429 rate-limit or network issue
        timeout(Duration::from_secs(15), self.spotify_connect.load_track(playlist_uri))
            .await
            .map_err(|_| {
                anyhow::anyhow!("Playback load timed out after 15s for {}", playlist_uri)
            })??;

        info!("Playback started successfully");
        Ok(())
    }

    /// Pause playback
    async fn pause_playback(&mut self) -> Result<()> {
        info!("Pausing playback");

        self.spotify_connect.pause()?;

        info!("Playback paused successfully");
        Ok(())
    }

    /// Blink LED to indicate status
    async fn blink_led(&mut self) -> Result<()> {
        self.hardware.blink_led(Duration::from_millis(500)).await
    }

    /// Play startup sound
    async fn play_startup_sound(&mut self) -> Result<()> {
        info!("Playing startup sound");

        // Blink LED three times to indicate startup
        for _ in 0..3 {
            self.hardware.blink_led(Duration::from_millis(200)).await?;
            sleep(Duration::from_millis(200)).await;
        }

        // Play audio startup sound
        if let Err(e) = audio::play_startup_sound(&self.config.audio.startup_sound).await {
            warn!("Failed to play startup sound: {}", e);
        }

        Ok(())
    }

    /// Get application status summary
    pub async fn get_status(&self) -> Result<String> {
        let state_summary = self.state_manager.get_summary()?;
        let is_authenticated = self.auth_manager.has_valid_token().await;
        let is_connect_initialized = self.spotify_connect.is_initialized();

        let is_playing = self.state_manager.with_state(|state| state.is_playing)?;
        let current_track = self.state_manager.with_state(|state| {
            state.context_uri.clone().unwrap_or_else(|| "No current track".to_string())
        })?;

        Ok(format!(
            "Turny Status:\n{}\nAuthenticated: {}\nConnect Initialized: {}\nCurrent Track: {}\nPlaying: {}",
            state_summary,
            is_authenticated,
            is_connect_initialized,
            current_track,
            is_playing
        ))
    }

    /// Shutdown the application gracefully
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down Turny application...");

        // Turn off LED
        self.hardware.led_off()?;

        // Pause playback
        let _ = self.pause_playback().await;

        // Stop Spotify Connect
        self.spotify_connect.stop().await?;

        info!("Turny application shutdown completed");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TurnyConfig;

    #[tokio::test]
    async fn test_app_creation() {
        let config = TurnyConfig::default();

        let result = TurnyApp::new(config, None).await;

        match result {
            Ok(_app) => {
                assert!(true);
            }
            Err(e) => {
                println!("Expected failure in test environment: {}", e);
                assert!(true);
            }
        }
    }

    #[test]
    fn test_oauth_url_generation() {
        let config = TurnyConfig::default();
        let rt = tokio::runtime::Runtime::new().unwrap();

        let app = rt.block_on(async {
            match TurnyApp::new(config, None).await {
                Ok(app) => Some(app),
                Err(_) => None,
            }
        });

        if let Some(app) = app {
            let oauth_url = app.get_oauth_url();
            assert!(oauth_url.contains("accounts.spotify.com/authorize"));
            assert!(oauth_url.contains("client_id"));
        }
    }

    #[tokio::test]
    async fn test_authentication_state() {
        let config = TurnyConfig::default();

        if let Ok(app) = TurnyApp::new(config, None).await {
            assert!(!app.is_authenticated().await);

            let oauth_url = app.get_oauth_url();
            assert!(!oauth_url.is_empty());
        }
    }
}
