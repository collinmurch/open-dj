use crate::audio::types::TrackBasicMetadata;
use crate::audio::errors::AudioProcessorError;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

// --- New Struct for Basic Metadata ---


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

/// Optimized function that decodes once and calculates all metadata
fn get_track_metadata_and_samples_internal(
    path: &str,
) -> Result<(TrackBasicMetadata, Arc<Vec<f32>>, f32), AudioProcessorError> {
    log::info!(
        "Metadata Intern: Starting optimized metadata analysis for: {}",
        path
    );
    
    // Decode once and reuse for all analysis
    let (samples, sample_rate) = crate::audio::decoding::decode_file_to_mono_samples(path)
        .map_err(|e| AudioProcessorError::AnalysisDecodingError {
            path: path.to_string(),
            source: e,
        })?;
    
    let samples_arc = Arc::new(samples);
    
    let duration_result = if sample_rate > 0.0 && !samples_arc.is_empty() {
        Ok(samples_arc.len() as f64 / sample_rate as f64)
    } else {
        log::warn!(
            "Metadata Intern: Cannot calculate duration for '{}' due to zero sample rate or empty samples.",
            path
        );
        Err(AudioProcessorError::InvalidDataForDurationCalculation {
            path: path.to_string(),
        })
    };
    
    let (bpm, first_beat_sec) = crate::audio::analysis::bpm_analyzer::analyze_bpm(&samples_arc, sample_rate)
        .map_err(|e| AudioProcessorError::AnalysisBpmError {
            path: path.to_string(),
            source: e,
        })?;
    
    let final_duration = log_and_convert_to_option(duration_result, path, "Duration");
    let final_bpm = Some(bpm);
    let final_first_beat_sec = Some(first_beat_sec);
    
    let metadata = TrackBasicMetadata {
        duration_seconds: final_duration,
        bpm: final_bpm,
        first_beat_sec: final_first_beat_sec,
    };
    
    Ok((metadata, samples_arc, sample_rate))
}

/// Decodes audio and calculates basic metadata (duration, BPM).
pub fn get_track_basic_metadata_internal(
    path: &str,
) -> Result<TrackBasicMetadata, AudioProcessorError> {
    let (metadata, _, _) = get_track_metadata_and_samples_internal(path)?;
    Ok(metadata)
}

/// Decodes audio and calculates full volume analysis (WaveBin levels).
fn get_track_volume_analysis_internal(
    path: &str,
) -> Result<crate::audio::types::AudioAnalysis, AudioProcessorError> {
    log::info!("Volume Intern: Starting volume analysis for: {}", path);
    let (samples, sample_rate) = crate::audio::decoding::decode_file_to_mono_samples(path)
        .map_err(|e| AudioProcessorError::AnalysisDecodingError {
            path: path.to_string(),
            source: e,
        })?;
    crate::audio::analysis::volume_analyzer::calculate_rms_intervals(&samples, sample_rate)
        .map_err(|e| AudioProcessorError::AnalysisVolumeError {
            path: path.to_string(),
            source: e,
        })
        .map(|(levels, max_band_energy)| crate::audio::types::AudioAnalysis {
            levels,
            max_band_energy,
        })
}

/// Optimized function for when both metadata and volume analysis are needed
pub fn get_track_complete_analysis_internal(
    path: &str,
) -> Result<(TrackBasicMetadata, crate::audio::types::AudioAnalysis), AudioProcessorError> {
    log::info!("Complete Intern: Starting complete analysis for: {}", path);
    let (metadata, samples_arc, sample_rate) = get_track_metadata_and_samples_internal(path)?;
    
    let volume_analysis = crate::audio::analysis::volume_analyzer::calculate_rms_intervals(&samples_arc, sample_rate)
        .map_err(|e| AudioProcessorError::AnalysisVolumeError {
            path: path.to_string(),
            source: e,
        })
        .map(|(levels, max_band_energy)| crate::audio::types::AudioAnalysis {
            levels,
            max_band_energy,
        })?;
    
    Ok((metadata, volume_analysis))
}

// --- Batch Command (To be modified next) ---

#[tauri::command(async)]
pub fn analyze_features_batch(
    paths: Vec<String>,
) -> HashMap<String, Result<TrackBasicMetadata, String>> {
    analyze_features_batch_with_cache(paths, None)
}

#[tauri::command(async)]
pub fn analyze_features_batch_with_cache(
    paths: Vec<String>,
    cache_dir: Option<String>,
) -> HashMap<String, Result<TrackBasicMetadata, String>> {
    log::info!(
        "Metadata Batch CMD: Starting batch analysis for {} files (cache: {})",
        paths.len(),
        cache_dir.is_some()
    );

    let cache_path = cache_dir.map(|dir| std::path::PathBuf::from(dir));

    let results: HashMap<String, Result<TrackBasicMetadata, String>> = paths
        .par_iter()
        .map(|path| {
            let analysis_result = if let Some(ref cache_dir) = cache_path {
                match crate::audio::cache::analyze_with_cache(path, Some(cache_dir), false) {
                    Ok((metadata, _)) => Ok(metadata),
                    Err(e) => {
                        log::warn!("Cache analysis failed for {}: {}. Falling back to direct analysis.", path, e);
                        get_track_basic_metadata_internal(path)
                    }
                }
            } else {
                get_track_basic_metadata_internal(path)
            };

            match analysis_result {
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
pub fn get_track_volume_analysis(path: String) -> Result<crate::audio::types::AudioAnalysis, String> {
    log::info!("Volume CMD: Request for: {}", path);
    get_track_volume_analysis_internal(&path).map_err(|e| {
        log::error!("Volume CMD: Error for path '{}': {}", path, e);
        e.to_string()
    })
}

// --- New Command for Complete Analysis (Optimized) ---
#[tauri::command(async)]
pub fn get_track_complete_analysis(
    path: String,
) -> Result<(TrackBasicMetadata, crate::audio::types::AudioAnalysis), String> {
    log::info!("Complete CMD: Request for: {}", path);
    get_track_complete_analysis_internal(&path).map_err(|e| {
        log::error!("Complete CMD: Error for path '{}': {}", path, e);
        e.to_string()
    })
}

// --- Batch Analysis with Complete Data ---
#[tauri::command(async)]
pub fn analyze_features_and_waveforms_batch(
    paths: Vec<String>,
) -> HashMap<String, Result<(TrackBasicMetadata, crate::audio::types::AudioAnalysis), String>> {
    analyze_features_and_waveforms_batch_with_cache(paths, None)
}

#[tauri::command(async)]
pub fn analyze_features_and_waveforms_batch_with_cache(
    paths: Vec<String>,
    cache_dir: Option<String>,
) -> HashMap<String, Result<(TrackBasicMetadata, crate::audio::types::AudioAnalysis), String>> {
    log::info!(
        "Complete Batch CMD: Starting batch complete analysis for {} files (cache: {})",
        paths.len(),
        cache_dir.is_some()
    );

    let cache_path = cache_dir.map(|dir| std::path::PathBuf::from(dir));

    let results: HashMap<String, Result<(TrackBasicMetadata, crate::audio::types::AudioAnalysis), String>> = paths
        .par_iter()
        .map(|path| {
            let analysis_result = if let Some(ref cache_dir) = cache_path {
                match crate::audio::cache::analyze_with_cache(path, Some(cache_dir), true) {
                    Ok((metadata, Some(waveform))) => Ok((metadata, waveform)),
                    Ok((_, None)) => {
                        log::warn!("Waveform analysis missing from cache for {}. Falling back to direct analysis.", path);
                        get_track_complete_analysis_internal(path)
                    }
                    Err(e) => {
                        log::warn!("Cache analysis failed for {}: {}. Falling back to direct analysis.", path, e);
                        get_track_complete_analysis_internal(path)
                    }
                }
            } else {
                get_track_complete_analysis_internal(path)
            };

            match analysis_result {
                Ok((metadata, waveform)) => (path.clone(), Ok((metadata, waveform))),
                Err(e) => {
                    log::error!("Complete analysis failed for path '{}': {}", path, e);
                    (path.clone(), Err(e.to_string()))
                }
            }
        })
        .collect();

    log::info!("Complete Batch CMD: Finished batch complete analysis.");
    results
}
