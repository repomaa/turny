use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WebEvent {
    RfidDetected {
        card_id: String,
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
}

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    Play,
    Pause,
    Next,
    Previous,
}
