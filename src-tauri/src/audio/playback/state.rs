use cpal::Stream;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

// Helper for atomic f32/f64 operations
pub(crate) struct AtomicF32 {
    value: AtomicU32,
}

impl AtomicF32 {
    pub fn new(value: f32) -> Self {
        Self {
            value: AtomicU32::new(value.to_bits()),
        }
    }
    
    pub fn load(&self, ordering: Ordering) -> f32 {
        f32::from_bits(self.value.load(ordering))
    }
    
    pub fn store(&self, value: f32, ordering: Ordering) {
        self.value.store(value.to_bits(), ordering)
    }
}

pub(crate) struct AtomicF64 {
    value: AtomicU64,
}

impl AtomicF64 {
    pub fn new(value: f64) -> Self {
        Self {
            value: AtomicU64::new(value.to_bits()),
        }
    }
    
    pub fn load(&self, ordering: Ordering) -> f64 {
        f64::from_bits(self.value.load(ordering))
    }
    
    pub fn store(&self, value: f64, ordering: Ordering) {
        self.value.store(value.to_bits(), ordering)
    }
}

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
    pub(crate) current_sample_read_head: Arc<AtomicF64>,
    /// Paused position of the read head, if paused.
    pub(crate) paused_position_read_head: Arc<AtomicF64>,
    /// Duration of the loaded track.
    pub(crate) duration: Duration,
    /// Whether the deck is currently playing.
    pub(crate) is_playing: Arc<AtomicBool>,
    /// Current EQ parameters (smoothed).
    pub(crate) current_eq_params: Arc<Mutex<EqParams>>,
    /// Target EQ parameters (for smoothing).
    pub(crate) target_eq_params: Arc<Mutex<EqParams>>,
    /// Current trim gain (smoothed).
    pub(crate) current_trim_gain: Arc<AtomicF32>,
    /// Target trim gain (for smoothing).
    pub(crate) target_trim_gain: Arc<AtomicF32>,
    /// Optional cue point for the deck.
    pub(crate) cue_point: Option<Duration>,
    /// Current pitch rate (smoothed).
    pub(crate) current_pitch_rate: Arc<AtomicF32>,
    /// Target pitch rate (for smoothing).
    pub(crate) target_pitch_rate: Arc<AtomicF32>,
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
    /// Cached EQ coefficients to avoid recalculation
    pub(crate) cached_low_coeffs: Arc<Mutex<Option<biquad::Coefficients<f32>>>>,
    pub(crate) cached_mid_coeffs: Arc<Mutex<Option<biquad::Coefficients<f32>>>>,
    pub(crate) cached_high_coeffs: Arc<Mutex<Option<biquad::Coefficients<f32>>>>,
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
    /// Channel fader level (0.0 to 1.0), controlled by individual deck faders.
    pub(crate) channel_fader_level: Arc<AtomicF32>,
    // --- Precise Timing Fields (Phase 5) ---
    /// Output sample rate of the audio device (set on stream creation).
    pub(crate) output_sample_rate: Option<u32>,
    /// Last playback instant (for precise timing).
    pub(crate) last_playback_instant: Arc<Mutex<Option<std::time::Instant>>>,
    /// Read head at last playback instant (for precise timing).
    pub(crate) read_head_at_last_playback_instant: Arc<Mutex<Option<f64>>>,
    // --- Seek Fading (Phase 6) ---
    /// State for seek fade in/out. Value is progress from 0.0 (start, muted) to 1.0 (fully faded in).
    pub(crate) seek_fade_state: Arc<Mutex<Option<f32>>>,
    /// Last time a pitch event was emitted (for rate limiting)  
    pub(crate) last_pitch_event_time: Arc<Mutex<Option<std::time::Instant>>>,
    /// Last frame number when a timing event was emitted (for per-deck timing control)
    pub(crate) last_emit_frame: Arc<AtomicU64>,
} 