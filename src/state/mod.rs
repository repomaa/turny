use anyhow::Result;
use log::error;
use std::sync::{Arc, Mutex};

/// Application state for the Turny music player
#[derive(Debug)]
pub struct TurnyState {
    /// Currently playing RFID card ID
    pub current_id: Option<String>,
    /// Current Spotify context URI (playlist, album, etc.)
    pub context_uri: Option<String>,
    /// Whether music is currently playing
    pub is_playing: bool,
    /// Count of consecutive card absences (for auto-pause)
    pub absence_count: u32,
}

impl Default for TurnyState {
    fn default() -> Self {
        Self {
            current_id: None,
            context_uri: None,
            is_playing: false,
            absence_count: 0,
        }
    }
}

impl TurnyState {
    /// Create a new state instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset the state to default values
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Set the current card and context. Also resets absence_count to 0
    /// since the card is now present.
    pub fn set_current_card(&mut self, card_id: String, context_uri: String) {
        self.current_id = Some(card_id);
        self.context_uri = Some(context_uri);
        self.absence_count = 0;
    }

    /// Set the playback state
    pub fn set_playing(&mut self, is_playing: bool) {
        self.is_playing = is_playing;
    }

    /// Increment the absence count
    pub fn increment_absence_count(&mut self) {
        self.absence_count = self.absence_count.saturating_add(1);
    }

    /// Reset the absence count
    pub fn reset_absence_count(&mut self) {
        self.absence_count = 0;
    }

    /// Check if the absence count exceeds the threshold
    pub fn should_auto_pause(&self, threshold: u32) -> bool {
        self.absence_count >= threshold
    }

    /// Get a summary of the current state
    pub fn summary(&self) -> String {
        format!(
            "State: card={}, context={}, playing={}, absences={}",
            self.current_id.as_deref().unwrap_or("none"),
            self.context_uri.as_deref().unwrap_or("none"),
            self.is_playing,
            self.absence_count
        )
    }
}

/// Thread-safe state manager for the Turny application
#[derive(Debug, Clone)]
pub struct StateManager {
    state: Arc<Mutex<TurnyState>>,
}

impl StateManager {
    /// Create a new state manager
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TurnyState::new())),
        }
    }

    /// Execute a closure with read access to the state
    pub fn with_state<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&TurnyState) -> T,
    {
        let state = self.state.lock().map_err(|e| {
            anyhow::anyhow!("Failed to acquire state lock for reading: {}", e)
        })?;
        Ok(f(&*state))
    }

    /// Execute a closure with write access to the state
    pub fn with_state_mut<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut TurnyState) -> T,
    {
        let mut state = self.state.lock().map_err(|e| {
            anyhow::anyhow!("Failed to acquire state lock for writing: {}", e)
        })?;
        Ok(f(&mut *state))
    }

    /// Reset the state to default values
    pub fn reset_state(&self) {
        if let Err(e) = self.with_state_mut(|state| state.reset()) {
            error!("StateManager lock failed in reset_state: {}", e);
        }
    }

    /// Set the current card and context
    pub fn set_current_card(&self, card_id: String, context_uri: String) {
        if let Err(e) = self.with_state_mut(|state| state.set_current_card(card_id, context_uri)) {
            error!("StateManager lock failed in set_current_card: {}", e);
        }
    }

    /// Set the playback state
    pub fn set_playing(&self, is_playing: bool) {
        if let Err(e) = self.with_state_mut(|state| state.set_playing(is_playing)) {
            error!("StateManager lock failed in set_playing: {}", e);
        }
    }

    /// Increment the absence count
    pub fn increment_absence_count(&self) {
        if let Err(e) = self.with_state_mut(|state| state.increment_absence_count()) {
            error!("StateManager lock failed in increment_absence_count: {}", e);
        }
    }

    /// Reset the absence count
    pub fn reset_absence_count(&self) {
        if let Err(e) = self.with_state_mut(|state| state.reset_absence_count()) {
            error!("StateManager lock failed in reset_absence_count: {}", e);
        }
    }

    /// Check if should auto-pause based on absence count
    pub fn should_auto_pause(&self, threshold: u32) -> bool {
        match self.with_state(|state| state.should_auto_pause(threshold)) {
            Ok(v) => v,
            Err(e) => {
                error!("StateManager lock failed in should_auto_pause: {}", e);
                false
            }
        }
    }

    /// Get a summary of the current state
    pub fn get_summary(&self) -> String {
        self.with_state(|state| state.summary())
            .unwrap_or_else(|e| {
                error!("StateManager lock failed in get_summary: {}", e);
                "State unavailable".to_string()
            })
    }
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turny_state_creation() {
        let state = TurnyState::new();
        assert!(state.current_id.is_none());
        assert!(state.context_uri.is_none());
        assert!(!state.is_playing);
        assert_eq!(state.absence_count, 0);
    }

    #[test]
    fn test_state_card_management() {
        let mut state = TurnyState::new();

        state.set_current_card("card123".to_string(), "spotify:playlist:test".to_string());
        assert_eq!(state.current_id, Some("card123".to_string()));
        assert_eq!(state.context_uri, Some("spotify:playlist:test".to_string()));
        assert_eq!(state.absence_count, 0);
    }

    #[test]
    fn test_state_playback_management() {
        let mut state = TurnyState::new();

        assert!(!state.is_playing);
        state.set_playing(true);
        assert!(state.is_playing);
        state.set_playing(false);
        assert!(!state.is_playing);
    }

    #[test]
    fn test_state_absence_count() {
        let mut state = TurnyState::new();

        assert_eq!(state.absence_count, 0);
        assert!(!state.should_auto_pause(3));

        state.increment_absence_count();
        assert_eq!(state.absence_count, 1);

        state.increment_absence_count();
        state.increment_absence_count();
        assert_eq!(state.absence_count, 3);
        assert!(state.should_auto_pause(3));

        state.reset_absence_count();
        assert_eq!(state.absence_count, 0);
    }

    #[test]
    fn test_state_reset() {
        let mut state = TurnyState::new();

        state.set_current_card("card123".to_string(), "playlist123".to_string());
        state.set_playing(true);
        state.increment_absence_count();

        state.reset();

        assert!(state.current_id.is_none());
        assert!(state.context_uri.is_none());
        assert!(!state.is_playing);
        assert_eq!(state.absence_count, 0);
    }

    #[test]
    fn test_state_summary() {
        let mut state = TurnyState::new();
        state.set_current_card("card123".to_string(), "playlist123".to_string());
        state.set_playing(true);
        state.increment_absence_count();

        let summary = state.summary();
        assert!(summary.contains("card=card123"));
        assert!(summary.contains("context=playlist123"));
        assert!(summary.contains("playing=true"));
        assert!(summary.contains("absences=1"));
    }

    #[test]
    fn test_state_manager_creation() {
        let manager = StateManager::new();
        let summary = manager.get_summary();
        assert!(summary.contains("card=none"));
        assert!(summary.contains("playing=false"));
    }

    #[test]
    fn test_state_manager_thread_safety() {
        let manager = StateManager::new();
        let manager = Arc::new(manager);
        let mut handles = vec![];

        for i in 0..4 {
            let m = Arc::clone(&manager);
            handles.push(std::thread::spawn(move || {
                for j in 0..100 {
                    m.set_current_card(format!("card_{}_{}", i, j), format!("uri_{}", j));
                    m.increment_absence_count();
                    m.reset_absence_count();
                    m.set_playing(i % 2 == 0);
                }
            }));
        }

        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        let summary = manager.get_summary();
        assert!(summary.contains("card="));
    }

    #[test]
    fn test_state_manager_operations() {
        let manager = StateManager::new();

        manager.set_current_card("card123".to_string(), "playlist123".to_string());
        let summary = manager.get_summary();
        assert!(summary.contains("card=card123"));

        manager.set_playing(true);
        let summary = manager.get_summary();
        assert!(summary.contains("playing=true"));

        manager.increment_absence_count();
        assert!(manager.should_auto_pause(1));

        manager.reset_absence_count();
        assert!(!manager.should_auto_pause(1));

        manager.reset_state();
        let summary = manager.get_summary();
        assert!(summary.contains("card=none"));
        assert!(summary.contains("playing=false"));
    }
}
