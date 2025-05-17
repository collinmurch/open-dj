use cpal::Stream;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::audio::types::EqParams; // EqParams is in audio::types
use super::commands::AudioThreadCommand; // AudioThreadCommand will be in playback/commands.rs
use biquad::DirectForm1; // Import DirectForm1

// --- State Management ---

/// Application state for the audio thread, holding the command sender for communication.
pub struct AppState {
    /// Sender for audio thread commands.
    audio_command_sender: mpsc::Sender<AudioThreadCommand>,
}

impl AppState {
    /// Create a new AppState with the given command sender.
    pub fn new(sender: mpsc::Sender<AudioThreadCommand>) -> Self {
        AppState {
            audio_command_sender: sender,
        }
    }

    /// Get a clone of the command sender for sending audio commands.
    pub fn get_command_sender(&self) -> mpsc::Sender<AudioThreadCommand> {
        self.audio_command_sender.clone()
    }
}

/// State for a single deck in the audio thread, including playback, EQ, sync, and timing fields.
pub(crate) struct AudioThreadDeckState {
    /// The CPAL audio stream for this deck, if active.
    pub(crate) cpal_stream: Option<Stream>,
    /// Decoded audio samples (mono, f32).
    pub(crate) decoded_samples: Arc<Vec<f32>>,
    /// Source sample rate of the decoded audio.
    pub(crate) sample_rate: f32,
    /// Current read head position (sample index, floating point for interpolation).
    pub(crate) current_sample_read_head: Arc<Mutex<f64>>,
    /// Paused position of the read head, if paused.
    pub(crate) paused_position_read_head: Arc<Mutex<Option<f64>>>,
    /// Duration of the loaded track.
    pub(crate) duration: Duration,
    /// Whether the deck is currently playing.
    pub(crate) is_playing: Arc<Mutex<bool>>,
    /// Current EQ parameters (smoothed).
    pub(crate) current_eq_params: Arc<Mutex<EqParams>>,
    /// Target EQ parameters (for smoothing).
    pub(crate) target_eq_params: Arc<Mutex<EqParams>>,
    /// Current trim gain (smoothed).
    pub(crate) current_trim_gain: Arc<Mutex<f32>>,
    /// Target trim gain (for smoothing).
    pub(crate) target_trim_gain: Arc<Mutex<f32>>,
    /// Optional cue point for the deck.
    pub(crate) cue_point: Option<Duration>,
    /// Current pitch rate (smoothed).
    pub(crate) current_pitch_rate: Arc<Mutex<f32>>,
    /// Target pitch rate (for smoothing).
    pub(crate) target_pitch_rate: Arc<Mutex<f32>>,
    /// Last pitch rate sent to the UI.
    pub(crate) last_ui_pitch_rate: Option<f32>,
    // --- EQ Filter Instances (Phase 3) ---
    /// Low shelf filter instance for EQ.
    pub(crate) low_shelf_filter: Arc<Mutex<DirectForm1<f32>>>,
    /// Mid peak filter instance for EQ.
    pub(crate) mid_peak_filter: Arc<Mutex<DirectForm1<f32>>>,
    /// High shelf filter instance for EQ.
    pub(crate) high_shelf_filter: Arc<Mutex<DirectForm1<f32>>>,
    /// Last EQ parameters used for filter coefficient calculation.
    pub(crate) last_eq_params: Arc<Mutex<EqParams>>,
    // --- Sync Feature Fields ---
    /// Original BPM of the loaded track, if known.
    pub(crate) original_bpm: Option<f32>,
    /// First beat offset in seconds, if known.
    pub(crate) first_beat_sec: Option<f32>,
    /// Whether sync is active for this deck.
    pub(crate) is_sync_active: bool,
    /// Whether this deck is the sync master.
    pub(crate) is_master: bool,
    /// The deck ID of the master deck, if this deck is a slave.
    pub(crate) master_deck_id: Option<String>,
    /// Target pitch rate for BPM match (sync).
    pub(crate) target_pitch_rate_for_bpm_match: f32,
    /// Manual pitch rate set by the user (for restoring after sync).
    pub(crate) manual_pitch_rate: f32,
    /// Integral error for PLL sync.
    pub pll_integral_error: f32,
    // --- Precise Timing Fields (Phase 5) ---
    /// Output sample rate of the audio device (set on stream creation).
    pub(crate) output_sample_rate: Option<u32>,
    /// Last playback instant (for precise timing).
    pub(crate) last_playback_instant: Arc<Mutex<Option<std::time::Instant>>>,
    /// Read head at last playback instant (for precise timing).
    pub(crate) read_head_at_last_playback_instant: Arc<Mutex<Option<f64>>>,
    // --- Seek Fading (Phase 6) ---
    /// State for seek fade in/out.
    pub(crate) seek_fade_state: Arc<Mutex<Option<SeekFadeProgress>>>,
}

/// Progress state for seek fade in/out.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SeekFadeProgress {
    /// Fading out: progress from 0.0 (start) to 1.0 (fully faded out).
    FadingOut { progress: f32 },
    /// Fading in: progress from 0.0 (start, muted) to 1.0 (fully faded in).
    FadingIn { progress: f32 },
} 