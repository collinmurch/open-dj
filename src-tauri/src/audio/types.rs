use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

// --- Track Metadata ---
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackBasicMetadata {
    pub duration_seconds: Option<f64>,
    pub bpm: Option<f32>,
    pub first_beat_sec: Option<f32>,
}

// --- EQ Parameters ---
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

impl EqParams {
    pub(crate) fn approx_eq(&self, other: &Self) -> bool {
        const EPSILON: f32 = 1e-5;
        (self.low_gain_db - other.low_gain_db).abs() < EPSILON
            && (self.mid_gain_db - other.mid_gain_db).abs() < EPSILON
            && (self.high_gain_db - other.high_gain_db).abs() < EPSILON
    }
}

// --- Audio Analysis Types ---
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioAnalysis {
    pub levels: Vec<Vec<WaveBin>>,
    pub max_band_energy: f32,
}

#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct WaveBin {
    pub low: f32,
    pub mid: f32,
    pub high: f32,
}

// --- Audio Thread Commands ---
#[derive(Debug)]
pub enum AudioThreadCommand {
    InitDeck(String),
    LoadTrack {
        deck_id: String,
        path: String,
        original_bpm: Option<f32>,
        first_beat_sec: Option<f32>,
    },
    Play(String),
    Pause(String),
    Seek {
        deck_id: String,
        position_seconds: f64,
    },
    SetFaderLevel {
        deck_id: String,
        level: f32,
    },
    SetTrimGain {
        deck_id: String,
        gain: f32,
    },
    SetEq {
        deck_id: String,
        params: EqParams,
    },
    SetCue {
        deck_id: String,
        position_seconds: f64,
    },
    SetPitchRate {
        deck_id: String,
        rate: f32,
        is_manual_adjustment: bool,
    },
    EnableSync {
        slave_deck_id: String,
        master_deck_id: String,
    },
    DisableSync {
        deck_id: String,
    },
    CleanupDeck(String),
    Shutdown(oneshot::Sender<()>),
}

// --- Event Payloads for Frontend ---
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

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPitchTickEventPayload {
    pub deck_id: String,
    pub pitch_rate: f32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatusEventPayload {
    pub deck_id: String,
    pub is_playing: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSyncStatusEventPayload {
    pub deck_id: String,
    #[serde(rename = "isSyncActive")]
    pub is_sync_active: bool,
    #[serde(rename = "isMaster")]
    pub is_master: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackLoadEventPayload {
    pub deck_id: String,
    pub duration: f64,
    pub cue_point_seconds: Option<f64>,
    pub original_bpm: Option<f32>,
    pub first_beat_sec: Option<f32>,
}
