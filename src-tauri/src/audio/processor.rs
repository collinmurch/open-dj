use crate::audio::analysis::{bpm_analyzer, volume_analyzer};
use crate::audio::errors::AudioProcessorError;
use rayon::prelude::*;
use std::collections::HashMap;

// --- New Struct for Basic Metadata ---
#[derive(serde::Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackBasicMetadata {
    pub duration_seconds: Option<f64>,
    pub bpm: Option<f32>,
}

// --- Internal Helper Functions ---

/// Helper to log an error and convert a Result to an Option.
fn log_and_convert_to_option<T, E: std::fmt::Display>(
    result: Result<T, E>,
    path: &str,
    feature_name: &str,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            log::error!(
                "Metadata Intern: {} calculation failed for '{}': {}. Storing None.",
                feature_name,
                path,
                e
            );
            None
        }
    }
}

/// Decodes audio and calculates basic metadata (duration, BPM).
fn get_track_basic_metadata_internal(
    path: &str,
) -> Result<TrackBasicMetadata, AudioProcessorError> {
    log::info!(
        "Metadata Intern: Starting basic metadata analysis for: {}",
        path
    );

    let (samples, sample_rate) = crate::audio::decoding::decode_file_to_mono_samples(path)
        .map_err(|e| AudioProcessorError::AnalysisDecodingError {
            path: path.to_string(),
            source: e,
        })?;

    let duration_result = if sample_rate > 0.0 && !samples.is_empty() {
        Ok(samples.len() as f64 / sample_rate as f64)
    } else {
        log::warn!(
            "Metadata Intern: Cannot calculate duration for '{}' due to zero sample rate or empty samples.",
            path
        );
        Err(AudioProcessorError::InvalidDataForDurationCalculation {
            path: path.to_string(),
        })
    };

    let bpm_result = bpm_analyzer::calculate_bpm(&samples, sample_rate).map_err(|e| {
        AudioProcessorError::AnalysisBpmError {
            path: path.to_string(),
            source: e,
        }
    });

    let final_duration = log_and_convert_to_option(duration_result, path, "Duration");
    let final_bpm = log_and_convert_to_option(bpm_result, path, "BPM");

    Ok(TrackBasicMetadata {
        duration_seconds: final_duration,
        bpm: final_bpm,
    })
}

/// Decodes audio and calculates full volume analysis (WaveBin levels).
fn get_track_volume_analysis_internal(
    path: &str,
) -> Result<volume_analyzer::AudioAnalysis, AudioProcessorError> {
    // Adjusted type path
    log::info!("Volume Intern: Starting volume analysis for: {}", path);

    let (samples, sample_rate) = crate::audio::decoding::decode_file_to_mono_samples(path)
        .map_err(|e| AudioProcessorError::AnalysisDecodingError {
            path: path.to_string(),
            source: e,
        })?;

    volume_analyzer::calculate_rms_intervals(&samples, sample_rate)
        .map_err(|e| AudioProcessorError::AnalysisVolumeError {
            path: path.to_string(),
            source: e,
        })
        .map(|(levels, max_band_energy)| volume_analyzer::AudioAnalysis {
            levels,
            max_band_energy,
        })
}

// --- Batch Command (To be modified next) ---

#[tauri::command(async)]
pub fn analyze_features_batch(
    paths: Vec<String>,
) -> HashMap<String, Result<TrackBasicMetadata, String>> {
    log::info!(
        "Metadata Batch CMD: Starting batch analysis for {} files",
        paths.len()
    );

    let results: HashMap<String, Result<TrackBasicMetadata, String>> = paths
        .par_iter()
        .map(|path| {
            let metadata_result = get_track_basic_metadata_internal(path);
            match metadata_result {
                Ok(metadata) => (path.clone(), Ok(metadata)),
                Err(e) => {
                    log::error!("Basic metadata analysis failed for path '{}': {}", path, e);
                    (path.clone(), Err(e.to_string()))
                }
            }
        })
        .collect();

    log::info!("Metadata Batch CMD: Finished batch analysis.");
    results
}

// --- New Command for On-Demand Volume Analysis ---
#[tauri::command(async)]
pub fn get_track_volume_analysis(path: String) -> Result<volume_analyzer::AudioAnalysis, String> {
    log::info!("Volume CMD: Request for: {}", path);
    get_track_volume_analysis_internal(&path).map_err(|e| {
        log::error!("Volume CMD: Error for path '{}': {}", path, e);
        e.to_string()
    })
}
