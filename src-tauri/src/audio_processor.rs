// src-tauri/src/audio_processor.rs
use crate::{
    audio_analysis,
    bpm_analyzer,
    // errors::{AudioProcessorError, AudioDecodingError, BpmError, AudioAnalysisError} // This line to be replaced
};
use crate::errors::AudioProcessorError; // Only this is needed if sub-errors are just sources
use rayon::prelude::*;
use std::collections::HashMap;

// --- Combined Result Structure ---

#[derive(serde::Serialize, Debug, Clone)]
pub struct AudioFeatures {
    // Store results directly, None if analysis failed for that part
    pub bpm: Option<f32>,
    pub volume: Option<audio_analysis::AudioAnalysis>, // Reuse existing struct
}

// --- Analysis Functions ---

/// Analyzes a single file, performing decoding and then parallel analysis.
fn analyze_single_file_features(path: &str) -> Result<AudioFeatures, AudioProcessorError> {
    log::info!("Analysis: Starting combined analysis for: {}", path);

    let (samples, sample_rate) = crate::audio_playback::decode_audio_for_playback(path)
        .map_err(|e| AudioProcessorError::AnalysisDecodingError{ path: path.to_string(), source: e })?;

    let (bpm_analysis_result, volume_analysis_result) = rayon::join(
        || {
            bpm_analyzer::calculate_bpm(&samples, sample_rate)
                .map_err(|e| AudioProcessorError::AnalysisBpmError{ path: path.to_string(), source: e })
        },
        || {
            audio_analysis::calculate_rms_intervals(&samples, sample_rate)
                .map_err(|e| AudioProcessorError::AnalysisVolumeError{ path: path.to_string(), source: e })
                .map(|(intervals, max_rms_amplitude)| audio_analysis::AudioAnalysis {
                    intervals,
                    max_rms_amplitude,
                })
        },
    );

    // Combine results and handle errors from individual analyses
    match (bpm_analysis_result, volume_analysis_result) {
        (Ok(bpm_val), Ok(vol_analysis)) => {
            log::info!("Analysis: BPM success for '{}': {:.2}", path, bpm_val);
            log::info!("Analysis: Volume success for '{}': {} intervals, max RMS {:.4}", 
                       path, vol_analysis.intervals.len(), vol_analysis.max_rms_amplitude);
            Ok(AudioFeatures { bpm: Some(bpm_val), volume: Some(vol_analysis) })
        }
        (Err(bpm_err), Ok(vol_analysis)) => {
            log::error!("Analysis: BPM failed for '{}': {}", path, bpm_err);
            log::info!("Analysis: Volume succeeded for '{}': {} intervals, max RMS {:.4}", 
                       path, vol_analysis.intervals.len(), vol_analysis.max_rms_amplitude);
            // Depending on requirements, could return partial success or the error.
            // Current AudioFeatures struct allows partial. To propagate error: return Err(bpm_err);
            Ok(AudioFeatures { bpm: None, volume: Some(vol_analysis) })
        }
        (Ok(bpm_val), Err(vol_err)) => {
            log::info!("Analysis: BPM success for '{}': {:.2}", path, bpm_val);
            log::error!("Analysis: Volume failed for '{}': {}", path, vol_err);
            // To propagate error: return Err(vol_err);
            Ok(AudioFeatures { bpm: Some(bpm_val), volume: None })
        }
        (Err(bpm_err), Err(vol_err)) => {
            log::error!("Analysis: BPM failed for '{}': {}", path, bpm_err);
            log::error!("Analysis: Volume failed for '{}': {}", path, vol_err);
            // Propagate a combined error or one of the specific errors.
            Err(AudioProcessorError::CombinedAnalysisFailed{ path: path.to_string() })
        }
    }
}

// --- Batch Command ---

#[tauri::command(async)]
pub fn analyze_features_batch(
    paths: Vec<String>,
) -> HashMap<String, Result<AudioFeatures, String>> {
    log::info!(
        "Analysis: Starting batch analysis for {} files",
        paths.len()
    );

    let results: HashMap<String, Result<AudioFeatures, String>> = paths
        .par_iter() // Process paths in parallel
        .map(|path| {
            // Use the updated analysis function
            let analysis_result = analyze_single_file_features(path);
            // Log top-level errors here, specific errors logged within analyze_single_file_features
            match analysis_result {
                Ok(features) => (path.clone(), Ok(features)),
                Err(e) => {
                    log::error!("Feature analysis failed for path '{}': {}", path, e);
                    (path.clone(), Err(e.to_string())) // Convert AudioProcessorError to String
                }
            }
        })
        .collect();

    log::info!("Analysis: Finished batch analysis.");
    results
}
