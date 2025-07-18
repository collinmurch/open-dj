// --- EQ Filter Constants ---
pub const LOW_MID_CROSSOVER_HZ: f32 = 250.0;
pub const MID_HIGH_CROSSOVER_HZ: f32 = 3000.0;
pub const MID_CENTER_HZ: f32 = 1000.0;
pub const MID_PEAK_Q_FACTOR: f32 = std::f32::consts::FRAC_1_SQRT_2;
pub const SHELF_Q_FACTOR: f32 = 0.5;

// --- BPM Analyzer Constants ---
pub const BPM_MIN: f32 = 60.0;
pub const BPM_MAX: f32 = 200.0;

// --- Audio Playback Thread Constants ---
// PERFORMANCE OPTIMIZATIONS:
// - Reduced time update interval from 25ms to 10ms for tighter sync
// - Increased PLL responsiveness for better phase tracking
// - Implemented cubic interpolation for better audio quality
// - Added sample rate mismatch correction for accurate playback speed
// - Pre-computed constants to reduce callback overhead
// - Improved macOS Core Audio compatibility
pub const AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS: u64 = 20; // 50 FPS for smooth UI while preventing sync oscillations  
pub const AUDIO_BUFFER_CHAN_SIZE: usize = 64; // Increased buffer size for better batching

// --- Utility Constants --
// Increased default capacity for better performance with longer tracks
pub const DEFAULT_MONO_SAMPLE_CAPACITY: usize = 1024 * 512;

// --- Audio Analysis Performance Constants ---
/// FFT frame size for BPM analysis - optimized for performance vs accuracy
pub const BPM_FRAME_SIZE: usize = 1024;
/// Hop size for BPM analysis - 25% overlap for good time resolution
pub const BPM_HOP_SIZE: usize = BPM_FRAME_SIZE / 4;
/// Downsampling factor for BPM analysis to reduce computational load
pub const BPM_DOWNSAMPLE_FACTOR: usize = 2;

/// FFT frame size for waveform analysis
pub const WAVEFORM_FRAME_SIZE: usize = 1024;
/// Hop size for waveform analysis - 50% overlap for smooth waveform
pub const WAVEFORM_HOP_SIZE: usize = WAVEFORM_FRAME_SIZE / 2;

// -- Initial Values --
pub const INITIAL_TRIM_GAIN: f32 = 1.0;

// -- EQ Performance Constants --
/// Minimum change in dB before recalculating EQ filter coefficients
/// This prevents expensive recalculation for tiny inaudible changes
pub const EQ_RECALC_THRESHOLD_DB: f32 = 0.1;

/// Smoothing factor for EQ parameter changes (higher = faster response)
pub const EQ_SMOOTHING_FACTOR: f32 = 0.08;

// -- Event Rate Limiting Constants --
/// Minimum interval between pitch events (to prevent UI flooding)  
pub const MIN_PITCH_EVENT_INTERVAL_MS: u64 = 16; // ~60 FPS max (smooth for UI)
