use cpal::Stream;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::audio::types::EqParams; // EqParams is in audio::types
use super::commands::AudioThreadCommand; // AudioThreadCommand will be in playback/commands.rs

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
    pub(crate) current_sample_index: Arc<Mutex<usize>>,
    pub(crate) paused_position_samples: Arc<Mutex<Option<usize>>>,
    pub(crate) duration: Duration,
    pub(crate) is_playing: Arc<Mutex<bool>>,
    pub(crate) eq_params: Arc<Mutex<EqParams>>,
    pub(crate) trim_gain: Arc<Mutex<f32>>,
    pub(crate) cue_point: Option<Duration>,
    pub(crate) current_pitch_rate: Arc<Mutex<f32>>,
    pub(crate) last_ui_pitch_rate: Option<f32>,
    // --- Sync Feature Fields ---
    pub(crate) original_bpm: Option<f32>,
    pub(crate) first_beat_sec: Option<f32>,
    pub(crate) is_sync_active: bool,
    pub(crate) is_master: bool,
    pub(crate) master_deck_id: Option<String>,
    pub(crate) target_pitch_rate_for_bpm_match: f32,
    pub(crate) manual_pitch_rate: f32,
    pub pll_integral_error: f32,
} 