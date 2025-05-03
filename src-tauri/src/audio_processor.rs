// src-tauri/src/audio_processor.rs
use crate::{audio_analysis, bpm_analyzer};
use rayon::prelude::*;
use std::{collections::HashMap, fs::File};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

// --- Combined Result Structure ---

#[derive(serde::Serialize, Debug, Clone)]
pub struct AudioFeatures {
    // Store results directly, None if analysis failed for that part
    pub bpm: Option<f32>,
    pub volume: Option<audio_analysis::AudioAnalysis>, // Reuse existing struct
}

// --- Central Decoding Function ---

/// Decodes an audio file to mono f32 samples.
/// Takes a file path and returns the samples and the original sample rate.
fn decode_audio_to_mono_f32(path: &str) -> Result<(Vec<f32>, f32), String> {
    let file =
        File::open(path).map_err(|e| format!("Decode: Failed to open file '{}': {}", path, e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let hint = Hint::new();

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Decode: Failed to probe format for '{}': {}", path, e))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| format!("Decode: No suitable audio track found in '{}'", path))?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| format!("Decode: Sample rate not found for track in '{}'", path))?
        as f32; // Store as f32 directly
    let channels = track
        .codec_params
        .channels
        .ok_or_else(|| format!("Decode: Channel info not found for track in '{}'", path))?
        .count();
    let codec_params = track.codec_params.clone(); // Clone params needed for decoder

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Decode: Failed to create decoder for '{}': {}", path, e))?;

    let mut samples: Vec<f32> = Vec::with_capacity(1024 * 256); // Pre-allocate estimate
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        match format.next_packet() {
            Ok(packet) => {
                if packet.track_id() != track_id {
                    continue;
                }

                match decoder.decode(&packet) {
                    Ok(audio_buf) => {
                        if sample_buf.is_none() {
                            sample_buf = Some(SampleBuffer::<f32>::new(
                                audio_buf.capacity() as u64,
                                *audio_buf.spec(),
                            ));
                        }
                        if let Some(buf) = sample_buf.as_mut() {
                            buf.copy_interleaved_ref(audio_buf);
                            let raw_samples = buf.samples();

                            if channels > 1 {
                                // Calculate mono samples chunk by chunk
                                samples.extend(
                                    raw_samples
                                        .chunks_exact(channels)
                                        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32),
                                );
                            } else {
                                samples.extend_from_slice(raw_samples);
                            }
                        }
                    }
                    Err(SymphoniaError::DecodeError(err)) => {
                        log::warn!("Decode: Ignoring decode error in '{}': {}", path, err);
                    }
                    Err(e) => {
                        return Err(format!("Decode: Fatal decode error in '{}': {}", path, e));
                    }
                }
            }
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                log::debug!("Decode: Reached EOF for '{}'", path);
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                log::warn!("Decode: Decoder reset required unexpectedly for '{}'", path);
                break; // Treat as error or EOF for simplicity here
            }
            Err(e) => {
                return Err(format!(
                    "Decode: Error reading audio packet for '{}': {}",
                    path, e
                ));
            }
        }
    }

    decoder.finalize(); // Finalize decoder state if needed
    log::debug!(
        "Decode: Decoded {} mono samples at {} Hz for '{}'",
        samples.len(),
        sample_rate,
        path
    );
    if samples.is_empty() {
        return Err(format!("Decode: No samples decoded from '{}'", path));
    }

    Ok((samples, sample_rate))
}

// --- Analysis Functions ---

/// Analyzes a single file, performing decoding and then parallel analysis.
fn analyze_single_file_features(path: &str) -> Result<AudioFeatures, String> {
    log::info!("Analysis: Starting combined analysis for: {}", path);

    // 1. Decode centrally
    let (samples, sample_rate) = decode_audio_to_mono_f32(path)?;

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
