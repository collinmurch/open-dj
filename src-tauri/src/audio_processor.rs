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
#[serde(rename_all = "camelCase")]
pub struct AudioFeatures {
    // Store results directly, None if analysis failed for that part
    pub bpm: Option<f32>,
    pub volume: Option<audio_analysis::AudioAnalysis>, // Reuse existing struct
    pub duration_seconds: Option<f64>, // Field name in Rust remains snake_case
}

// --- Analysis Functions ---

/// Analyzes a single file, performing decoding and then parallel analysis.
fn analyze_single_file_features(path: &str) -> Result<AudioFeatures, AudioProcessorError> {
    log::info!("Analysis: Starting combined analysis for: {}", path);

    let (samples, sample_rate) = crate::audio_playback::decode_audio_for_playback(path)
        .map_err(|e| AudioProcessorError::AnalysisDecodingError{ path: path.to_string(), source: e })?;

    let duration_calc_result = if sample_rate > 0.0 && !samples.is_empty() {
        Ok(samples.len() as f64 / sample_rate as f64)
    } else {
        log::warn!("Analysis: Cannot calculate duration for '{}' due to zero sample rate or empty samples.", path);
        Err(AudioProcessorError::InvalidDataForDurationCalculation{ path: path.to_string() })
    };

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

    let final_duration_seconds = match duration_calc_result {
        Ok(d) => Some(d),
        Err(e) => {
            log::error!("Analysis: Duration calculation failed for '{}': {}", path, e);
            None
        }
    };

    // Combine results and handle errors from individual analyses
    match (bpm_analysis_result, volume_analysis_result) {
        (Ok(bpm_val), Ok(vol_analysis)) => {
            log::info!("Analysis: BPM success for '{}': {:.2}", path, bpm_val);
            log::info!("Analysis: Volume success for '{}': {} intervals, max RMS {:.4}", 
                       path, vol_analysis.intervals.len(), vol_analysis.max_rms_amplitude);
            Ok(AudioFeatures { 
                bpm: Some(bpm_val), 
                volume: Some(vol_analysis), 
                duration_seconds: final_duration_seconds 
            })
        }
        (Err(bpm_err), Ok(vol_analysis)) => {
            log::error!("Analysis: BPM failed for '{}': {}", path, bpm_err);
            log::info!("Analysis: Volume succeeded for '{}': {} intervals, max RMS {:.4}", 
                       path, vol_analysis.intervals.len(), vol_analysis.max_rms_amplitude);
            Ok(AudioFeatures { 
                bpm: None, 
                volume: Some(vol_analysis), 
                duration_seconds: final_duration_seconds 
            })
        }
        (Ok(bpm_val), Err(vol_err)) => {
            log::info!("Analysis: BPM success for '{}': {:.2}", path, bpm_val);
            log::error!("Analysis: Volume failed for '{}': {}", path, vol_err);
            Ok(AudioFeatures { 
                bpm: Some(bpm_val), 
                volume: None, 
                duration_seconds: final_duration_seconds 
            })
        }
        (Err(bpm_err), Err(vol_err)) => {
            log::error!("Analysis: BPM failed for '{}': {}", path, bpm_err);
            log::error!("Analysis: Volume failed for '{}': {}", path, vol_err);
            Err(AudioProcessorError::CombinedAnalysisFailed{ path: path.to_string() })
            // Even if BPM/Volume fails, we might still want to return duration if it was calculated.
            // However, current error path returns a full error. If partial success with duration is needed,
            // this part needs restructuring, e.g., always returning Ok(AudioFeatures{...}) and putting errors inside.
            // For now, if both BPM/Vol fail, the whole feature analysis for the file is considered failed.
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
