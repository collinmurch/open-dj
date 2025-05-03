// src-tauri/src/audio_processor.rs
use crate::{audio_analysis, bpm_analyzer};
use rayon::prelude::*;
use std::collections::HashMap;

// --- Combined Result Structure ---

#[derive(serde::Serialize, Debug, Clone)]
pub struct AudioFeatures {
    // Store results directly, None if analysis failed for that part
    pub bpm: Option<f32>,
    pub volume: Option<audio_analysis::AudioAnalysis>, // Reuse existing struct
}

// --- Central Decoding Function ---

// REMOVED: decode_audio_to_mono_f32 moved to audio_playback.rs

// --- Analysis Functions ---

/// Analyzes a single file, performing decoding and then parallel analysis.
fn analyze_single_file_features(path: &str) -> Result<AudioFeatures, String> {
    log::info!("Analysis: Starting combined analysis for: {}", path);

    // 1. Decode centrally - Use the playback module's decoder now
    let (samples, sample_rate) = crate::audio_playback::decode_audio_for_playback(path)
        .map_err(|e| format!("Analysis Decode Error: {}", e))?; // Map error type if needed

    // 2. Run analyses in parallel
    let (bpm_result, volume_result) = rayon::join(
        || {
            log::debug!("Analysis: Starting BPM calculation for '{}'", path);
            let bpm = bpm_analyzer::calculate_bpm(&samples, sample_rate);
            log::debug!("Analysis: Finished BPM calculation for '{}'", path);
            bpm // Returns Result<f32, String>
        },
        || {
            log::debug!("Analysis: Starting Volume calculation for '{}'", path);
            // Pass f32 sample rate, cast inside if necessary
            let volume_data = audio_analysis::calculate_rms_intervals(&samples, sample_rate);
            log::debug!("Analysis: Finished Volume calculation for '{}'", path);
            // volume_data is Result<(Vec<VolumeInterval>, f32), String>
            // We need to map the Ok case to AudioAnalysis
            volume_data.map(
                |(intervals, max_rms_amplitude)| audio_analysis::AudioAnalysis {
                    intervals,
                    max_rms_amplitude,
                },
            )
        },
    );

    // 3. Combine results
    let bpm = match bpm_result {
        Ok(val) => {
            log::info!("Analysis: BPM success for '{}': {:.2}", path, val);
            Some(val)
        }
        Err(e) => {
            log::error!("Analysis: BPM failed for '{}': {}", path, e);
            None
        }
    };

    let volume = match volume_result {
        Ok(analysis_struct) => {
            log::info!(
                "Analysis: Volume success for '{}': {} intervals, max RMS {:.4}",
                path,
                analysis_struct.intervals.len(),
                analysis_struct.max_rms_amplitude
            );
            Some(analysis_struct)
        }
        Err(e) => {
            log::error!("Analysis: Volume failed for '{}': {}", path, e);
            None
        }
    };

    Ok(AudioFeatures { bpm, volume })
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
            if let Err(e) = &analysis_result {
                log::error!("Analysis: Top-level error for '{}': {}", path, e);
            }
            (path.clone(), analysis_result)
        })
        .collect();

    log::info!("Analysis: Finished batch analysis.");
    results
}
