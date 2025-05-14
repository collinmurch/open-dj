use rodio::PlayError;
use symphonia::core::errors::Error as SymphoniaError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AudioAnalysisError {
    #[error("Invalid sample rate for volume calculation: {0}")]
    InvalidSampleRate(f32),
    #[error("Cannot calculate RMS from empty samples")]
    EmptySamples,
}

#[derive(Error, Debug)]
pub enum BpmError {
    #[error("Cannot estimate BPM from empty spectral flux")]
    EmptySpectralFlux,
    #[error(
        "Invalid lag range calculated (min: {min_lag}, max: {max_lag}). Sample rate: {sample_rate}, hop size: {hop_size}"
    )]
    InvalidLagRange {
        min_lag: usize,
        max_lag: usize,
        sample_rate: f32,
        hop_size: usize,
    },
    #[error(
        "Effective max lag ({eff_max_lag}) not greater than min lag ({min_lag}) after flux length check"
    )]
    EffectiveLagTooSmall { eff_max_lag: usize, min_lag: usize },
    #[error("Autocorrelation result length ({ac_len}) not greater than min lag ({min_lag})")]
    AutocorrelationTooShort { ac_len: usize, min_lag: usize },
    #[error("Calculated period is too small, cannot determine BPM")]
    PeriodTooSmall,
    #[error("Could not find a significant peak in autocorrelation for BPM estimation")]
    NoAutocorrelationPeak,
    #[error("Cannot calculate BPM from empty samples")]
    EmptySamplesForBpm,
    #[error(
        "Samples became empty after downsampling (factor {factor}). Original count: {original_count}"
    )]
    EmptyAfterDownsample {
        factor: usize,
        original_count: usize,
    },
    #[error("Spectral flux calculation resulted in empty vector. Insufficient samples?")]
    EmptyFluxVector,
    #[error("FFT Autocorrelation failed: {0}")]
    AutocorrelationFailure(String),
}

#[derive(Error, Debug)]
pub enum AudioEffectsError {
    #[error("Failed to calculate {filter_type} coefficients")]
    CoefficientCalculationError { filter_type: String },
    #[error("Failed to lock EQ params: {reason}")]
    EqParamsLockError { reason: String },
}

#[derive(Error, Debug)]
pub enum AudioDecodingError {
    #[error("Failed to open file '{path}': {source}")]
    FileOpenError {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Symphonia probe/format error for '{path}': {source}")]
    FormatError {
        path: String,
        #[source]
        source: SymphoniaError,
    },
    #[error("No suitable audio track in '{path}'")]
    NoSuitableTrack { path: String },
    #[error("Sample rate missing in '{path}'")]
    MissingSampleRate { path: String },
    #[error("Channel info missing in '{path}'")]
    MissingChannelInfo { path: String },
    #[error("Failed to create decoder for '{path}': {source}")]
    DecoderCreationError {
        path: String,
        #[source]
        source: SymphoniaError,
    },
    #[error("Symphonia fatal decode error in '{path}': {source}")]
    FatalDecodeError {
        path: String,
        #[source]
        source: SymphoniaError,
    },
    #[error("Symphonia I/O error reading packet for '{path}': {source}")]
    PacketReadIoError {
        path: String,
        #[source]
        source: SymphoniaError,
    },
    #[error("No samples decoded from '{path}'")]
    NoSamplesDecoded { path: String },
}

#[derive(Error, Debug)]
pub enum PlaybackError {
    #[error("Failed to initialize audio output stream: {0}")]
    OutputStreamInitError(String),
    #[error("Deck '{deck_id}' not found in local state")]
    DeckNotFound { deck_id: String },
    #[error("Failed to create audio sink for deck '{deck_id}': {source:?}")]
    SinkCreationError {
        deck_id: String,
        #[source]
        source: PlayError,
    },
    #[error("Cannot perform operation on deck '{deck_id}': No track loaded or invalid state.")]
    TrackNotLoadedOrInvalidState { deck_id: String },
    #[error("Audio decoding for playback failed for deck '{deck_id}': {source}")]
    PlaybackDecodeError {
        deck_id: String,
        source: AudioDecodingError,
    },
    #[error("Audio decoding task panicked for deck '{deck_id}': {reason}")]
    DecodeTaskPanic { deck_id: String, reason: String },
    #[error("Audio command send error: {0}")]
    CommandSendError(String),
    #[error("Failed to lock logical playback states: {0}")]
    LogicalStateLockError(String),
    #[error("Logical state not found for deck '{deck_id}'")]
    LogicalStateNotFound { deck_id: String },
    #[error("Failed to send shutdown completion signal: {0}")]
    ShutdownSignalError(String),
    #[error("Tokio MPSC send error for audio command: {0}")]
    MpscSendError(
        #[from] tokio::sync::mpsc::error::SendError<crate::audio::playback::commands::AudioThreadCommand>,
    ),
    #[error("Tokio JoinError from spawned task: {0}")]
    JoinError(#[from] tokio::task::JoinError),
}

// Specific error for Tauri command results when they need to be stringified
// This helps centralize the conversion.
impl From<AudioDecodingError> for String {
    fn from(err: AudioDecodingError) -> String {
        err.to_string()
    }
}
impl From<BpmError> for String {
    fn from(err: BpmError) -> String {
        err.to_string()
    }
}
impl From<AudioAnalysisError> for String {
    fn from(err: AudioAnalysisError) -> String {
        err.to_string()
    }
}
impl From<PlaybackError> for String {
    fn from(err: PlaybackError) -> String {
        err.to_string()
    }
}

#[derive(Error, Debug)]
pub enum AudioProcessorError {
    #[error("Decoding error during analysis for '{path}': {source}")]
    AnalysisDecodingError {
        path: String,
        source: AudioDecodingError,
    },
    #[error("BPM calculation failed for '{path}': {source}")]
    AnalysisBpmError { path: String, source: BpmError },
    #[error("Volume analysis failed for '{path}': {source}")]
    AnalysisVolumeError {
        path: String,
        source: AudioAnalysisError,
    },
    #[error(
        "Invalid data (empty samples or zero sample rate) for duration calculation for '{path}'."
    )]
    InvalidDataForDurationCalculation { path: String },
}

// For the HashMap<String, Result<AudioFeatures, String>> in analyze_features_batch
// We need a way to convert AudioProcessorError to a String for the Err case.
impl From<AudioProcessorError> for String {
    fn from(err: AudioProcessorError) -> String {
        err.to_string()
    }
}
