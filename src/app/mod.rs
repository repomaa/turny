use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{sleep, timeout};

use crate::audio;
use crate::auth::{AuthManager, PlaybackAuth, TokenInfo};
use crate::config::TurnyConfig;
use crate::hardware::{ButtonEvent, HardwareManager};
use crate::spotify_connect::{pct_to_librespot_volume, SpotifyConnect};
use crate::state::StateManager;
use crate::web::spotify_api::SpotifyApi;
use crate::web::{Db, PlayerCommand, WebEvent};

/// Main application struct that coordinates all components
pub struct TurnyApp {
    config: TurnyConfig,
    state_manager: StateManager,
    hardware: HardwareManager,
    spotify_connect: SpotifyConnect,
    spotify_api: SpotifyApi,
    auth_manager: Arc<AuthManager>,
    playback_auth: Arc<PlaybackAuth>,
    db: Option<Arc<Db>>,
    event_tx: Option<broadcast::Sender<WebEvent>>,
    player_cmd_rx: Option<mpsc::Receiver<PlayerCommand>>,
    /// Set when a long-press restart-track action has already been triggered
    /// (so the subsequent `Released` event doesn't fire another action).
    restart_track_triggered: bool,
}

impl TurnyApp {
    /// Create a new Turny application instance
    pub async fn new(
        config: TurnyConfig,
        db: Option<Arc<Db>>,
        event_tx: Option<broadcast::Sender<WebEvent>>,
        player_cmd_rx: Option<mpsc::Receiver<PlayerCommand>>,
    ) -> Result<Self> {
        info!("Initializing Turny application...");

        // Validate configuration
        config.validate().context("Invalid configuration")?;

        // Initialize state manager
        let state_manager = StateManager::new();

        // Initialize hardware — wrapped in spawn_blocking because
        // HardwareManager::new() performs blocking GPIO/SPI init including
        // std::thread::sleep for the MFRC522 hardware reset sequence.
        let gpio_config = config.gpio.clone();
        let hardware = tokio::task::spawn_blocking(move || HardwareManager::new(&gpio_config))
            .await
            .context("Hardware init task panicked")?
            .context("Failed to initialize hardware")?;

        // Initialize Spotify Connect
        let spotify_connect = SpotifyConnect::new("Turny Speaker".to_string());
        let spotify_api = SpotifyApi::new();

        // Initialize authentication manager
        let auth_manager = Arc::new(
            AuthManager::new(
                config.spotify.client_id.clone(),
                config.spotify.client_secret.clone(),
                config.spotify.redirect_uri.clone(),
                config.advanced.scopes.clone(),
                db.clone(),
            )
            .await,
        );

        // Initialize playback authentication (Spotify device pairing —
        // Web API tokens are not accepted for Connect playback).
        let playback_auth = Arc::new(PlaybackAuth::new(db.clone()).await);

        info!("Turny application initialized successfully");

        Ok(Self {
            config,
            state_manager,
            hardware,
            spotify_connect,
            spotify_api,
            auth_manager,
            playback_auth,
            db,
            event_tx,
            player_cmd_rx,
            restart_track_triggered: false,
        })
    }

    /// Initialize Spotify services
    pub async fn initialize_spotify(&mut self) -> Result<()> {
        info!("Initializing Spotify services...");

        // Playback uses its own paired credentials (Spotify device flow),
        // because Spotify rejects Web API tokens at the Connect/login5 step.
        let token_info = self.playback_auth.ensure_valid_token().await.context(
            "No paired playback authentication. Pair via the web UI (Spotify device pairing).",
        )?;

        // Load persisted volume and set it as initial volume for Spotify Connect
        let default_vol = self.config.settings.default_volume;
        let volume = match &self.db {
            Some(db) => db.get_volume().await.ok().flatten().unwrap_or(default_vol),
            None => default_vol,
        };
        self.spotify_connect
            .set_initial_volume(pct_to_librespot_volume(volume));

        // Initialize Spotify Connect with the token
        self.spotify_connect
            .initialize_with_token(token_info.access_token)
            .await
            .context("Failed to initialize Spotify Connect")?;

        info!("Spotify services initialized successfully");
        Ok(())
    }

    /// Get OAuth URL for authentication (used in tests)
    #[allow(dead_code)]
    pub fn get_oauth_url(&self) -> String {
        self.auth_manager.get_auth_url()
    }

    /// Check if authenticated
    pub fn is_authenticated(&self) -> Result<bool> {
        self.auth_manager.has_valid_token()
    }

    /// Refresh token using existing refresh token
    pub async fn refresh_token(&self) -> Result<TokenInfo> {
        self.auth_manager.ensure_valid_token().await
    }

    /// Check if Spotify Connect is initialized
    pub fn is_spotify_connect_initialized(&self) -> bool {
        self.spotify_connect.is_initialized()
    }

    /// Get a clone of the auth manager
    pub fn get_auth_manager(&self) -> Arc<AuthManager> {
        self.auth_manager.clone()
    }

    /// Get a clone of the playback auth manager
    pub fn get_playback_auth(&self) -> Arc<PlaybackAuth> {
        self.playback_auth.clone()
    }

    /// Get a clone of the state manager
    pub fn get_state_manager(&self) -> StateManager {
        self.state_manager.clone()
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
            // Playback additionally requires paired device-flow credentials;
            // while unpaired the web UI pairing card drives the setup.
            if self.auth_manager.has_valid_token()?
                && !self.spotify_connect.is_initialized()
                && self.playback_auth.has_refresh_token()
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
                    // `{:#}` prints the full anyhow chain, not just the outer
                    // context — librespot errors were invisible without it.
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => warn!("Deferred Spotify init failed: {:#}", e),
                    Err(_) => {
                        warn!("Deferred Spotify init timed out after 30s, will retry next loop")
                    }
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

            // Check for long-press restart-track (fires while button is still held,
            // without waiting for release).
            if !self.restart_track_triggered {
                if let Some(hold) = self.hardware.button_hold_duration() {
                    if hold >= self.config.settings.restart_track_threshold
                        && hold < self.config.settings.manual_reset_threshold
                        && self.spotify_connect.is_initialized()
                    {
                        self.restart_track_triggered = true;
                        self.restart_current_playlist().await?;
                    }
                }
            }

            // Check for web player commands
            let pending_cmds: Vec<PlayerCommand> = if let Some(rx) = self.player_cmd_rx.as_mut() {
                let mut cmds = Vec::new();
                loop {
                    match rx.try_recv() {
                        Ok(cmd) => cmds.push(cmd),
                        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                            log::warn!("Player command channel disconnected");
                            break;
                        }
                    }
                }
                cmds
            } else {
                Vec::new()
            };
            for cmd in pending_cmds {
                self.handle_player_command(cmd).await?;
            }

            // Small delay to prevent busy waiting
            sleep(self.config.poll_interval_duration()).await;
        }
    }

    /// Handle RFID card detection
    async fn handle_rfid_card(&mut self, card_id: String) -> Result<()> {
        debug!("RFID card detected: {}", card_id);

        // Reset absence count
        self.state_manager.reset_absence_count();

        // Broadcast RFID detected event
        let existing_mapping = if let Some(db) = &self.db {
            db.get_mapping_for_card(&card_id)
                .await
                .ok()
                .flatten()
                .map(|m| crate::web::events::ExistingMapping {
                    playlist_uri: m.playlist_uri,
                    playlist_name: m.playlist_name,
                    is_audiobook: m.is_audiobook,
                })
        } else {
            None
        };
        self.broadcast(WebEvent::RfidDetected {
            card_id: card_id.clone(),
            existing_mapping,
        });

        // Store last card in DB
        if let Some(db) = &self.db {
            if let Err(e) = db.set_last_card(&card_id).await {
                warn!("Failed to store last card: {}", e);
            }
        }

        // Check if this is a new card
        let current_card = self
            .state_manager
            .with_state(|state| state.current_id.clone())?;

        if current_card.as_ref() != Some(&card_id) {
            // Look up full mapping: DB first, then config fallback
            let (playlist_uri, is_audiobook) = if let Some(db) = &self.db {
                match db.get_mapping_for_card(&card_id).await? {
                    Some(m) => (Some(m.playlist_uri), m.is_audiobook),
                    None => (None, false),
                }
            } else {
                (self.config.get_playlist_for_card(&card_id).cloned(), false)
            };

            if let Some(playlist_uri) = playlist_uri {
                info!(
                    "Starting playback for card {} with playlist {}{}",
                    card_id,
                    playlist_uri,
                    if is_audiobook { " [audiobook]" } else { "" }
                );

                // For audiobook cards, try to resume from saved position
                let (seek_to, playing_track) = if is_audiobook {
                    if let Some(db) = &self.db {
                        if let Some(progress) =
                            db.get_audiobook_progress(&card_id).await.ok().flatten()
                        {
                            info!(
                                "Resuming audiobook for card {} at track {} position {}ms",
                                card_id, progress.track_uri, progress.position_ms
                            );
                            (progress.position_ms, Some(progress.track_uri))
                        } else {
                            (0, None)
                        }
                    } else {
                        (0, None)
                    }
                } else {
                    (0, None)
                };

                // Start playback — only update state/LED on success so that
                // a failed attempt (e.g. Spirc not yet ready) will be retried
                // on the next poll instead of being silently swallowed.
                match self
                    .start_playback_with_options(
                        &playlist_uri,
                        !is_audiobook,
                        seek_to,
                        playing_track,
                    )
                    .await
                {
                    Ok(()) => {
                        self.state_manager.set_current_card_with_audiobook(
                            card_id.clone(),
                            playlist_uri.clone(),
                            is_audiobook,
                        );
                        self.state_manager.set_playing(true);
                        self.hardware.led_on()?;

                        self.broadcast(WebEvent::PlaybackStarted {
                            card_id: card_id.clone(),
                            playlist_uri: playlist_uri.clone(),
                        });
                        self.broadcast_state_change(true, Some(card_id), Some(playlist_uri));
                    }
                    Err(e) => {
                        warn!("Failed to start playback for card {}: {:#}", card_id, e);
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
        self.state_manager.increment_absence_count();

        // Only auto-pause if we are actually playing and the card has been
        // absent long enough. The absence_threshold is in poll cycles
        // (default 5 × 100ms = 500ms), but the MFRC522 can intermittently
        // miss a card that is still on the reader. To avoid thrashing
        // (play → pause → play → pause), we require the absence count to
        // reach the threshold *and* not reset immediately on the next read.
        if self
            .state_manager
            .should_auto_pause(self.config.settings.absence_threshold as u32)
        {
            let (is_playing, current_id, is_audiobook) =
                self.state_manager.with_state(|state| {
                    (
                        state.is_playing,
                        state.current_id.clone(),
                        state.is_audiobook,
                    )
                })?;

            if is_playing {
                info!("Auto-pausing due to card absence");

                // For audiobook cards, save current playback position before pausing
                if is_audiobook {
                    if let Some(ref card_id) = current_id {
                        if let Err(e) = self.save_audiobook_progress(card_id).await {
                            warn!("Failed to save audiobook progress: {}", e);
                        }
                    }
                }

                if let Err(e) = self.pause_playback().await {
                    warn!("Failed to auto-pause: {}", e);
                }
                self.state_manager.set_playing(false);

                // Clear current card so re-placing the same card restarts playback
                self.state_manager.with_state_mut(|state| {
                    state.current_id = None;
                })?;

                self.hardware.led_off()?;

                // Broadcast state change
                self.broadcast(WebEvent::PlaybackPaused);
                self.broadcast_state_change(false, None, None);
            }
        }

        Ok(())
    }

    /// Handle button events
    async fn handle_button_event(&mut self, event: ButtonEvent) -> Result<()> {
        match event {
            ButtonEvent::Pressed => {
                debug!("Button pressed");
                self.restart_track_triggered = false;
            }
            ButtonEvent::Released(duration) => {
                debug!("Button released after {:?}", duration);
                if self.restart_track_triggered {
                    // The long-press action already fired; swallow this release.
                    debug!("Swallowing button release (restart-track already triggered)");
                    return Ok(());
                }
                self.handle_button_release(duration).await?;
            }
        }
        Ok(())
    }

    /// Handle button release with duration-based actions
    async fn handle_button_release(&mut self, duration: Duration) -> Result<()> {
        if duration >= self.config.settings.manual_reset_threshold {
            // Long press - manual reset
            self.manual_reset().await?;
        } else if duration >= self.config.settings.previous_track_threshold {
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
                self.state_manager.set_playing(true);
                self.broadcast(WebEvent::PlaybackResumed);
            }
            PlayerCommand::Pause => {
                info!("Web: pause command");
                if let Err(e) = self.spotify_connect.pause() {
                    error!("Failed to pause: {}", e);
                }
                self.state_manager.set_playing(false);
                self.broadcast(WebEvent::PlaybackPaused);
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
                    if let Err(e) = db.set_volume(volume).await {
                        warn!("Failed to persist volume: {}", e);
                    }
                }
                self.broadcast(WebEvent::VolumeChanged { volume });
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
        self.state_manager.reset_state();

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
        let token_info = self
            .auth_manager
            .get_token_info()?
            .context("No valid token for Spotify Connect restart")?;

        self.spotify_connect
            .restart_with_token(token_info.access_token)
            .await?;

        info!("Spotify Connect restarted successfully");
        Ok(())
    }

    /// Start playback with given playlist (repeat on, start from beginning)
    #[allow(dead_code)]
    async fn start_playback(&mut self, playlist_uri: &str) -> Result<()> {
        self.start_playback_with_options(playlist_uri, true, 0, None)
            .await
    }

    /// Start playback with fine-grained options.
    async fn start_playback_with_options(
        &mut self,
        playlist_uri: &str,
        repeat: bool,
        seek_to: u32,
        playing_track: Option<String>,
    ) -> Result<()> {
        info!(
            "Starting playback with playlist: {}{}",
            playlist_uri,
            if !repeat { " [no repeat]" } else { "" }
        );

        // Token is ensured by the caller (initialize_spotify or main loop reinit)

        // Start playback using Spirc — wrap in timeout as Spirc/load can hang
        // after a 429 rate-limit or network issue
        timeout(
            Duration::from_secs(15),
            self.spotify_connect.load_track_with_options(
                playlist_uri,
                repeat,
                seek_to,
                playing_track,
            ),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Playback load timed out after 15s for {}", playlist_uri))??;

        info!("Playback started successfully");
        Ok(())
    }

    /// Restart the current playlist from the first track.
    async fn restart_current_playlist(&mut self) -> Result<()> {
        let context_uri = self.state_manager.with_state(|s| s.context_uri.clone())?;
        let Some(ref playlist_uri) = context_uri else {
            warn!("Restart track requested but no playlist is loaded");
            return Ok(());
        };

        info!("Restarting playlist from first track: {}", playlist_uri);

        // For audiobooks, clear saved progress so resume doesn't jump back
        let current_id = self.state_manager.with_state(|s| s.current_id.clone())?;
        let is_audiobook = self.state_manager.with_state(|s| s.is_audiobook)?;
        if is_audiobook {
            if let Some(ref card_id) = current_id {
                if let Some(db) = &self.db {
                    if let Err(e) = db.clear_audiobook_progress(card_id).await {
                        warn!("Failed to clear audiobook progress: {}", e);
                    }
                }
            }
        }

        match timeout(
            Duration::from_secs(15),
            self.spotify_connect.restart_playlist(playlist_uri),
        )
        .await
        {
            Ok(Ok(())) => {
                info!("Playlist restarted from first track");
                self.state_manager.set_playing(true);
            }
            Ok(Err(e)) => warn!("Failed to restart playlist: {}", e),
            Err(_) => warn!("Restart playlist timed out after 15s"),
        }
        Ok(())
    }

    /// Save the current audiobook playback position to the database.
    /// Queries the Spotify Web API for the currently-playing track and progress.
    async fn save_audiobook_progress(&self, card_id: &str) -> Result<()> {
        let token_info = self
            .auth_manager
            .ensure_valid_token()
            .await
            .context("No valid token to fetch currently playing")?;

        let playing = self
            .spotify_api
            .get_currently_playing(&token_info.access_token)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch currently playing: {}", e))?;

        if let Some(np) = playing {
            if let Some(ref track_uri) = np.track_uri {
                if let Some(db) = &self.db {
                    db.save_audiobook_progress(card_id, track_uri, np.progress_ms)
                        .await?;
                    info!(
                        "Saved audiobook progress for card {}: track={}, position={}ms",
                        card_id, track_uri, np.progress_ms
                    );
                }
            } else {
                warn!("Cannot save audiobook progress: track_uri missing from currently-playing response");
            }
        } else {
            debug!("No track currently playing, skipping audiobook progress save");
        }
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
        match audio::AudioManager::new() {
            Ok(audio_manager) => {
                if let Err(e) = audio_manager
                    .play_startup_sound(&self.config.audio.startup_sound)
                    .await
                {
                    warn!("Failed to play startup sound: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to initialize audio system: {}", e);
            }
        }

        Ok(())
    }

    /// Get application status summary
    pub async fn get_status(&self) -> Result<String> {
        let state_summary = self.state_manager.get_summary();
        let is_authenticated = self.auth_manager.has_valid_token()?;
        let is_connect_initialized = self.spotify_connect.is_initialized();

        let is_playing = self.state_manager.with_state(|state| state.is_playing)?;
        let current_track = self.state_manager.with_state(|state| {
            state
                .context_uri
                .clone()
                .unwrap_or_else(|| "No current track".to_string())
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

    /// Broadcast a web event to all connected WebSocket clients
    fn broadcast(&self, event: WebEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }

    /// Broadcast a playback state change event.
    fn broadcast_state_change(
        &self,
        is_playing: bool,
        card_id: Option<String>,
        context_uri: Option<String>,
    ) {
        self.broadcast(WebEvent::StateChanged {
            is_playing,
            current_card: card_id,
            context_uri,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TurnyConfig;

    #[tokio::test]
    async fn test_app_creation() {
        // On Pi: app creation succeeds. On non-Pi: fails with hardware error.
        // Both outcomes are valid — we just verify no panic.
        let config = TurnyConfig::default();
        let _ = TurnyApp::new(config, None, None, None).await;
    }

    #[tokio::test]
    async fn test_oauth_url_generation() {
        let config = TurnyConfig::default();
        if let Ok(app) = TurnyApp::new(config, None, None, None).await {
            let oauth_url = app.get_oauth_url();
            assert!(oauth_url.contains("accounts.spotify.com/authorize"));
            assert!(oauth_url.contains("client_id"));
        }
    }

    #[tokio::test]
    async fn test_authentication_state() {
        let config = TurnyConfig::default();

        if let Ok(app) = TurnyApp::new(config, None, None, None).await {
            assert!(!app.is_authenticated().unwrap_or(false));

            let oauth_url = app.get_oauth_url();
            assert!(!oauth_url.is_empty());
        }
    }
}
