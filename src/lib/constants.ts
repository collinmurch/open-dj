// Audio and UI constants for the Open DJ application

// Audio playback constants
export const AUDIO_CONSTANTS = {
    /** Seek amount in seconds for forward/backward buttons */
    SEEK_AMOUNT_SECONDS: 5,
    
    /** Tolerance for cue point detection in seconds */
    CUE_POINT_TOLERANCE_SECONDS: 0.1,
    
    /** Minimum change in fader level to trigger backend update */
    FADER_LEVEL_CHANGE_THRESHOLD: 0.01,
} as const;

// Pitch and synchronization constants
export const SYNC_CONSTANTS = {
    /** Step size for pitch slider */
    PITCH_SLIDER_STEP: 0.0001,
    
    /** Minimum pitch rate */
    PITCH_RATE_MIN: 0.75,
    
    /** Maximum pitch rate */
    PITCH_RATE_MAX: 1.25,
    
    /** Default pitch rate (normal speed) */
    PITCH_RATE_DEFAULT: 1.0,
} as const;

// EQ constants
export const EQ_CONSTANTS = {
    /** Minimum EQ gain in dB */
    EQ_GAIN_MIN_DB: -26,
    
    /** Maximum EQ gain in dB */
    EQ_GAIN_MAX_DB: 6,
    
    /** Default EQ gain (no boost/cut) */
    EQ_GAIN_DEFAULT_DB: 0,
    
    /** EQ adjustment step size in dB */
    EQ_STEP_DB: 1,
} as const;

// Trim constants
export const TRIM_CONSTANTS = {
    /** Minimum trim gain in dB */
    TRIM_GAIN_MIN_DB: -12,
    
    /** Maximum trim gain in dB */
    TRIM_GAIN_MAX_DB: 12,
    
    /** Default trim gain (no boost/cut) */
    TRIM_GAIN_DEFAULT_DB: 0,
    
    /** Trim adjustment step size in dB */
    TRIM_STEP_DB: 1,
} as const;

// Fader constants
export const FADER_CONSTANTS = {
    /** Minimum fader level */
    FADER_MIN: 0,
    
    /** Maximum fader level */
    FADER_MAX: 1,
    
    /** Fader adjustment step size */
    FADER_STEP: 0.01,
} as const;

// Crossfader constants
export const CROSSFADER_CONSTANTS = {
    /** Crossfader center position */
    CROSSFADER_CENTER: 0.5,
    
    /** Crossfader minimum position (full A) */
    CROSSFADER_MIN: 0,
    
    /** Crossfader maximum position (full B) */
    CROSSFADER_MAX: 1,
} as const;