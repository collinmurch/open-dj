use crate::{
    audio_analysis,
    bpm_analyzer,
};
use crate::errors::AudioProcessorError; // Only this is needed if sub-errors are just sources
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

/// Decodes audio and calculates basic metadata (duration, BPM).
fn get_track_basic_metadata_internal(path: &str) -> Result<TrackBasicMetadata, AudioProcessorError> {
    log::info!("Metadata Intern: Starting basic metadata analysis for: {}", path);

    let (samples, sample_rate) = crate::audio_playback::decode_audio_for_playback(path)
        .map_err(|e| AudioProcessorError::AnalysisDecodingError{ path: path.to_string(), source: e })?;

    let duration_seconds = if sample_rate > 0.0 && !samples.is_empty() {
        Ok(samples.len() as f64 / sample_rate as f64)
    } else {
        log::warn!("Metadata Intern: Cannot calculate duration for '{}' due to zero sample rate or empty samples.", path);
        Err(AudioProcessorError::InvalidDataForDurationCalculation{ path: path.to_string() })
    };

    let bpm = bpm_analyzer::calculate_bpm(&samples, sample_rate)
        .map_err(|e| AudioProcessorError::AnalysisBpmError{ path: path.to_string(), source: e });

    Ok(TrackBasicMetadata {
        duration_seconds: duration_seconds.ok(), // Store Option<f64>
        bpm: bpm.ok(),                     // Store Option<f32>
    })
}

/// Decodes audio and calculates full volume analysis (WaveBin levels).
fn get_track_volume_analysis_internal(path: &str) -> Result<audio_analysis::AudioAnalysis, AudioProcessorError> {
    log::info!("Volume Intern: Starting volume analysis for: {}", path);

    let (samples, sample_rate) = crate::audio_playback::decode_audio_for_playback(path)
        .map_err(|e| AudioProcessorError::AnalysisDecodingError{ path: path.to_string(), source: e })?;
    
    audio_analysis::calculate_rms_intervals(&samples, sample_rate)
        .map_err(|e| AudioProcessorError::AnalysisVolumeError{ path: path.to_string(), source: e })
        .map(|(levels, max_rms_amplitude)| audio_analysis::AudioAnalysis {
            levels,
            max_rms_amplitude,
        })
}

// --- Batch Command (To be modified next) ---

#[tauri::command(async)]
pub fn analyze_features_batch(
    paths: Vec<String>,
) -> HashMap<String, Result<TrackBasicMetadata, String>> { // MODIFIED Return Type
    log::info!(
        "Metadata Batch CMD: Starting batch analysis for {} files",
        paths.len()
    );

    let results: HashMap<String, Result<TrackBasicMetadata, String>> = paths
        .par_iter() // Process paths in parallel
        .map(|path| {
            // Use the new internal function for basic metadata
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
pub fn get_track_volume_analysis(
    path: String,
) -> Result<audio_analysis::AudioAnalysis, String> {
    log::info!("Volume CMD: Request for: {}", path);
    get_track_volume_analysis_internal(&path).map_err(|e| {
        log::error!("Volume CMD: Error for path '{}': {}", path, e);
        e.to_string()
    })
}
