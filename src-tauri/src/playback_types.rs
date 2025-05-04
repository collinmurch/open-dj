use serde::Serialize;
use tokio::sync::oneshot;

// --- Audio Thread Communication ---

#[derive(Debug)]
pub enum AudioThreadCommand {
    InitDeck(String), // deck_id
    LoadTrack {
        deck_id: String,
        path: String,
    },
    Play(String),  // deck_id
    Pause(String), // deck_id
    Seek {
        deck_id: String,
        position_seconds: f64,
    },
    SetVolume {
        deck_id: String,
        volume: f32,
    },
    CleanupDeck(String), // deck_id
    Shutdown(oneshot::Sender<()>),
}

// --- State Definitions ---

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    pub is_playing: bool,
    pub is_loading: bool,
    pub current_time: f64,
    pub duration: f64,
    pub error: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        PlaybackState {
            is_playing: false,
            is_loading: false,
            current_time: 0.0,
            duration: 0.0,
            error: None,
        }
    }
}

// Note: AudioThreadDeckState remains internal to audio_playback.rs for now,
// as it's not directly part of the public API or state management structure visible outside. 