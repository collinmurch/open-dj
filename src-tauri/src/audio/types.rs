use serde::{Deserialize, Serialize};

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

// --- Event Payloads for Frontend ---
