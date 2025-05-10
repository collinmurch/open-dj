use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

// --- EQ Parameters ---

/// Holds the gain values (in dB) for the 3-band EQ.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EqParams {
    pub low_gain_db: f32,
    pub mid_gain_db: f32,
    pub high_gain_db: f32,
}

impl Default for EqParams {
    fn default() -> Self {
        EqParams {
            low_gain_db: 0.0,
            mid_gain_db: 0.0,
            high_gain_db: 0.0,
        }
    }
}

// Add approximate comparison for f32
impl EqParams {
    pub(crate) fn approx_eq(&self, other: &Self) -> bool {
        const EPSILON: f32 = 1e-5; // Tolerance for float comparison
        (self.low_gain_db - other.low_gain_db).abs() < EPSILON
            && (self.mid_gain_db - other.mid_gain_db).abs() < EPSILON
            && (self.high_gain_db - other.high_gain_db).abs() < EPSILON
    }
}

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
    SetFaderLevel {
        deck_id: String,
        level: f32, // Linear level 0.0 to 1.0
    },
    SetTrimGain {
        deck_id: String,
        gain: f32, // Linear gain, e.g., 0.0 to 4.0 (+12dB)
    },
    SetEq {
        deck_id: String,
        params: EqParams,
    },
    SetCue {
        deck_id: String,
        position_seconds: f64,
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
    pub duration: Option<f64>,
    pub error: Option<String>,
    pub cue_point_seconds: Option<f64>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        PlaybackState {
            is_playing: false,
            is_loading: false,
            current_time: 0.0,
            duration: None,
            error: None,
            cue_point_seconds: None,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackUpdateEventPayload {
    pub deck_id: String,
    pub state: PlaybackState,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTickEventPayload {
    pub deck_id: String,
    pub current_time: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackErrorEventPayload {
    pub deck_id: String,
    pub error: String,
}
