use serde::{Deserialize, Serialize};

// --- Track Metadata ---
/// Basic metadata for an audio track, including duration, BPM, and first beat offset.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackBasicMetadata {
    /// Duration of the track in seconds, if known.
    pub duration_seconds: Option<f64>,
    /// Estimated BPM of the track, if analyzed.
    pub bpm: Option<f32>,
    /// Time (in seconds) of the first beat, if detected.
    pub first_beat_sec: Option<f32>,
}

// --- EQ Parameters ---
/// Parameters for 3-band EQ (low, mid, high) in decibels.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EqParams {
    /// Gain for the low band in dB.
    pub low_gain_db: f32,
    /// Gain for the mid band in dB.
    pub mid_gain_db: f32,
    /// Gain for the high band in dB.
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
    /// Returns true if all bands are approximately equal to another set of EQ params.
    pub(crate) fn approx_eq(&self, other: &Self) -> bool {
        const EPSILON: f32 = 1e-5;
        (self.low_gain_db - other.low_gain_db).abs() < EPSILON
            && (self.mid_gain_db - other.mid_gain_db).abs() < EPSILON
            && (self.high_gain_db - other.high_gain_db).abs() < EPSILON
    }
}

// --- Audio Analysis Types ---
/// Audio analysis results for a track, including waveform levels and max energy.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioAnalysis {
    /// Waveform levels for each band and interval.
    pub levels: Vec<Vec<WaveBin>>,
    /// Maximum energy found in any band.
    pub max_band_energy: f32,
}

/// A single bin of waveform energy for low, mid, and high bands.
#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct WaveBin {
    /// Energy in the low band.
    pub low: f32,
    /// Energy in the mid band.
    pub mid: f32,
    /// Energy in the high band.
    pub high: f32,
}

// --- Audio Thread Commands ---

// --- Event Payloads for Frontend ---
