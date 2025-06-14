use super::*;

pub(crate) async fn audio_thread_handle_load<R: Runtime>(
    deck_id: String,
    path: String,
    original_bpm: Option<f32>,
    first_beat_sec: Option<f32>,
    output_device_name: Option<String>,
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
            log::info!(
                "Audio Thread: Dropped existing CPAL stream for deck '{}' before loading new track.",
                deck_id
            );
        }
    }
    let path_clone = path.clone();
    let decode_app_handle = app_handle.clone();
    let decode_deck_id = deck_id.clone();
    let decode_result =
        tokio::task::spawn_blocking(move || decoding::decode_file_to_mono_samples(&path_clone))
            .await;
    match decode_result {
        Ok(Ok((samples, rate))) => {
            let duration_val = Duration::from_secs_f64(samples.len() as f64 / rate as f64);
            log::info!(
                "Audio Thread: Decoded '{}'. Duration: {:?}, Rate: {}, Samples: {}",
                path,
                duration_val,
                rate,
                samples.len()
            );
            
            // Find the appropriate CPAL device for output
            let actual_cpal_device = if let Some(ref device_name) = output_device_name {
                log::info!("Audio Thread: Looking for selected device '{}' for deck '{}'", device_name, deck_id);
                match crate::audio::devices::find_cpal_output_device(Some(device_name)) {
                    Ok(Some(device)) => {
                        log::info!("Audio Thread: Using selected device '{}' for deck '{}'", device_name, deck_id);
                        device
                    },
                    Ok(None) => {
                        log::warn!("Audio Thread: Selected device '{}' not found for deck '{}', using default", device_name, deck_id);
                        cpal_device.clone()
                    },
                    Err(e) => {
                        log::error!("Audio Thread: Error finding device '{}' for deck '{}': {}. Using default.", device_name, deck_id, e);
                        cpal_device.clone()
                    }
                }
            } else {
                log::info!("Audio Thread: No device selected for deck '{}', using default", deck_id);
                cpal_device.clone()
            };
            let supported_configs = match actual_cpal_device.supported_output_configs() {
                Ok(configs) => configs.collect::<Vec<_>>(),
                Err(e) => {
                    log::warn!(
                        "Audio Thread: LoadTrack: Could not get supported configs for deck '{}', using default: {}",
                        deck_id, e
                    );
                    vec![]
                }
            };
            let target_track_sample_rate = rate as u32;
            let mut best_config: Option<SupportedStreamConfigRange> = None;
            
            for config_range in supported_configs.iter() {
                if config_range.sample_format() == cpal::SampleFormat::F32 {
                    if config_range.min_sample_rate().0 <= target_track_sample_rate
                        && config_range.max_sample_rate().0 >= target_track_sample_rate
                    {
                        if config_range.channels() == 2 {
                            best_config = Some(config_range.clone());
                            break;
                        }
                        if best_config.is_none()
                            || best_config
                                .as_ref()
                                .map(|c| c.channels() != 2)
                                .unwrap_or(false)
                        {
                            best_config = Some(config_range.clone());
                        }
                    }
                }
            }
            
            if best_config.is_none() {
                for target_sr in [48000, 44100].iter() {
                    for config_range in supported_configs.iter() {
                        if config_range.sample_format() == cpal::SampleFormat::F32 {
                            if config_range.min_sample_rate().0 <= *target_sr
                                && config_range.max_sample_rate().0 >= *target_sr
                            {
                                if config_range.channels() == 2 {
                                    best_config = Some(config_range.clone());
                                    break;
                                }
                                if best_config.is_none()
                                    || best_config
                                        .as_ref()
                                        .map(|c| c.channels() != 2)
                                        .unwrap_or(false)
                                {
                                    best_config = Some(config_range.clone());
                                }
                            }
                        }
                    }
                    if best_config.is_some()
                        && best_config
                            .as_ref()
                            .map(|c| {
                                c.channels() == 2
                                    && c.min_sample_rate().0 <= *target_sr
                                    && c.max_sample_rate().0 >= *target_sr
                            })
                            .unwrap_or(false)
                    {
                        break;
                    }
                }
            }
            
            if best_config.is_none() {
                let mut f32_configs: Vec<SupportedStreamConfigRange> = supported_configs
                    .iter()
                    .filter(|c| c.sample_format() == cpal::SampleFormat::F32)
                    .cloned()
                    .collect();
                if !f32_configs.is_empty() {
                    f32_configs.sort_by(|a, b| {
                        b.channels()
                            .cmp(&a.channels())
                            .then_with(|| b.max_sample_rate().cmp(&a.max_sample_rate()))
                    });
                    best_config = Some(f32_configs[0].clone());
                }
            }
            
            let chosen_supported_config_range = match best_config {
                Some(conf) => conf,
                None => {
                    match actual_cpal_device.default_output_config() {
                        Ok(default_config) => {
                            log::warn!(
                                "Audio Thread: Using default output config as fallback for deck '{}': {:?}",
                                deck_id, default_config
                            );
                            cpal::SupportedStreamConfigRange::new(
                                default_config.channels(),
                                default_config.sample_rate(),
                                default_config.sample_rate(),
                                default_config.buffer_size().clone(),
                                default_config.sample_format(),
                            )
                        }
                        Err(default_err) => {
                            log::error!(
                                "Audio Thread: LoadTrack: No audio configuration available for deck '{}': {:?}",
                                deck_id, default_err
                            );
                            emit_error_event(
                                app_handle,
                                &deck_id,
                                "No audio output configuration available.",
                            );
                            return Ok(());
                        }
                    }
                }
            };
            
            let cpal_sample_rate_val = if chosen_supported_config_range.min_sample_rate().0
                <= target_track_sample_rate
                && chosen_supported_config_range.max_sample_rate().0 >= target_track_sample_rate
            {
                target_track_sample_rate
            } else if chosen_supported_config_range.min_sample_rate().0 <= 48000
                && chosen_supported_config_range.max_sample_rate().0 >= 48000
            {
                48000
            } else if chosen_supported_config_range.min_sample_rate().0 <= 44100
                && chosen_supported_config_range.max_sample_rate().0 >= 44100
            {
                44100
            } else {
                chosen_supported_config_range.max_sample_rate().0
            };
            
            let cpal_sample_rate = cpal::SampleRate(cpal_sample_rate_val);
            let cpal_channels = chosen_supported_config_range.channels();
            let sample_rate_ratio = cpal_sample_rate.0 as f32 / rate;
            
            if (sample_rate_ratio - 1.0).abs() > 0.01 {
                log::warn!(
                    "Audio Thread: Sample rate mismatch for deck '{}'. Track: {} Hz, CPAL Stream: {} Hz (ratio: {:.3}). Playback speed will be adjusted.",
                    deck_id, rate, cpal_sample_rate.0, sample_rate_ratio
                );
            } else {
                log::info!(
                    "Audio Thread: Matched sample rate for deck '{}'. Track: {} Hz, CPAL Stream: {} Hz.",
                    deck_id, rate, cpal_sample_rate.0
                );
            }
            
            let stream_config = StreamConfig {
                channels: cpal_channels,
                sample_rate: cpal_sample_rate,
                buffer_size: cpal::BufferSize::Default,
            };
            
            let samples_arc = std::sync::Arc::new(samples);
            let deck_state =
                local_states
                    .get_mut(&deck_id)
                    .ok_or_else(|| PlaybackError::DeckNotFound {
                        deck_id: deck_id.clone(),
                    })?;
            deck_state.decoded_samples = samples_arc.clone();

            let current_sample_read_head_arc = deck_state.current_sample_read_head.clone();
            let is_playing_arc = deck_state.is_playing.clone();
            let app_handle_clone_for_callback = app_handle.clone();
            let deck_id_clone_for_callback = deck_id.clone();
            let track_total_samples = samples_arc.len();
            let stream_output_channels = cpal_channels;

            let last_eq_params_mut = deck_state.last_eq_params.clone();
            let low_shelf_filter_mut = deck_state.low_shelf_filter.clone();
            let mid_peak_filter_mut = deck_state.mid_peak_filter.clone();
            let high_shelf_filter_mut = deck_state.high_shelf_filter.clone();
            let track_sample_rate_for_eq = rate;
            
            let cached_low_coeffs_mut = deck_state.cached_low_coeffs.clone();
            let cached_mid_coeffs_mut = deck_state.cached_mid_coeffs.clone();
            let cached_high_coeffs_mut = deck_state.cached_high_coeffs.clone();

            let last_playback_instant_arc = deck_state.last_playback_instant.clone();
            let read_head_at_last_playback_instant_arc =
                deck_state.read_head_at_last_playback_instant.clone();

            let current_eq_params_arc = deck_state.current_eq_params.clone();
            let target_eq_params_arc = deck_state.target_eq_params.clone();
            let current_trim_gain_arc = deck_state.current_trim_gain.clone();
            let target_trim_gain_arc = deck_state.target_trim_gain.clone();
            const AUDIO_PARAM_SMOOTHING_FACTOR: f32 = EQ_SMOOTHING_FACTOR;

            let current_pitch_rate_arc_cb = deck_state.current_pitch_rate.clone();
            let target_pitch_rate_arc_cb = deck_state.target_pitch_rate.clone();

            let seek_fade_state_arc = deck_state.seek_fade_state.clone();
            const SEEK_FADE_INCREMENT_PER_BUFFER: f32 = 0.08;
            let channel_fader_level_arc = deck_state.channel_fader_level.clone();
            
            let last_emit_frame_arc = deck_state.last_emit_frame.clone();

            let inv_smoothing_factor = 1.0 - AUDIO_PARAM_SMOOTHING_FACTOR;
            let sample_rate_adjustment = rate / cpal_sample_rate.0 as f32;
            let track_sample_rate_f64 = rate as f64;
            
            let inv_track_sample_rate_f64 = 1.0 / track_sample_rate_f64;
            let sample_rate_adjustment_f64 = sample_rate_adjustment as f64;
            
            let buffer_frame_counter = Arc::new(AtomicU64::new(0u64));

            let data_callback = move |output: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                let frames_in_buffer = output.len() / stream_output_channels as usize;
                let buffer_start_frame = buffer_frame_counter.fetch_add(frames_in_buffer as u64, Ordering::Relaxed);
                log::trace!(
                    "[Callback {}] Entered data_callback.",
                    deck_id_clone_for_callback
                );

                let now_for_timing = std::time::Instant::now();
                let read_head_before_advancing_for_this_buffer =
                    current_sample_read_head_arc.load(Ordering::Relaxed);
                *last_playback_instant_arc.lock().unwrap() = Some(now_for_timing);
                *read_head_at_last_playback_instant_arc.lock().unwrap() =
                    Some(read_head_before_advancing_for_this_buffer);

                let is_playing = is_playing_arc.load(Ordering::Relaxed);

                // Always emit timing events for UI updates, regardless of playing state
                let current_read_head_for_timing = current_sample_read_head_arc.load(Ordering::Relaxed);
                let actual_time_secs = current_read_head_for_timing * inv_track_sample_rate_f64;
                
                let emit_interval_frames = (track_sample_rate_f64 * (1.0 / 120.0)) as u64;
                let last_emit_frame = last_emit_frame_arc.load(Ordering::Relaxed);
                if buffer_start_frame >= last_emit_frame + emit_interval_frames {
                    last_emit_frame_arc.store(buffer_start_frame, Ordering::Relaxed);
                    use crate::audio::playback::events::emit_tick_event;
                    emit_tick_event(&app_handle_clone_for_callback, &deck_id_clone_for_callback, actual_time_secs);
                }

                if !is_playing {
                    for sample_out in output.iter_mut() {
                        *sample_out = 0.0;
                    }
                    return;
                }

                let mut current_eq_params_guard = current_eq_params_arc.lock().unwrap();
                let target_eq_params_guard = target_eq_params_arc.lock().unwrap();

                current_eq_params_guard.low_gain_db = target_eq_params_guard.low_gain_db
                    * AUDIO_PARAM_SMOOTHING_FACTOR
                    + current_eq_params_guard.low_gain_db * inv_smoothing_factor;
                current_eq_params_guard.mid_gain_db = target_eq_params_guard.mid_gain_db
                    * AUDIO_PARAM_SMOOTHING_FACTOR
                    + current_eq_params_guard.mid_gain_db * inv_smoothing_factor;
                current_eq_params_guard.high_gain_db = target_eq_params_guard.high_gain_db
                    * AUDIO_PARAM_SMOOTHING_FACTOR
                    + current_eq_params_guard.high_gain_db * inv_smoothing_factor;

                let mut last_eq_params_guard = last_eq_params_mut.lock().unwrap();
                
                let low_diff = (current_eq_params_guard.low_gain_db - last_eq_params_guard.low_gain_db).abs();
                let mid_diff = (current_eq_params_guard.mid_gain_db - last_eq_params_guard.mid_gain_db).abs();
                let high_diff = (current_eq_params_guard.high_gain_db - last_eq_params_guard.high_gain_db).abs();
                
                if low_diff > EQ_RECALC_THRESHOLD_DB || mid_diff > EQ_RECALC_THRESHOLD_DB || high_diff > EQ_RECALC_THRESHOLD_DB {
                    let mut low_filter = low_shelf_filter_mut.lock().unwrap();
                    let mut mid_filter = mid_peak_filter_mut.lock().unwrap();
                    let mut high_filter = high_shelf_filter_mut.lock().unwrap();
                    
                    let mut low_cached = cached_low_coeffs_mut.lock().unwrap();
                    let mut mid_cached = cached_mid_coeffs_mut.lock().unwrap();
                    let mut high_cached = cached_high_coeffs_mut.lock().unwrap();

                    if low_diff > EQ_RECALC_THRESHOLD_DB {
                        match effects::calculate_low_shelf(
                            track_sample_rate_for_eq,
                            current_eq_params_guard.low_gain_db,
                        ) {
                            Ok(coeffs) => {
                                low_filter.update_coefficients(coeffs);
                                *low_cached = Some(coeffs);
                            },
                            Err(e) => log::error!(
                                "Deck {}: Failed to update low_shelf_filter: {}",
                                deck_id_clone_for_callback,
                                e
                            ),
                        }
                    }
                    
                    if mid_diff > EQ_RECALC_THRESHOLD_DB {
                        match effects::calculate_mid_peak(
                            track_sample_rate_for_eq,
                            current_eq_params_guard.mid_gain_db,
                        ) {
                            Ok(coeffs) => {
                                mid_filter.update_coefficients(coeffs);
                                *mid_cached = Some(coeffs);
                            },
                            Err(e) => log::error!(
                                "Deck {}: Failed to update mid_peak_filter: {}",
                                deck_id_clone_for_callback,
                                e
                            ),
                        }
                    }
                    
                    if high_diff > EQ_RECALC_THRESHOLD_DB {
                        match effects::calculate_high_shelf(
                            track_sample_rate_for_eq,
                            current_eq_params_guard.high_gain_db,
                        ) {
                            Ok(coeffs) => {
                                high_filter.update_coefficients(coeffs);
                                *high_cached = Some(coeffs);
                            },
                            Err(e) => log::error!(
                                "Deck {}: Failed to update high_shelf_filter: {}",
                                deck_id_clone_for_callback,
                                e
                            ),
                        }
                    }
                    
                    *last_eq_params_guard = current_eq_params_guard.clone();
                }
                drop(target_eq_params_guard);
                drop(current_eq_params_guard);
                drop(last_eq_params_guard);

                let mut low_filter_processing_guard = low_shelf_filter_mut.lock().unwrap();
                let mut mid_filter_processing_guard = mid_peak_filter_mut.lock().unwrap();
                let mut high_filter_processing_guard = high_shelf_filter_mut.lock().unwrap();

                let mut smoothed_pitch_val = current_pitch_rate_arc_cb.load(Ordering::Relaxed);
                let target_pitch_val = target_pitch_rate_arc_cb.load(Ordering::Relaxed);
                smoothed_pitch_val = target_pitch_val * AUDIO_PARAM_SMOOTHING_FACTOR
                    + smoothed_pitch_val * inv_smoothing_factor;
                current_pitch_rate_arc_cb.store(smoothed_pitch_val, Ordering::Relaxed);

                let mut current_read_head = current_sample_read_head_arc.load(Ordering::Relaxed);
                let source_samples_guard = samples_arc.as_ref();
                let active_pitch_for_callback = smoothed_pitch_val;

                let mut current_trim_gain_val = current_trim_gain_arc.load(Ordering::Relaxed);
                let target_trim_gain_val = target_trim_gain_arc.load(Ordering::Relaxed);
                current_trim_gain_val = target_trim_gain_val * AUDIO_PARAM_SMOOTHING_FACTOR
                    + current_trim_gain_val * inv_smoothing_factor;
                current_trim_gain_arc.store(current_trim_gain_val, Ordering::Relaxed);

                let channel_fader_level_val = channel_fader_level_arc.load(Ordering::Relaxed);

                let mut seek_fade_gain = 1.0f32;
                match seek_fade_state_arc.lock() {
                    Ok(mut fade_state_guard) => {
                        if let Some(progress_ref_mut) = fade_state_guard.as_mut() {
                            log::trace!(
                                "[Callback {}] Seek fade active. Progress: {:.2}",
                                deck_id_clone_for_callback,
                                *progress_ref_mut
                            );
                            seek_fade_gain = *progress_ref_mut;
                            *progress_ref_mut += SEEK_FADE_INCREMENT_PER_BUFFER;
                            if *progress_ref_mut >= 1.0 {
                                *fade_state_guard = None;
                                log::debug!(
                                    "[Callback {}] Seek fade complete.",
                                    deck_id_clone_for_callback
                                );
                            }
                        }
                    }
                    Err(poisoned) => {
                        log::error!(
                            "[Callback {}] Seek fade state Mutex poisoned: {}. Setting fade gain to 1.0 to avoid silence.",
                            deck_id_clone_for_callback,
                            poisoned
                        );
                        seek_fade_gain = 1.0;
                    }
                }

                for frame_out in output.chunks_mut(stream_output_channels as usize) {
                    let read_head_floor = current_read_head.floor();
                    let idx_floor = read_head_floor as usize;

                    if idx_floor >= track_total_samples.saturating_sub(3) {
                        if is_playing {
                            is_playing_arc.store(false, Ordering::Relaxed);
                            log::info!(
                                "Audio Thread Callback: Track ended for deck '{}' (read_head {:.2})",
                                deck_id_clone_for_callback,
                                current_read_head
                            );
                        }
                        for sample_out in frame_out.iter_mut() {
                            *sample_out = 0.0;
                        }
                        continue;
                    }

                    let fraction = current_read_head.fract() as f32;
                    let mut interpolated_sample =
                        if idx_floor >= 1 && idx_floor + 2 < track_total_samples {
                            let y0 = source_samples_guard[idx_floor - 1];
                            let y1 = source_samples_guard[idx_floor];
                            let y2 = source_samples_guard[idx_floor + 1];
                            let y3 = source_samples_guard[idx_floor + 2];

                            let a = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
                            let b = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
                            let c = -0.5 * y0 + 0.5 * y2;
                            let d = y1;

                            a * fraction * fraction * fraction
                                + b * fraction * fraction
                                + c * fraction
                                + d
                        } else {
                            let sample1 = source_samples_guard[idx_floor];
                            let sample2 =
                                source_samples_guard[(idx_floor + 1).min(track_total_samples - 1)];
                            sample1 + (sample2 - sample1) * fraction
                        };

                    interpolated_sample *= current_trim_gain_val;
                    interpolated_sample *= channel_fader_level_val;

                    interpolated_sample = low_filter_processing_guard.run(interpolated_sample);
                    interpolated_sample = mid_filter_processing_guard.run(interpolated_sample);
                    interpolated_sample = high_filter_processing_guard.run(interpolated_sample);

                    interpolated_sample *= seek_fade_gain;

                    // Check if this deck should send audio to cue output
                    {
                        use crate::audio::playback::handlers::cue_output::{push_cue_sample, should_deck_output_to_cue};
                        
                        if should_deck_output_to_cue(&deck_id_clone_for_callback) {
                            // Minimal sample tracking for debugging
                            #[cfg(debug_assertions)]
                            {
                                use std::sync::atomic::{AtomicU64, Ordering};
                                static CUE_SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);
                                let count = CUE_SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed);
                                if count % 441000 == 0 { // Log every 10 seconds in debug builds only
                                    log::trace!("[Track{}] Cue samples: {}", deck_id_clone_for_callback, count);
                                }
                            }
                            
                            push_cue_sample(interpolated_sample);
                        }
                    }

                    for i in 0..stream_output_channels as usize {
                        frame_out[i] = interpolated_sample;
                    }

                    current_read_head += active_pitch_for_callback as f64 * sample_rate_adjustment_f64;
                }
                
                current_sample_read_head_arc.store(current_read_head, Ordering::Relaxed);
            };

            let err_callback_app_handle = app_handle.clone();
            let err_callback_deck_id = deck_id.clone();
            let error_callback = move |err: cpal::StreamError| {
                log::error!(
                    "CPAL stream error for deck '{}': {}",
                    err_callback_deck_id,
                    err
                );
                emit_error_event(
                    &err_callback_app_handle,
                    &err_callback_deck_id,
                    &format!("Audio stream error: {}", err),
                );
            };

            let stream = match actual_cpal_device.build_output_stream(
                &stream_config,
                data_callback,
                error_callback,
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    let err = PlaybackError::CpalBuildStreamError(e);
                    log::error!(
                        "Audio Thread: LoadTrack: Failed to build CPAL stream for deck '{}': {:?}",
                        deck_id,
                        err
                    );
                    emit_error_event(app_handle, &deck_id, &err.to_string());
                    return Ok(());
                }
            };

            deck_state.cpal_stream = Some(stream);
            deck_state.sample_rate = rate;
            deck_state.output_sample_rate = Some(stream_config.sample_rate.0);
            deck_state.duration = duration_val;
            deck_state.cue_point = None;
            deck_state.original_bpm = original_bpm;
            deck_state.first_beat_sec = first_beat_sec;

            // Update the cue output sample rate for any deck that might use cue
            {
                use crate::audio::playback::handlers::cue_output::set_cue_sample_rate;
                if let Err(e) = set_cue_sample_rate(rate as f64) {
                    log::debug!("Failed to set cue sample rate for deck {}: {}", deck_id, e);
                }
            }

            deck_state.is_playing.store(false, Ordering::Relaxed);
            deck_state.current_sample_read_head.store(0.0, Ordering::Relaxed);
            deck_state.paused_position_read_head.store(0.0, Ordering::Relaxed);

            deck_state.current_pitch_rate.store(1.0, Ordering::Relaxed);
            deck_state.manual_pitch_rate = 1.0;
            deck_state.last_ui_pitch_rate = Some(1.0);
            
            // Reset timing event state for new track
            deck_state.last_emit_frame.store(0, Ordering::Relaxed);
            
            // Always reset sync state for the current deck
            deck_state.is_sync_active = false;
            deck_state.is_master = false;
            deck_state.master_deck_id = None;
            deck_state.target_pitch_rate_for_bpm_match = 1.0;
            deck_state.pll_integral_error = 0.0;

            log::info!(
                "Audio Thread: Track '{}' loaded and CPAL stream built for deck '{}' with config: {:?}, {} channels, {} Hz",
                path,
                deck_id,
                chosen_supported_config_range.sample_format(),
                cpal_channels,
                cpal_sample_rate.0
            );
            emit_load_update_event(
                app_handle,
                &deck_id,
                duration_val.as_secs_f64(),
                None,
                original_bpm,
                first_beat_sec,
            );
            emit_status_update_event(app_handle, &deck_id, false);
            emit_pitch_tick_event(app_handle, &deck_id, 1.0);
            
            // Disable sync for ALL decks when any deck loads a new track
            // This ensures both deck sync buttons reset to normal state
            let all_deck_ids: Vec<String> = local_states.keys().cloned().collect();
            for other_deck_id in all_deck_ids {
                if let Some(other_deck_state) = local_states.get_mut(&other_deck_id) {
                    if other_deck_state.is_sync_active || other_deck_state.is_master {
                        // Use the existing disable sync logic to properly handle master/slave relationships
                        if let Err(e) = super::super::sync::audio_thread_handle_disable_sync(
                            &other_deck_id,
                            local_states,
                            app_handle,
                        ) {
                            log::error!(
                                "Audio Thread: LoadTrack: Failed to disable sync for deck '{}': {:?}",
                                other_deck_id,
                                e
                            );
                        }
                        break; // Only need to call disable_sync once as it handles all related decks
                    }
                }
            }
            
            Ok(())
        }
        Ok(Err(e_decode)) => {
            let err = PlaybackError::PlaybackDecodeError {
                deck_id: decode_deck_id,
                source: e_decode,
            };
            log::error!("Audio Thread: Decode failed for path '{}': {:?}", path, err);
            emit_error_event(&decode_app_handle, &deck_id, &err.to_string());
            Ok(())
        }
        Err(join_error) => {
            log::error!(
                "Audio Thread: Decode task panicked for deck '{}': {}",
                decode_deck_id,
                join_error
            );
            let error_msg = format!("Audio decoding task failed: {}", join_error);
            emit_error_event(&decode_app_handle, &deck_id, &error_msg);
            Ok(())
        }
    }
}