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
pub const AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS: u64 = 75;
pub const AUDIO_BUFFER_CHAN_SIZE: usize = 32;

// --- Utility Constants --
pub const DEFAULT_MONO_SAMPLE_CAPACITY: usize = 1024 * 256;

// -- Initial Values --
pub const INITIAL_TRIM_GAIN: f32 = 1.0;
