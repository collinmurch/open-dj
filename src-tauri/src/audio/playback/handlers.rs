use std::collections::HashMap;
use std::sync::{Arc, Mutex}; // Mutex for EqParams, trim_gain, current_pitch_rate
use std::time::Duration;

// Remove Rodio specific imports
// use rodio::{buffer::SamplesBuffer, Sink, Source}; 

// Add CPAL imports
use cpal::{Device, StreamConfig, SupportedStreamConfigRange};
use cpal::traits::{DeviceTrait, StreamTrait};
use tauri::{AppHandle, Runtime};

use crate::audio::config::INITIAL_TRIM_GAIN; // Changed from {self, INITIAL_TRIM_GAIN}
use crate::audio::decoding;
use crate::audio::effects; // Import the effects module
use crate::audio::errors::PlaybackError;
use crate::audio::types::EqParams;     // Used by handlers

use super::state::AudioThreadDeckState;
use super::events::*; // For calling emit_..._event functions
use biquad::DirectForm1; // Import DirectForm1
use biquad::Biquad; // Import the Biquad trait

// --- Private Handler Functions for Audio Thread Commands ---

/// Initializes a new deck in the local state.
/// Returns an error if a lock cannot be acquired or the deck already exists.
pub(crate) fn audio_thread_handle_init<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    if local_states.contains_key(deck_id) {
        log::warn!(
            "Audio Thread: InitDeck: Deck '{}' already exists. No action taken.",
            deck_id
        );
        emit_error_event(app_handle, deck_id, "Deck already initialized.");
        return Ok(());
    }

    // No Sink creation here. Stream is created on load.
    let initial_eq_params = EqParams::default();
    let initial_current_eq_params_shared = Arc::new(Mutex::new(initial_eq_params.clone()));
    let initial_target_eq_params_shared = Arc::new(Mutex::new(initial_eq_params.clone()));
    
    let initial_linear_trim_gain = INITIAL_TRIM_GAIN;
    let initial_current_trim_gain_shared = Arc::new(Mutex::new(initial_linear_trim_gain));
    let initial_target_trim_gain_shared = Arc::new(Mutex::new(initial_linear_trim_gain));

    let initial_pitch_val = 1.0f32;
    let initial_current_pitch_rate_shared = Arc::new(Mutex::new(initial_pitch_val));
    let initial_target_pitch_rate_shared = Arc::new(Mutex::new(initial_pitch_val));

    // Initial flat coefficients for filters (assuming 0.0 sample rate initially, will be updated on load)
    // It's better to initialize with valid, albeit possibly dummy, coefficients.
    // Using a placeholder sample rate like 44100.0 for initial coefficient calculation.
    // These will be recalculated when a track is loaded with its actual sample rate.
    let placeholder_sr = 44100.0;
    let default_coeffs = effects::calculate_low_shelf(placeholder_sr, 0.0)
        .unwrap_or_else(|e| {
            log::warn!("Failed to create default low_shelf coeffs: {}. Using default flat Coefficients.", e);
            biquad::Coefficients { // Return Coefficients struct directly
                a1: 0.0, a2: 0.0, b0: 1.0, b1: 0.0, b2: 0.0,
            }
        });

    let low_shelf_filter = Arc::new(Mutex::new(DirectForm1::<f32>::new(default_coeffs)));
    let mid_peak_filter = Arc::new(Mutex::new(DirectForm1::<f32>::new(
        effects::calculate_mid_peak(placeholder_sr, 0.0).unwrap_or(default_coeffs)
    )));
    let high_shelf_filter = Arc::new(Mutex::new(DirectForm1::<f32>::new(
        effects::calculate_high_shelf(placeholder_sr, 0.0).unwrap_or(default_coeffs)
    )));
    let last_eq_params = Arc::new(Mutex::new(EqParams::default()));

    let deck_state = AudioThreadDeckState {
        cpal_stream: None, // Stream created on load
        decoded_samples: Arc::new(Vec::new()),
        sample_rate: 0.0,
        current_sample_read_head: Arc::new(Mutex::new(0.0)), // Initialize with 0.0
        paused_position_read_head: Arc::new(Mutex::new(Some(0.0))), // Initialize with Some(0.0)
        duration: Duration::ZERO,
        is_playing: Arc::new(Mutex::new(false)),
        current_eq_params: initial_current_eq_params_shared,
        target_eq_params: initial_target_eq_params_shared,
        current_trim_gain: initial_current_trim_gain_shared,
        target_trim_gain: initial_target_trim_gain_shared,
        cue_point: None,
        current_pitch_rate: initial_current_pitch_rate_shared,
        target_pitch_rate: initial_target_pitch_rate_shared,
        last_ui_pitch_rate: Some(1.0),
        original_bpm: None,
        first_beat_sec: None,
        is_sync_active: false,
        is_master: false,
        master_deck_id: None,
        target_pitch_rate_for_bpm_match: 1.0,
        manual_pitch_rate: 1.0,
        pll_integral_error: 0.0,
        // --- EQ Filter Instances (Phase 3) ---
        low_shelf_filter,
        mid_peak_filter,
        high_shelf_filter,
        last_eq_params,
        // --- Sync Feature Fields ---
        output_sample_rate: None,
        last_playback_instant: Arc::new(Mutex::new(None)),
        read_head_at_last_playback_instant: Arc::new(Mutex::new(None)),
        seek_fade_state: Arc::new(Mutex::new(None)),
        channel_fader_level: Arc::new(Mutex::new(1.0f32)), // Initialize channel_fader_level
    };
    local_states.insert(deck_id.to_string(), deck_state);
    log::info!("Audio Thread: Initialized deck '{}' for CPAL", deck_id);

    // Emit initial state events
    emit_load_update_event(app_handle, deck_id, 0.0, None, None, None);
    emit_status_update_event(app_handle, deck_id, false);
    emit_sync_status_update_event(app_handle, deck_id, false, false);
    emit_pitch_tick_event(app_handle, deck_id, 1.0);
    Ok(())
}

pub(crate) async fn audio_thread_handle_load<R: Runtime>(
    deck_id: String,
    path: String,
    original_bpm: Option<f32>,
    first_beat_sec: Option<f32>,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    cpal_device: &Device,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    if !local_states.contains_key(&deck_id) {
        let err_msg = format!("Deck '{}' not initialized before load.", deck_id);
        log::error!("Audio Thread: LoadTrack: {}", err_msg);
        emit_error_event(app_handle, &deck_id, &err_msg);
        return Ok(());
    }
    if let Some(state) = local_states.get_mut(&deck_id) {
        if state.cpal_stream.take().is_some() {
            log::info!("Audio Thread: Dropped existing CPAL stream for deck '{}' before loading new track.", deck_id);
        }
    }
    let path_clone = path.clone();
    let decode_app_handle = app_handle.clone();
    let decode_deck_id = deck_id.clone();
    let decode_result = tokio::task::spawn_blocking(move || {
        decoding::decode_file_to_mono_samples(&path_clone)
    }).await;
    match decode_result {
        Ok(Ok((samples, rate))) => {
            let duration_val = Duration::from_secs_f64(samples.len() as f64 / rate as f64);
            log::info!(
                "Audio Thread: Decoded '{}'. Duration: {:?}, Rate: {}, Samples: {}",
                path, duration_val, rate, samples.len()
            );
            let supported_configs = match cpal_device.supported_output_configs() {
                Ok(configs) => configs.collect::<Vec<_>>(),
                Err(e) => {
                    let err = PlaybackError::CpalSupportedStreamConfigsError(e);
                    log::error!("Audio Thread: LoadTrack: Could not get supported configs for deck '{}': {:?}", deck_id, err);
                    emit_error_event(app_handle, &deck_id, &err.to_string());
                    return Ok(());
                }
            };
            let target_track_sample_rate = rate as u32;
            let mut best_config: Option<SupportedStreamConfigRange> = None;
            for config_range in supported_configs.iter() {
                if config_range.sample_format() == cpal::SampleFormat::F32 {
                    if config_range.min_sample_rate().0 <= target_track_sample_rate && config_range.max_sample_rate().0 >= target_track_sample_rate {
                        if config_range.channels() == 2 {
                            best_config = Some(config_range.clone());
                            break;
                        }
                        if best_config.is_none() || best_config.as_ref().map(|c| c.channels() != 2).unwrap_or(false) {
                            best_config = Some(config_range.clone());
                        }
                    }
                }
            }
            if best_config.is_none() {
                for target_sr in [48000, 44100].iter() {
                    for config_range in supported_configs.iter() {
                        if config_range.sample_format() == cpal::SampleFormat::F32 {
                            if config_range.min_sample_rate().0 <= *target_sr && config_range.max_sample_rate().0 >= *target_sr {
                                if config_range.channels() == 2 {
                                    best_config = Some(config_range.clone());
                                    break;
                                }
                                if best_config.is_none() || best_config.as_ref().map(|c| c.channels() != 2).unwrap_or(false) {
                                    best_config = Some(config_range.clone());
                                }
                            }
                        }
                    }
                    if best_config.is_some() && best_config.as_ref().map(|c| c.channels() == 2 && c.min_sample_rate().0 <= *target_sr && c.max_sample_rate().0 >= *target_sr).unwrap_or(false) {
                        break;
                    }
                }
            }
            if best_config.is_none() {
                let mut f32_configs: Vec<SupportedStreamConfigRange> = supported_configs.iter()
                    .filter(|c| c.sample_format() == cpal::SampleFormat::F32)
                    .cloned()
                    .collect();
                if !f32_configs.is_empty() {
                    f32_configs.sort_by(|a, b| {
                        b.channels().cmp(&a.channels())
                         .then_with(|| b.max_sample_rate().cmp(&a.max_sample_rate()))
                    });
                    best_config = Some(f32_configs[0].clone());
                }
            }
            let chosen_supported_config_range = match best_config {
                Some(conf) => conf,
                None => {
                    log::error!("Audio Thread: LoadTrack: No suitable F32 output stream configuration found for device on deck '{}'. Available: {:?}", deck_id, supported_configs);
                    emit_error_event(app_handle, &deck_id, "No suitable F32 audio output configuration found.");
                    return Ok(());
                }
            };
            let cpal_sample_rate_val = if chosen_supported_config_range.min_sample_rate().0 <= target_track_sample_rate && chosen_supported_config_range.max_sample_rate().0 >= target_track_sample_rate {
                target_track_sample_rate 
            } else if chosen_supported_config_range.min_sample_rate().0 <= 48000 && chosen_supported_config_range.max_sample_rate().0 >= 48000 {
                 48000
            } else if chosen_supported_config_range.min_sample_rate().0 <= 44100 && chosen_supported_config_range.max_sample_rate().0 >= 44100 {
                 44100
            }
            else {
                 chosen_supported_config_range.max_sample_rate().0
            };
            let cpal_sample_rate = cpal::SampleRate(cpal_sample_rate_val);
            let cpal_channels = chosen_supported_config_range.channels();
            if (cpal_sample_rate.0 as f32 - rate).abs() > 1.0 {
                 log::warn!("Audio Thread: Sample rate mismatch for deck '{}'. Track: {} Hz, CPAL Stream: {} Hz. Playback quality may be affected if resampling is not perfect (or not yet implemented).",
                    deck_id, rate, cpal_sample_rate.0);
            } else {
                log::info!("Audio Thread: Matched sample rate for deck '{}'. Track: {} Hz, CPAL Stream: {} Hz.", deck_id, rate, cpal_sample_rate.0);
            }
            let stream_config = StreamConfig {
                channels: cpal_channels,
                sample_rate: cpal_sample_rate,
                buffer_size: cpal::BufferSize::Default,
            };
            let samples_arc = std::sync::Arc::new(samples);
            let deck_state = local_states.get_mut(&deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.clone() })?;
            deck_state.decoded_samples = samples_arc.clone();

            let current_sample_read_head_arc = deck_state.current_sample_read_head.clone();
            let is_playing_arc = deck_state.is_playing.clone();
            let _app_handle_clone_for_callback = app_handle.clone();
            let deck_id_clone_for_callback = deck_id.clone();
            let track_total_samples = samples_arc.len();
            let stream_output_channels = cpal_channels;
            
            // --- EQ and Trim references for the callback (Phase 3) ---
            let last_eq_params_mut = deck_state.last_eq_params.clone(); // Mutex for last_eq_params
            let low_shelf_filter_mut = deck_state.low_shelf_filter.clone(); // Mutex for low_shelf_filter
            let mid_peak_filter_mut = deck_state.mid_peak_filter.clone();   // Mutex for mid_peak_filter
            let high_shelf_filter_mut = deck_state.high_shelf_filter.clone(); // Mutex for high_shelf_filter
            let track_sample_rate_for_eq = rate; // Actual sample rate of the track for EQ calc

            // --- Precise Timing (Phase 5) --- 
            let last_playback_instant_arc = deck_state.last_playback_instant.clone();
            let read_head_at_last_playback_instant_arc = deck_state.read_head_at_last_playback_instant.clone();

            // --- Smoothing (Phase 6) ---
            let current_eq_params_arc = deck_state.current_eq_params.clone();
            let target_eq_params_arc = deck_state.target_eq_params.clone();
            let current_trim_gain_arc = deck_state.current_trim_gain.clone();
            let target_trim_gain_arc = deck_state.target_trim_gain.clone();
            const EQ_TRIM_SMOOTHING_FACTOR: f32 = 0.005; // For per-sample smoothing

            // --- Pitch Smoothing (Phase 6) ---
            let current_pitch_rate_arc_cb = deck_state.current_pitch_rate.clone(); // Renamed to avoid conflict with outer scope
            let target_pitch_rate_arc_cb = deck_state.target_pitch_rate.clone(); // Renamed
            const PITCH_SMOOTHING_FACTOR: f32 = 0.005; // Per-sample smoothing factor for pitch

            // --- Seek Fading (Phase 6) ---
            let seek_fade_state_arc = deck_state.seek_fade_state.clone();
            const SEEK_FADE_INCREMENT_PER_BUFFER: f32 = 0.05; // Takes ~20 buffers to fade in
            let channel_fader_level_arc = deck_state.channel_fader_level.clone(); // Clone for callback

            let data_callback = move |output: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                log::trace!("[Callback {}] Entered data_callback.", deck_id_clone_for_callback);

                // --- Store Playback Timestamp (Phase 5) ---
                let now_for_timing = std::time::Instant::now(); // Get current system time
                let read_head_before_advancing_for_this_buffer = *current_sample_read_head_arc.lock().unwrap();
                *last_playback_instant_arc.lock().unwrap() = Some(now_for_timing); // Store std::time::Instant
                *read_head_at_last_playback_instant_arc.lock().unwrap() = Some(read_head_before_advancing_for_this_buffer);
                // --- End Store Playback Timestamp ---

                let mut is_playing_guard = match is_playing_arc.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        log::error!("[Callback {}] is_playing_arc Mutex poisoned: {}. Audio will stop.", deck_id_clone_for_callback, poisoned);
                        // Fill output with silence and return to prevent further processing with a poisoned lock.
                        for sample_out in output.iter_mut() { *sample_out = 0.0; }
                        return;
                    }
                };

                if !*is_playing_guard {
                    for sample_out in output.iter_mut() { *sample_out = 0.0; }
                    return;
                }

                // --- EQ Parameter Update Check & Smoothing (Phase 3 & 6) ---
                let mut current_eq_params_guard = current_eq_params_arc.lock().unwrap();
                let target_eq_params_guard = target_eq_params_arc.lock().unwrap();
                
                current_eq_params_guard.low_gain_db = target_eq_params_guard.low_gain_db * EQ_TRIM_SMOOTHING_FACTOR + current_eq_params_guard.low_gain_db * (1.0 - EQ_TRIM_SMOOTHING_FACTOR);
                current_eq_params_guard.mid_gain_db = target_eq_params_guard.mid_gain_db * EQ_TRIM_SMOOTHING_FACTOR + current_eq_params_guard.mid_gain_db * (1.0 - EQ_TRIM_SMOOTHING_FACTOR);
                current_eq_params_guard.high_gain_db = target_eq_params_guard.high_gain_db * EQ_TRIM_SMOOTHING_FACTOR + current_eq_params_guard.high_gain_db * (1.0 - EQ_TRIM_SMOOTHING_FACTOR);
                
                let mut last_eq_params_guard = last_eq_params_mut.lock().unwrap();
                if !current_eq_params_guard.approx_eq(&*last_eq_params_guard) { 
                    let mut low_filter = low_shelf_filter_mut.lock().unwrap();
                    let mut mid_filter = mid_peak_filter_mut.lock().unwrap();
                    let mut high_filter = high_shelf_filter_mut.lock().unwrap();

                    match effects::calculate_low_shelf(track_sample_rate_for_eq, current_eq_params_guard.low_gain_db) {
                        Ok(coeffs) => low_filter.update_coefficients(coeffs),
                        Err(e) => log::error!("Deck {}: Failed to update low_shelf_filter: {}", deck_id_clone_for_callback, e),
                    }
                    match effects::calculate_mid_peak(track_sample_rate_for_eq, current_eq_params_guard.mid_gain_db) {
                        Ok(coeffs) => mid_filter.update_coefficients(coeffs),
                        Err(e) => log::error!("Deck {}: Failed to update mid_peak_filter: {}", deck_id_clone_for_callback, e),
                    }
                    match effects::calculate_high_shelf(track_sample_rate_for_eq, current_eq_params_guard.high_gain_db) {
                        Ok(coeffs) => high_filter.update_coefficients(coeffs),
                        Err(e) => log::error!("Deck {}: Failed to update high_shelf_filter: {}", deck_id_clone_for_callback, e),
                    }
                    *last_eq_params_guard = current_eq_params_guard.clone(); 
                }
                drop(target_eq_params_guard); 
                drop(current_eq_params_guard); // Release lock before filter processing guards
                drop(last_eq_params_guard); // Release lock

                let mut low_filter_processing_guard = low_shelf_filter_mut.lock().unwrap();
                let mut mid_filter_processing_guard = mid_peak_filter_mut.lock().unwrap();
                let mut high_filter_processing_guard = high_shelf_filter_mut.lock().unwrap();

                let mut smoothed_pitch_val = *current_pitch_rate_arc_cb.lock().unwrap(); 
                let target_pitch_val = *target_pitch_rate_arc_cb.lock().unwrap(); 
                smoothed_pitch_val = target_pitch_val * PITCH_SMOOTHING_FACTOR + smoothed_pitch_val * (1.0 - PITCH_SMOOTHING_FACTOR);
                *current_pitch_rate_arc_cb.lock().unwrap() = smoothed_pitch_val; 

                let mut current_read_head_guard = current_sample_read_head_arc.lock().unwrap();
                let source_samples_guard = samples_arc.as_ref();
                let active_pitch_for_callback = smoothed_pitch_val; 
                
                let mut current_trim_gain_val = *current_trim_gain_arc.lock().unwrap();
                let target_trim_gain_val = *target_trim_gain_arc.lock().unwrap();
                current_trim_gain_val = target_trim_gain_val * EQ_TRIM_SMOOTHING_FACTOR + current_trim_gain_val * (1.0 - EQ_TRIM_SMOOTHING_FACTOR);
                *current_trim_gain_arc.lock().unwrap() = current_trim_gain_val;

                let channel_fader_level_val = *channel_fader_level_arc.lock().unwrap(); // Get fader level for this buffer

                let mut seek_fade_gain = 1.0f32;
                match seek_fade_state_arc.lock() {
                    Ok(mut fade_state_guard) => {
                        if let Some(progress_ref_mut) = fade_state_guard.as_mut() {
                            log::trace!("[Callback {}] Seek fade active. Progress: {:.2}", deck_id_clone_for_callback, *progress_ref_mut);
                            seek_fade_gain = *progress_ref_mut;
                            *progress_ref_mut += SEEK_FADE_INCREMENT_PER_BUFFER;
                            if *progress_ref_mut >= 1.0 {
                                *fade_state_guard = None; // Clear the Option<f32>
                                log::debug!("[Callback {}] Seek fade complete.", deck_id_clone_for_callback);
                            }
                        }
                    },
                    Err(poisoned) => {
                        log::error!("[Callback {}] Seek fade state Mutex poisoned: {}. Setting fade gain to 1.0 to avoid silence.", deck_id_clone_for_callback, poisoned);
                        seek_fade_gain = 1.0; // Default to full volume if lock fails
                    }
                }

                for frame_out in output.chunks_mut(stream_output_channels as usize) {
                    let read_head_floor = current_read_head_guard.floor();
                    let idx_floor = read_head_floor as usize;

                    if idx_floor >= track_total_samples.saturating_sub(1) {
                        if *is_playing_guard { 
                            *is_playing_guard = false;
                            log::info!("Audio Thread Callback: Track ended for deck '{}' (read_head {:.2})", deck_id_clone_for_callback, *current_read_head_guard);
                        }
                        for sample_out in frame_out.iter_mut() { *sample_out = 0.0; }
                        continue; 
                    }

                    let idx_ceil = (read_head_floor + 1.0) as usize;
                    let safe_idx_ceil = idx_ceil.min(track_total_samples.saturating_sub(1)); 

                    let sample1 = source_samples_guard[idx_floor];
                    let sample2 = source_samples_guard[safe_idx_ceil];
                    
                    let fraction = current_read_head_guard.fract();
                    let mut interpolated_sample = sample1 + (sample2 - sample1) * fraction as f32;

                    // --- Apply Trim Gain and EQ (Phase 3 & 6) ---
                    interpolated_sample *= current_trim_gain_val; // Use smoothed value
                    interpolated_sample *= channel_fader_level_val; // Apply channel fader level

                    // Use the guards acquired before the loop
                    interpolated_sample = low_filter_processing_guard.run(interpolated_sample);
                    interpolated_sample = mid_filter_processing_guard.run(interpolated_sample);
                    interpolated_sample = high_filter_processing_guard.run(interpolated_sample);
                    // --- End Apply Trim Gain and EQ ---

                    interpolated_sample *= seek_fade_gain; // Apply seek fade gain

                    for i in 0..stream_output_channels as usize {
                        frame_out[i] = interpolated_sample; 
                    }
                    
                    // *current_read_head_guard is advanced using active_pitch_for_callback
                    *current_read_head_guard += active_pitch_for_callback as f64;
                }
            };
            
            let err_callback_app_handle = app_handle.clone();
            let err_callback_deck_id = deck_id.clone();
            let error_callback = move |err: cpal::StreamError| {
                log::error!("CPAL stream error for deck '{}': {}", err_callback_deck_id, err);
                emit_error_event(&err_callback_app_handle, &err_callback_deck_id, &format!("Audio stream error: {}", err));
            };

            let stream = match cpal_device.build_output_stream(
                &stream_config,
                data_callback,
                error_callback,
                None, // Timeout
            ) {
                Ok(s) => s,
                Err(e) => {
                    let err = PlaybackError::CpalBuildStreamError(e);
                    log::error!("Audio Thread: LoadTrack: Failed to build CPAL stream for deck '{}': {:?}", deck_id, err);
                    emit_error_event(app_handle, &deck_id, &err.to_string());
                    return Ok(());
                }
            };
            
            // Stream is paused by default after creation.
            // deck_state is already mutably borrowed from earlier.
            deck_state.cpal_stream = Some(stream);
            deck_state.sample_rate = rate; // Store actual sample rate of the decoded audio
            deck_state.output_sample_rate = Some(stream_config.sample_rate.0); // Store output sample rate (Phase 5)
            deck_state.duration = duration_val;
            deck_state.cue_point = None; // Reset cue point on new load
            deck_state.original_bpm = original_bpm;
            deck_state.first_beat_sec = first_beat_sec;
            
            // Reset playback state for the new track
            *deck_state.is_playing.lock().unwrap() = false;
            *deck_state.current_sample_read_head.lock().unwrap() = 0.0; // Reset to 0.0
            *deck_state.paused_position_read_head.lock().unwrap() = Some(0.0); // Reset to Some(0.0)
            
            // Reset pitch/sync related fields for new track
            *deck_state.current_pitch_rate.lock().unwrap() = 1.0;
            deck_state.manual_pitch_rate = 1.0;
            deck_state.last_ui_pitch_rate = Some(1.0);
            deck_state.is_sync_active = false;
            deck_state.is_master = false;
            deck_state.master_deck_id = None;
            deck_state.target_pitch_rate_for_bpm_match = 1.0;
            deck_state.pll_integral_error = 0.0; // Reset PLL integral error on new track load

            log::info!("Audio Thread: Track '{}' loaded and CPAL stream built for deck '{}' with config: {:?}, {} channels, {} Hz", path, deck_id, chosen_supported_config_range.sample_format(), cpal_channels, cpal_sample_rate.0);
            emit_load_update_event(app_handle, &deck_id, duration_val.as_secs_f64(), None, original_bpm, first_beat_sec);
            emit_status_update_event(app_handle, &deck_id, false);
            emit_pitch_tick_event(app_handle, &deck_id, 1.0);
            Ok(())
        }
        Ok(Err(e_decode)) => {
            let err = PlaybackError::PlaybackDecodeError { deck_id: decode_deck_id, source: e_decode };
            log::error!("Audio Thread: Decode failed for path '{}': {:?}", path, err);
            emit_error_event(&decode_app_handle, &deck_id, &err.to_string());
            Ok(())
        }
        Err(join_error) => {
            log::error!("Audio Thread: Decode task panicked for deck '{}': {}", decode_deck_id, join_error);
            let error_msg = format!("Audio decoding task failed: {}", join_error);
            emit_error_event(&decode_app_handle, &deck_id, &error_msg);
            Ok(())
        }
    }
}

pub(crate) fn audio_thread_handle_play<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let state = local_states.get_mut(deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })?;
    if state.cpal_stream.is_none() {
        log::warn!("Audio Thread: Play ignored for deck '{}', no CPAL stream (track not loaded?).", deck_id);
        emit_error_event(app_handle, deck_id, "Cannot play: Track not loaded.");
        return Ok(());
    }
    if state.decoded_samples.is_empty() {
        log::warn!("Audio Thread: Play ignored for deck '{}', decoded samples are empty.", deck_id);
        emit_error_event(app_handle, deck_id, "Cannot play: Track data is empty.");
        return Ok(());
    }
    state.cpal_stream.as_ref().unwrap().play().map_err(PlaybackError::CpalPlayStreamError)?;
    {
        let mut playing_guard = state.is_playing.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing for deck '{}'.", deck_id)))?;
        *playing_guard = true;
    }
    *state.last_playback_instant.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock last_playback_instant for deck '{}'.", deck_id)))? = None;
    *state.read_head_at_last_playback_instant.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock read_head_at_last_playback_instant for deck '{}'.", deck_id)))? = None;
    log::info!("Audio Thread: Playing deck '{}' via CPAL", deck_id);
    emit_status_update_event(app_handle, deck_id, true);
    Ok(())
}

pub(crate) fn audio_thread_handle_pause<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let state = local_states.get_mut(deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })?;
    {
        let mut playing_guard = state.is_playing.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing for deck '{}'.", deck_id)))?;
        *playing_guard = false;
    }
    if state.cpal_stream.is_none() {
        log::warn!("Audio Thread: Pause ignored for deck '{}', no CPAL stream.", deck_id);
        emit_status_update_event(app_handle, deck_id, false);
        return Ok(());
    }
    state.cpal_stream.as_ref().unwrap().pause().map_err(PlaybackError::CpalPauseStreamError)?;
    let current_idx = *state.current_sample_read_head.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock current_sample_read_head for deck '{}'.", deck_id)))?;
    *state.paused_position_read_head.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock paused_position_read_head for deck '{}'.", deck_id)))? = Some(current_idx);
    log::info!("Audio Thread: Paused deck '{}' via CPAL at sample {}", deck_id, current_idx);
    emit_status_update_event(app_handle, deck_id, false);

    // --- NEW: Disengage sync for both decks if either is master or synced ---
    let any_deck_synced_or_master = local_states.values().any(|s| s.is_sync_active || s.is_master);
    if any_deck_synced_or_master {
        let deck_ids: Vec<String> = local_states.keys().cloned().collect();
        for id in deck_ids {
            // Ignore errors here to ensure all decks are processed
            let _ = crate::audio::playback::sync::audio_thread_handle_disable_sync(&id, local_states, app_handle);
        }
    }
    // --- END NEW ---

    Ok(())
}

pub(crate) fn audio_thread_handle_seek<R: Runtime>(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let state = local_states.get_mut(deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })?;
    if state.decoded_samples.is_empty() || state.sample_rate == 0.0 {
        log::warn!("Audio Thread: Seek ignored for deck '{}', no track loaded or invalid sample rate.", deck_id);
        return Ok(());
    }
    let total_samples = state.decoded_samples.len();
    let target_sample_float = position_seconds * state.sample_rate as f64;
    let mut target_sample_index = target_sample_float.round() as usize;
    if target_sample_index >= total_samples {
        log::warn!(
            "Audio Thread: Seek position {:.2}s (sample {}) beyond duration for deck '{}'. Clamping to end.",
            position_seconds, target_sample_index, deck_id
        );
        target_sample_index = total_samples.saturating_sub(1);
    } else {
        target_sample_index = target_sample_index.max(0);
    }
    *state.current_sample_read_head.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock current_sample_read_head for deck '{}'.", deck_id)))? = target_sample_index as f64;
    *state.seek_fade_state.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock seek_fade_state for deck '{}'.", deck_id)))? = Some(0.0);
    if !*state.is_playing.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing for deck '{}'.", deck_id)))? {
        *state.paused_position_read_head.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock paused_position_read_head for deck '{}'.", deck_id)))? = Some(target_sample_index as f64);
    }
    let current_time_secs = target_sample_index as f64 / state.sample_rate as f64;
    emit_tick_event(app_handle, deck_id, current_time_secs);
    Ok(())
}

pub(crate) fn audio_thread_handle_set_fader_level(
    deck_id: &str,
    level: f32,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) -> Result<(), PlaybackError> {
    let state = local_states.get_mut(deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })?;
    let clamped_level = level.clamp(0.0, 1.0);
    *state.channel_fader_level.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock channel_fader_level for deck '{}'.", deck_id)))? = clamped_level;
    log::debug!(
        "Audio Thread: Set channel_fader_level for deck '{}' to {}",
        deck_id, clamped_level
    );
    Ok(())
}

pub(crate) fn audio_thread_handle_set_trim_gain(
    deck_id: &str,
    gain: f32,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) -> Result<(), PlaybackError> {
    let state = local_states.get_mut(deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })?;
    *state.target_trim_gain.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock target_trim_gain for deck '{}'.", deck_id)))? = gain;
    log::debug!(
        "Audio Thread: Set target_trim_gain (linear) for deck '{}' to {}",
        deck_id, gain
    );
    Ok(())
}

pub(crate) fn audio_thread_handle_set_eq(
    deck_id: &str,
    new_params: EqParams,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) -> Result<(), PlaybackError> {
    let state = local_states.get_mut(deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })?;
    *state.target_eq_params.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock target_eq_params for deck '{}'.", deck_id)))? = new_params;
    log::debug!("Audio Thread: Updated target_eq_params for deck '{}'", deck_id);
    Ok(())
}

pub(crate) fn audio_thread_handle_set_cue<R: Runtime>(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    _app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let state = local_states.get_mut(deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })?;
    if state.duration == Duration::ZERO {
        log::warn!("Audio Thread: SetCue ignored for deck '{}', track duration is zero (not loaded?).", deck_id);
        return Ok(());
    }
    let cue_duration = Duration::from_secs_f64(position_seconds.max(0.0).min(state.duration.as_secs_f64()));
    state.cue_point = Some(cue_duration);
    log::info!(
        "Audio Thread: Set cue point for deck '{}' to {:.2}s",
        deck_id, cue_duration.as_secs_f64()
    );
    Ok(())
}

pub(crate) fn audio_thread_handle_cleanup(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) -> Result<(), PlaybackError> {
    if let Some(mut removed_state) = local_states.remove(deck_id) {
        if let Some(stream) = removed_state.cpal_stream.take() {
            drop(stream);
        }
        log::info!("Audio Thread: Cleaned up deck '{}' (CPAL stream dropped if existed).", deck_id);
        Ok(())
    } else {
        log::warn!("Audio Thread: CleanupDeck: Deck '{}' not found for cleanup.", deck_id);
        Err(PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })
    }
}

pub(crate) fn audio_thread_handle_set_pitch_rate<R: Runtime>(
    deck_id: &str,
    rate: f32,
    is_user_initiated_change: bool,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let mut master_new_target_pitch_for_slaves: Option<f32> = None;
    let state = local_states.get_mut(deck_id).ok_or_else(|| PlaybackError::DeckNotFound { deck_id: deck_id.to_string() })?;
    let clamped_new_target_rate = rate.clamp(0.5, 2.0);
    let old_current_pitch_rate = *state.current_pitch_rate.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock current_pitch_rate for deck '{}'.", deck_id)))?;
    if is_user_initiated_change {
        state.manual_pitch_rate = clamped_new_target_rate;
        if state.is_master {
            master_new_target_pitch_for_slaves = Some(clamped_new_target_rate);
        }
    }
    *state.target_pitch_rate.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock target_pitch_rate for deck '{}'.", deck_id)))? = clamped_new_target_rate;
    if !is_user_initiated_change {
        *state.current_pitch_rate.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock current_pitch_rate for deck '{}'.", deck_id)))? = clamped_new_target_rate;
        if (clamped_new_target_rate - old_current_pitch_rate).abs() > 1e-5 {
            *state.last_playback_instant.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock last_playback_instant for deck '{}'.", deck_id)))? = None;
            *state.read_head_at_last_playback_instant.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock read_head_at_last_playback_instant for deck '{}'.", deck_id)))? = None;
            log::info!(
                "Audio Thread: Invalidated precise timing for deck '{}' due to system pitch change from {:.4} to {:.4}.",
                deck_id, old_current_pitch_rate, clamped_new_target_rate
            );
        }
        log::info!(
            "Audio Thread: Snapped current_pitch_rate for deck '{}' to {} (System-initiated change for sync/tempo).",
            deck_id, clamped_new_target_rate
        );
        emit_pitch_tick_event(app_handle, deck_id, clamped_new_target_rate);
        state.last_ui_pitch_rate = Some(clamped_new_target_rate);
        log::info!(
            "Audio Thread: Set target_pitch_rate and SNAPPED current_pitch_rate for deck '{}' to {} (System change).",
            deck_id, clamped_new_target_rate
        );
    } else {
        emit_pitch_tick_event(app_handle, deck_id, clamped_new_target_rate);
        state.last_ui_pitch_rate = Some(clamped_new_target_rate);
        log::info!(
            "Audio Thread: Set target_pitch_rate for deck '{}' to {} (User initiated: {}). Smoothing will occur in callback.",
            deck_id, clamped_new_target_rate, is_user_initiated_change
        );
    }
    if let Some(master_new_target_pitch) = master_new_target_pitch_for_slaves {
        let master_deck_id_str = deck_id.to_string();
        let master_original_bpm = local_states.get(deck_id).and_then(|s| s.original_bpm);
        if let Some(master_bpm) = master_original_bpm {
            let mut slave_updates: Vec<(String, f32)> = Vec::new();
            for (id, state) in local_states.iter() {
                if state.is_sync_active && state.master_deck_id.as_deref() == Some(&master_deck_id_str) {
                    if let Some(slave_bpm) = state.original_bpm {
                        if slave_bpm.abs() > 1e-6 {
                            let new_target_rate_for_slave = (master_bpm / slave_bpm) * master_new_target_pitch;
                            slave_updates.push((id.clone(), new_target_rate_for_slave));
                        }
                    }
                }
            }
            for (slave_id_str, new_target_rate_for_slave) in slave_updates {
                if let Some(slave_state) = local_states.get_mut(&slave_id_str) {
                    slave_state.target_pitch_rate_for_bpm_match = new_target_rate_for_slave;
                    *slave_state.target_pitch_rate.lock().map_err(|_| PlaybackError::LogicalStateLockError(format!("Failed to lock target_pitch_rate for deck '{}'.", slave_id_str)))? = new_target_rate_for_slave.clamp(0.5, 2.0);
                    log::info!("Audio Thread: Master '{}' target pitch change, slave '{}' new target_pitch_rate: {:.4}", master_deck_id_str, slave_id_str, new_target_rate_for_slave);
                    emit_pitch_tick_event(app_handle, &slave_id_str, new_target_rate_for_slave.clamp(0.5, 2.0));
                    slave_state.last_ui_pitch_rate = Some(new_target_rate_for_slave.clamp(0.5, 2.0));
                } else {
                    log::warn!("Audio Thread: Slave '{}' not found during master pitch update propagation.", slave_id_str);
                }
            }
        } else {
            log::warn!("Audio Thread: Master '{}' missing BPM, cannot update slave target pitches.", deck_id);
        }
    }
    Ok(())
} 