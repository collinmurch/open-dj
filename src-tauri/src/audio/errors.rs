use symphonia::core::errors::Error as SymphoniaError;
use thiserror::Error;
use cpal::{BuildStreamError, DefaultStreamConfigError, PlayStreamError, PauseStreamError, DevicesError, SupportedStreamConfigsError};

/// Errors that can occur during audio analysis (e.g., volume, waveform).
#[derive(Error, Debug)]
pub enum AudioAnalysisError {
    /// Invalid sample rate for volume calculation.
    #[error("Invalid sample rate for volume calculation: {0}")]
    InvalidSampleRate(f32),
    /// Cannot calculate RMS from empty samples.
    #[error("Cannot calculate RMS from empty samples")]
    EmptySamples,
}

/// Errors that can occur during BPM analysis and detection.
#[derive(Error, Debug)]
pub enum BpmError {
    /// Cannot estimate BPM from empty spectral flux.
    #[error("Cannot estimate BPM from empty spectral flux")]
    EmptySpectralFlux,
    /// Invalid lag range calculated for autocorrelation.
    #[error(
        "Invalid lag range calculated (min: {min_lag}, max: {max_lag}). Sample rate: {sample_rate}, hop size: {hop_size}"
    )]
    InvalidLagRange {
        min_lag: usize,
        max_lag: usize,
        sample_rate: f32,
        hop_size: usize,
    },
    /// Effective max lag not greater than min lag after flux length check.
    #[error(
        "Effective max lag ({eff_max_lag}) not greater than min lag ({min_lag}) after flux length check"
    )]
    EffectiveLagTooSmall { eff_max_lag: usize, min_lag: usize },
    /// Autocorrelation result length not greater than min lag.
    #[error("Autocorrelation result length ({ac_len}) not greater than min lag ({min_lag})")]
    AutocorrelationTooShort { ac_len: usize, min_lag: usize },
    /// Calculated period is too small, cannot determine BPM.
    #[error("Calculated period is too small, cannot determine BPM")]
    PeriodTooSmall,
    /// Could not find a significant peak in autocorrelation for BPM estimation.
    #[error("Could not find a significant peak in autocorrelation for BPM estimation")]
    NoAutocorrelationPeak,
    /// Cannot calculate BPM from empty samples.
    #[error("Cannot calculate BPM from empty samples")]
    EmptySamplesForBpm,
    /// Samples became empty after downsampling.
    #[error(
        "Samples became empty after downsampling (factor {factor}). Original count: {original_count}"
    )]
    EmptyAfterDownsample {
        factor: usize,
        original_count: usize,
    },
    /// Spectral flux calculation resulted in empty vector.
    #[error("Spectral flux calculation resulted in empty vector. Insufficient samples?")]
    EmptyFluxVector,
    /// FFT Autocorrelation failed.
    #[error("FFT Autocorrelation failed: {0}")]
    AutocorrelationFailure(String),
}

/// Errors that can occur during audio effects processing (EQ, filter, etc).
#[derive(Error, Debug)]
pub enum AudioEffectsError {
    /// Failed to calculate filter coefficients.
    #[error("Failed to calculate {filter_type} coefficients")]
    CoefficientCalculationError { filter_type: String },
    /// Failed to lock EQ params.
    #[error("Failed to lock EQ params: {reason}")]
    EqParamsLockError { reason: String },
}

/// Errors that can occur during audio decoding (file I/O, format, etc).
#[derive(Error, Debug)]
pub enum AudioDecodingError {
    /// Failed to open file for reading.
    #[error("Failed to open file '{path}': {source}")]
    FileOpenError {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// Symphonia probe/format error.
    #[error("Symphonia probe/format error for '{path}': {source}")]
    FormatError {
        path: String,
        #[source]
        source: SymphoniaError,
    },
    /// No suitable audio track found in file.
    #[error("No suitable audio track in '{path}'")]
    NoSuitableTrack { path: String },
    /// Sample rate missing in file.
    #[error("Sample rate missing in '{path}'")]
    MissingSampleRate { path: String },
    /// Channel info missing in file.
    #[error("Channel info missing in '{path}'")]
    MissingChannelInfo { path: String },
    /// Failed to create decoder for file.
    #[error("Failed to create decoder for '{path}': {source}")]
    DecoderCreationError {
        path: String,
        #[source]
        source: SymphoniaError,
    },
    /// Symphonia fatal decode error.
    #[error("Symphonia fatal decode error in '{path}': {source}")]
    FatalDecodeError {
        path: String,
        #[source]
        source: SymphoniaError,
    },
    /// Symphonia I/O error reading packet.
    #[error("Symphonia I/O error reading packet for '{path}': {source}")]
    PacketReadIoError {
        path: String,
        #[source]
        source: SymphoniaError,
    },
    /// No samples decoded from file.
    #[error("No samples decoded from '{path}'")]
    NoSamplesDecoded { path: String },
}

/// Errors that can occur during playback (streaming, state, etc).
#[derive(Error, Debug)]
pub enum PlaybackError {
    /// Failed to initialize audio output stream.
    #[error("Failed to initialize audio output stream: {0}")]
    OutputStreamInitError(String),
    /// Deck not found in local state.
    #[error("Deck '{deck_id}' not found in local state")]
    DeckNotFound { deck_id: String },
    /// CPAL: Failed to get supported output stream configs.
    #[error("CPAL: Failed to get supported output stream configs: {0}")]
    CpalSupportedStreamConfigsError(#[from] SupportedStreamConfigsError),
    /// CPAL: Failed to get default output device.
    #[error("CPAL: Failed to get default output device: {0}")]
    CpalNoDefaultOutputDevice(String),
    /// CPAL: Failed to get supported output stream config.
    #[error("CPAL: Failed to get supported output stream config: {0}")]
    CpalDefaultStreamConfigError(#[from] DefaultStreamConfigError),
    /// CPAL: No supported output config found matching sample rate.
    #[error("CPAL: No supported output config found matching sample rate {sample_rate}")]
    CpalNoMatchingConfig { sample_rate: u32 },
    /// CPAL: Failed to build output stream.
    #[error("CPAL: Failed to build output stream: {0}")]
    CpalBuildStreamError(#[from] BuildStreamError),
    /// CPAL: Failed to play stream.
    #[error("CPAL: Failed to play stream: {0}")]
    CpalPlayStreamError(#[from] PlayStreamError),
    /// CPAL: Failed to pause stream.
    #[error("CPAL: Failed to pause stream: {0}")]
    CpalPauseStreamError(#[from] PauseStreamError),
    /// CPAL: Devices enumeration error.
    #[error("CPAL: Devices enumeration error: {0}")]
    CpalDevicesError(#[from] DevicesError),
    /// Cannot perform operation on deck: No track loaded or invalid state.
    #[error("Cannot perform operation on deck '{deck_id}': No track loaded or invalid state.")]
    TrackNotLoadedOrInvalidState { deck_id: String },
    /// Audio decoding for playback failed for deck.
    #[error("Audio decoding for playback failed for deck '{deck_id}': {source}")]
    PlaybackDecodeError {
        deck_id: String,
        source: AudioDecodingError,
    },
    /// Audio decoding task panicked for deck.
    #[error("Audio decoding task panicked for deck '{deck_id}': {reason}")]
    DecodeTaskPanic { deck_id: String, reason: String },
    /// Audio command send error.
    #[error("Audio command send error: {0}")]
    CommandSendError(String),
    /// Failed to lock logical playback states.
    #[error("Failed to lock logical playback states: {0}")]
    LogicalStateLockError(String),
    /// Logical state not found for deck.
    #[error("Logical state not found for deck '{deck_id}'")]
    LogicalStateNotFound { deck_id: String },
    /// Failed to send shutdown completion signal.
    #[error("Failed to send shutdown completion signal: {0}")]
    ShutdownSignalError(String),
    /// Tokio MPSC send error for audio command.
    #[error("Tokio MPSC send error for audio command: {0}")]
    MpscSendError(
        #[from] tokio::sync::mpsc::error::SendError<crate::audio::playback::commands::AudioThreadCommand>,
    ),
    /// Tokio JoinError from spawned task.
    #[error("Tokio JoinError from spawned task: {0}")]
    JoinError(#[from] tokio::task::JoinError),
}

/// Errors that can occur during audio processing (analysis, BPM, volume, etc).
#[derive(Error, Debug)]
pub enum AudioProcessorError {
    /// Decoding error during analysis.
    #[error("Decoding error during analysis for '{path}': {source}")]
    AnalysisDecodingError {
        path: String,
        source: AudioDecodingError,
    },
    /// BPM calculation failed during analysis.
    #[error("BPM calculation failed for '{path}': {source}")]
    AnalysisBpmError { path: String, source: BpmError },
    /// Volume analysis failed during analysis.
    #[error("Volume analysis failed for '{path}': {source}")]
    AnalysisVolumeError {
        path: String,
        source: AudioAnalysisError,
    },
    /// Invalid data for duration calculation.
    #[error(
        "Invalid data (empty samples or zero sample rate) for duration calculation for '{path}'."
    )]
    InvalidDataForDurationCalculation { path: String },
}

// --- Error to String conversions for Tauri command results ---
/// Converts AudioDecodingError to a string for Tauri command results.
impl From<AudioDecodingError> for String {
    fn from(err: AudioDecodingError) -> String {
        err.to_string()
    }
}
/// Converts BpmError to a string for Tauri command results.
impl From<BpmError> for String {
    fn from(err: BpmError) -> String {
        err.to_string()
    }
}
/// Converts AudioAnalysisError to a string for Tauri command results.
impl From<AudioAnalysisError> for String {
    fn from(err: AudioAnalysisError) -> String {
        err.to_string()
    }
}
/// Converts PlaybackError to a string for Tauri command results.
impl From<PlaybackError> for String {
    fn from(err: PlaybackError) -> String {
        err.to_string()
    }
}
/// Converts AudioProcessorError to a string for Tauri command results.
impl From<AudioProcessorError> for String {
    fn from(err: AudioProcessorError) -> String {
        err.to_string()
    }
}
