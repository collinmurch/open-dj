// --- EQ Filter Constants ---
pub const LOW_MID_CROSSOVER_HZ: f32 = 250.0;
pub const MID_HIGH_CROSSOVER_HZ: f32 = 3000.0;
pub const MID_CENTER_HZ: f32 = 1000.0;
// Q factor for peaking filter (Butterworth)
pub const MID_PEAK_Q_FACTOR: f32 = std::f32::consts::FRAC_1_SQRT_2;
// Q factor for shelf filters
pub const SHELF_Q_FACTOR: f32 = 0.5;

// --- BPM Analyzer Constants ---
// Typical BPM range for music
pub const BPM_MIN: f32 = 60.0;
pub const BPM_MAX: f32 = 200.0;

// --- Audio Analysis Constants ---
// Target number of RMS volume intervals to calculate per second of audio

// --- Audio Playback Thread Constants ---
// Interval for time updates from the audio thread to the frontend, in milliseconds.
pub const AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS: u64 = 50; 

