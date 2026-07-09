use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ExistingMapping {
    pub playlist_uri: String,
    pub playlist_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WebEvent {
    /// Emitted when an RFID card is placed on the reader
    RfidDetected {
        card_id: String,
        existing_mapping: Option<ExistingMapping>,
    },
    /// Emitted when playback begins for a card
    PlaybackStarted {
        card_id: String,
        playlist_uri: String,
    },
    /// Emitted when playback is paused
    PlaybackPaused,
    /// Emitted when playback is resumed
    PlaybackResumed,
    /// Emitted when the overall playback state changes
    StateChanged {
        is_playing: bool,
        current_card: Option<String>,
        context_uri: Option<String>,
    },
    /// Emitted when the volume is changed
    VolumeChanged {
        volume: u8,
    },
    /// Sent when the WebSocket receiver lagged behind and may have missed events.
    /// Frontend should refresh state from the REST API.
    #[allow(dead_code)]
    LagDetected,
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    /// Start playback
    Play,
    /// Pause playback
    Pause,
    /// Skip to next track
    Next,
    /// Skip to previous track
    Previous,
    /// Set the volume level (0–100)
    SetVolume(u8),
}
