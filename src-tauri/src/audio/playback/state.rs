use cpal::Stream;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::audio::types::EqParams; // EqParams is in audio::types
use super::commands::AudioThreadCommand; // AudioThreadCommand will be in playback/commands.rs
use biquad::DirectForm1; // Import DirectForm1

// --- State Management ---

pub struct AppState {
    audio_command_sender: mpsc::Sender<AudioThreadCommand>,
}

impl AppState {
    pub fn new(sender: mpsc::Sender<AudioThreadCommand>) -> Self {
        AppState {
            audio_command_sender: sender,
        }
    }

    // Getter to allow sending commands from Tauri command handlers
    // Might be needed if command handlers move to a different module than AppState instantiation
    pub fn get_command_sender(&self) -> mpsc::Sender<AudioThreadCommand> {
        self.audio_command_sender.clone()
    }
}

pub(crate) struct AudioThreadDeckState {
    pub(crate) cpal_stream: Option<Stream>,
    pub(crate) decoded_samples: Arc<Vec<f32>>,
    pub(crate) sample_rate: f32,
    pub(crate) current_sample_read_head: Arc<Mutex<f64>>,
    pub(crate) paused_position_read_head: Arc<Mutex<Option<f64>>>,
    pub(crate) duration: Duration,
    pub(crate) is_playing: Arc<Mutex<bool>>,
    pub(crate) current_eq_params: Arc<Mutex<EqParams>>,
    pub(crate) target_eq_params: Arc<Mutex<EqParams>>,
    pub(crate) current_trim_gain: Arc<Mutex<f32>>,
    pub(crate) target_trim_gain: Arc<Mutex<f32>>,
    pub(crate) cue_point: Option<Duration>,
    pub(crate) current_pitch_rate: Arc<Mutex<f32>>,
    pub(crate) target_pitch_rate: Arc<Mutex<f32>>,
    pub(crate) last_ui_pitch_rate: Option<f32>,
    // --- EQ Filter Instances (Phase 3) ---
    pub(crate) low_shelf_filter: Arc<Mutex<DirectForm1<f32>>>,
    pub(crate) mid_peak_filter: Arc<Mutex<DirectForm1<f32>>>,
    pub(crate) high_shelf_filter: Arc<Mutex<DirectForm1<f32>>>,
    pub(crate) last_eq_params: Arc<Mutex<EqParams>>,
    // --- Sync Feature Fields ---
    pub(crate) original_bpm: Option<f32>,
    pub(crate) first_beat_sec: Option<f32>,
    pub(crate) is_sync_active: bool,
    pub(crate) is_master: bool,
    pub(crate) master_deck_id: Option<String>,
    pub(crate) target_pitch_rate_for_bpm_match: f32,
    pub(crate) manual_pitch_rate: f32,
    pub pll_integral_error: f32,
    // --- Precise Timing Fields (Phase 5) ---
    pub(crate) output_sample_rate: Option<u32>, // Set once on stream creation
    pub(crate) last_playback_instant: Arc<Mutex<Option<std::time::Instant>>>,
    pub(crate) read_head_at_last_playback_instant: Arc<Mutex<Option<f64>>>,
    // --- Seek Fading (Phase 6) ---
    pub(crate) seek_fade_state: Arc<Mutex<Option<SeekFadeProgress>>>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SeekFadeProgress {
    FadingOut { progress: f32 }, // 0.0 (start) to 1.0 (fully faded out)
    FadingIn { progress: f32 },  // 0.0 (start, muted) to 1.0 (fully faded in)
} 