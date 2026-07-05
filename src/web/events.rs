use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ExistingMapping {
    pub playlist_uri: String,
    pub playlist_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WebEvent {
    RfidDetected {
        card_id: String,
        existing_mapping: Option<ExistingMapping>,
    },
    PlaybackStarted {
        card_id: String,
        playlist_uri: String,
    },
    PlaybackPaused,
    PlaybackResumed,
    StateChanged {
        is_playing: bool,
        current_card: Option<String>,
        context_uri: Option<String>,
    },
    VolumeChanged {
        volume: u8,
    },
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    Play,
    Pause,
    Next,
    Previous,
    SetVolume(u8),
}
