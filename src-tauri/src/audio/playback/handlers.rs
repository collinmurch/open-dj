use std::collections::HashMap;
use std::sync::{Arc, Mutex}; // Mutex for EqParams, trim_gain, current_pitch_rate
use std::time::Duration;

// Remove Rodio specific imports
// use rodio::{buffer::SamplesBuffer, Sink, Source}; 

// Add CPAL imports
use cpal::{Device, Stream, StreamConfig, SupportedStreamConfigRange};
use cpal::traits::{DeviceTrait, StreamTrait};
use tauri::{AppHandle, Runtime};

use crate::audio::config::INITIAL_TRIM_GAIN; // Changed from {self, INITIAL_TRIM_GAIN}
use crate::audio::decoding;
// Remove Rodio effects
// use crate::audio::effects::EqSource; 
use crate::audio::errors::PlaybackError; // Potentially for constructing errors, though emit_error_event takes string
use crate::audio::types::EqParams;     // Used by handlers

use super::state::AudioThreadDeckState;
use super::events::*; // For calling emit_..._event functions
// use super::sync; // Added for calculate_pll_pitch_updates - Commented out
// use super::time::get_current_playback_time_secs; // Added for this function - Commented out

// --- Private Handler Functions for Audio Thread Commands ---

pub(crate) fn audio_thread_handle_init<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>, // Removed audio_handle: &rodio::OutputStreamHandle
) {
    if local_states.contains_key(deck_id) {
        log::warn!(
            "Audio Thread: InitDeck: Deck '{}' already exists. No action taken.",
            deck_id
        );
        emit_error_event(app_handle, deck_id, "Deck already initialized.");
        return;
    }

    // No Sink creation here. Stream is created on load.
    let initial_eq_params = Arc::new(Mutex::new(EqParams::default()));
    let initial_trim_gain = Arc::new(Mutex::new(INITIAL_TRIM_GAIN));
    let initial_pitch_rate = Arc::new(Mutex::new(1.0f32)); // For Phase 1, pitch is 1.0

    let deck_state = AudioThreadDeckState {
        cpal_stream: None, // Stream created on load
        decoded_samples: Arc::new(Vec::new()),
        sample_rate: 0.0,
        current_sample_index: Arc::new(Mutex::new(0)),
        paused_position_samples: Arc::new(Mutex::new(Some(0))), // Start paused at 0
        duration: Duration::ZERO,
        is_playing: Arc::new(Mutex::new(false)),
        eq_params: initial_eq_params,
        trim_gain: initial_trim_gain,
        cue_point: None,
        current_pitch_rate: initial_pitch_rate.clone(), // For Phase 1, effectively 1.0
        last_ui_pitch_rate: Some(1.0),
        original_bpm: None,
        first_beat_sec: None,
        is_sync_active: false,
        is_master: false,
        master_deck_id: None,
        target_pitch_rate_for_bpm_match: 1.0,
        manual_pitch_rate: 1.0,
        pll_integral_error: 0.0,
    };
    local_states.insert(deck_id.to_string(), deck_state);
    log::info!("Audio Thread: Initialized deck '{}' for CPAL", deck_id);

    // Emit initial state events
    emit_load_update_event(app_handle, deck_id, 0.0, None, None, None);
    emit_status_update_event(app_handle, deck_id, false);
    emit_sync_status_update_event(app_handle, deck_id, false, false);
    emit_pitch_tick_event(app_handle, deck_id, 1.0);
}

pub(crate) async fn audio_thread_handle_load<R: Runtime>(
    deck_id: String, // Keep as String for map keys
    path: String,
    original_bpm: Option<f32>,
    first_beat_sec: Option<f32>,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    cpal_device: &Device, // Added CPAL device
    app_handle: &AppHandle<R>,
) {
    // Ensure deck exists (after init)
    let deck_state_exists = local_states.contains_key(&deck_id);
    if !deck_state_exists {
        let err_msg = format!("Deck '{}' not initialized before load.", deck_id);
        log::error!("Audio Thread: LoadTrack: {}", err_msg);
        emit_error_event(app_handle, &deck_id, &err_msg);
        return;
    }
    
    // If a stream already exists for this deck, drop it.
    // Taking it out of local_states temporarily to satisfy borrow checker if needed
    // and ensure its Drop implementation (which stops the stream) is called.
    if let Some(state) = local_states.get_mut(&deck_id) {
        if state.cpal_stream.take().is_some() {
            log::info!("Audio Thread: Dropped existing CPAL stream for deck '{}' before loading new track.", deck_id);
        }
    }


    let path_clone = path.clone(); // For spawn_blocking
    let decode_app_handle = app_handle.clone(); // Clone for spawn_blocking error reporting
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

            // Find a supported CPAL output configuration
            let supported_configs_range = match cpal_device.supported_output_configs() {
                Ok(mut configs) => configs.next(), // Take the first one as an example, ideally iterate
                Err(e) => {
                    let err = PlaybackError::CpalSupportedStreamConfigsError(e);
                    log::error!("Audio Thread: LoadTrack: Could not get supported configs for deck '{}': {:?}", deck_id, err);
                    emit_error_event(app_handle, &deck_id, &err.to_string());
                    return;
                }
            };

            let supported_config: SupportedStreamConfigRange = match supported_configs_range {
                Some(config) => config,
                None => {
                     let err_msg = format!("No supported output stream configurations found for device on deck '{}'", deck_id);
                    log::error!("Audio Thread: LoadTrack: {}", err_msg);
                    emit_error_event(app_handle, &deck_id, &err_msg);
                    return;
                }
            };
            
            // For Phase 1, we try to match the sample rate.
            // If not, we'd need resampling. Here, we'll pick a supported rate and log if it mismatches.
            // CPAL configs often specify a range (min_sample_rate, max_sample_rate).
            let cpal_sample_rate = if rate >= supported_config.min_sample_rate().0 as f32 && rate <= supported_config.max_sample_rate().0 as f32 {
                cpal::SampleRate(rate as u32)
            } else {
                log::warn!("Audio Thread: Track sample rate {} Hz for deck '{}' is outside device's supported range [{}-{}]. Using device default/min.", 
                    rate, deck_id, supported_config.min_sample_rate().0, supported_config.max_sample_rate().0);
                // For now, let's use the minimum. Resampling will be critical in Phase 2/6.
                supported_config.min_sample_rate() 
            };
            if (cpal_sample_rate.0 as f32 - rate).abs() > 1.0 {
                 log::warn!("Audio Thread: Sample rate mismatch for deck '{}'. Track: {} Hz, CPAL Stream: {} Hz. Playback quality may be affected until resampling is implemented.",
                    deck_id, rate, cpal_sample_rate.0);
            }

            let cpal_channels = supported_config.channels(); // Typically 2 for stereo
            let stream_config = StreamConfig {
                channels: cpal_channels,
                sample_rate: cpal_sample_rate,
                buffer_size: cpal::BufferSize::Default, // Added buffer_size
            };

            // Prepare data for the audio callback
            let samples_arc = Arc::new(samples); // samples from decoding
            
            // Must re-fetch deck_state mutably to store the stream
            let deck_state = local_states.get_mut(&deck_id).unwrap(); // Should exist

            // Assign to deck_state *before* samples_arc is moved into the closure
            deck_state.decoded_samples = samples_arc.clone(); 

            let current_sample_index_arc = deck_state.current_sample_index.clone();
            let is_playing_arc = deck_state.is_playing.clone();
            let _app_handle_clone_for_callback = app_handle.clone();
            let deck_id_clone_for_callback = deck_id.clone();
            let track_total_samples = samples_arc.len();


            let data_callback = move |output: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                let mut is_playing_guard = is_playing_arc.lock().unwrap();
                if !*is_playing_guard {
                    for sample_out in output.iter_mut() { *sample_out = 0.0; }
                    return;
                }

                let mut current_idx_guard = current_sample_index_arc.lock().unwrap();
                // samples_arc is captured by move, its .as_ref() is used here
                let source_samples_guard = samples_arc.as_ref(); 

                for frame_out in output.chunks_mut(cpal_channels as usize) {
                    if *current_idx_guard >= track_total_samples {
                        // End of track
                        if *is_playing_guard { // Check again in case it was paused right at the end
                            *is_playing_guard = false;
                            // Signal track end to main audio thread? Could use a channel or atomic flag.
                            // For now, rely on time updates to show it ended.
                            // Emit status update from the main audio thread based on this flag.
                            log::info!("Audio Thread Callback: Track ended for deck '{}'", deck_id_clone_for_callback);
                        }
                        for sample_out in frame_out.iter_mut() { *sample_out = 0.0; }
                        continue; 
                    }

                    let source_sample = source_samples_guard[*current_idx_guard];
                    for i in 0..cpal_channels as usize {
                        frame_out[i] = source_sample; // Mono to Stereo/N-channel
                    }
                    *current_idx_guard += 1;
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
                    return;
                }
            };
            
            // Stream is paused by default after creation.
            // deck_state is already mutably borrowed from earlier.
            deck_state.cpal_stream = Some(stream);
            deck_state.sample_rate = rate; // Store actual sample rate of the decoded audio
            deck_state.duration = duration_val;
            deck_state.cue_point = None; // Reset cue point on new load
            deck_state.original_bpm = original_bpm;
            deck_state.first_beat_sec = first_beat_sec;
            
            // Reset playback state for the new track
            *deck_state.is_playing.lock().unwrap() = false;
            *deck_state.current_sample_index.lock().unwrap() = 0;
            *deck_state.paused_position_samples.lock().unwrap() = Some(0);
            
            // Reset pitch/sync related fields for new track
            *deck_state.current_pitch_rate.lock().unwrap() = 1.0;
            deck_state.manual_pitch_rate = 1.0;
            deck_state.last_ui_pitch_rate = Some(1.0);
            deck_state.is_sync_active = false;
            deck_state.is_master = false;
            deck_state.master_deck_id = None;
            deck_state.target_pitch_rate_for_bpm_match = 1.0;

            log::info!("Audio Thread: Track '{}' loaded and CPAL stream built for deck '{}'", path, deck_id);
            emit_load_update_event(app_handle, &deck_id, duration_val.as_secs_f64(), None, original_bpm, first_beat_sec);
            emit_status_update_event(app_handle, &deck_id, false);
            emit_pitch_tick_event(app_handle, &deck_id, 1.0);

        }
        Ok(Err(e_decode)) => { // Inner error from decode_file_to_mono_samples
            let err = PlaybackError::PlaybackDecodeError { deck_id: decode_deck_id, source: e_decode };
            log::error!("Audio Thread: Decode failed for path '{}': {:?}", path, err);
            emit_error_event(&decode_app_handle, &deck_id, &err.to_string());
        }
        Err(join_error) => { // JoinError from spawn_blocking
            log::error!("Audio Thread: Decode task panicked for deck '{}': {}", decode_deck_id, join_error);
            let error_msg = format!("Audio decoding task failed: {}", join_error);
            emit_error_event(&decode_app_handle, &deck_id, &error_msg);
        }
    }
}


pub(crate) fn audio_thread_handle_play<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        if state.cpal_stream.is_none() {
            log::warn!("Audio Thread: Play ignored for deck '{}', no CPAL stream (track not loaded?).", deck_id);
            emit_error_event(app_handle, deck_id, "Cannot play: Track not loaded.");
            return;
        }
        if state.decoded_samples.is_empty() {
             log::warn!("Audio Thread: Play ignored for deck '{}', decoded samples are empty.", deck_id);
            emit_error_event(app_handle, deck_id, "Cannot play: Track data is empty.");
            return;
        }

        match state.cpal_stream.as_ref().unwrap().play() {
            Ok(_) => {
                *state.is_playing.lock().unwrap() = true;
                // If resuming from a paused state, current_sample_index is already set.
                // If starting from beginning after load, current_sample_index is 0.
                // paused_position_samples is set to None when playing starts to signify it's not paused.
                // *state.paused_position_samples.lock().unwrap() = None; 
                // No, keep paused_position_samples as the last known pause, current_sample_index drives playback

                log::info!("Audio Thread: Playing deck '{}' via CPAL", deck_id);
                emit_status_update_event(app_handle, deck_id, true);
            }
            Err(e) => {
                let err = PlaybackError::CpalPlayStreamError(e);
                log::error!("Audio Thread: Failed to play CPAL stream for deck '{}': {:?}", deck_id, err);
                emit_error_event(app_handle, deck_id, &err.to_string());
                // Attempt to recover by setting is_playing to false.
                *state.is_playing.lock().unwrap() = false;

            }
        }
    } else {
        log::error!("Audio Thread: Play: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for play operation.");
    }
}

pub(crate) fn audio_thread_handle_pause<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        // Set is_playing to false first, so the callback starts producing silence.
        *state.is_playing.lock().unwrap() = false;

        if state.cpal_stream.is_none() {
            log::warn!("Audio Thread: Pause ignored for deck '{}', no CPAL stream.", deck_id);
            // No error event, as it's effectively paused if not loaded.
            emit_status_update_event(app_handle, deck_id, false); // Reflect logical state
            return;
        }

        match state.cpal_stream.as_ref().unwrap().pause() {
            Ok(_) => {
                // Store current position when pausing
                let current_idx = *state.current_sample_index.lock().unwrap();
                *state.paused_position_samples.lock().unwrap() = Some(current_idx);
                
                log::info!("Audio Thread: Paused deck '{}' via CPAL at sample {}", deck_id, current_idx);
                emit_status_update_event(app_handle, deck_id, false);

                // Sync logic for pause (Phase 4/5, placeholder for now)
                let was_master = state.is_master;
                let was_slave = state.is_sync_active;
                if was_master {
                    // TODO: Disable sync for slaves in later phases
                } else if was_slave {
                    // TODO: Disable sync for this slave in later phases
                }
            }
            Err(e) => {
                let err = PlaybackError::CpalPauseStreamError(e);
                log::error!("Audio Thread: Failed to pause CPAL stream for deck '{}': {:?}", deck_id, err);
                emit_error_event(app_handle, deck_id, &err.to_string());
                // is_playing is already false.
            }
        }
    } else {
        log::error!("Audio Thread: Pause: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for pause operation.");
    }
}

pub(crate) fn audio_thread_handle_seek<R: Runtime>(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        if state.decoded_samples.is_empty() || state.sample_rate == 0.0 {
            log::warn!("Audio Thread: Seek ignored for deck '{}', no track loaded or invalid sample rate.", deck_id);
            return;
        }

        let total_samples = state.decoded_samples.len();
        let target_sample_float = position_seconds * state.sample_rate as f64;
        let mut target_sample_index = target_sample_float.round() as usize;

        if target_sample_index >= total_samples {
            log::warn!(
                "Audio Thread: Seek position {:.2}s (sample {}) beyond duration for deck '{}'. Clamping to end.",
                position_seconds, target_sample_index, deck_id
            );
            target_sample_index = total_samples.saturating_sub(1); // Ensure it's a valid index
        } else {
            target_sample_index = target_sample_index.max(0);
        }
        
        log::info!("Audio Thread: Seeking deck '{}' to {:.2}s (sample {})", deck_id, position_seconds, target_sample_index);

        *state.current_sample_index.lock().unwrap() = target_sample_index;

        // If paused, also update the canonical paused position.
        // The callback will read from current_sample_index next time it's unpaused.
        if !*state.is_playing.lock().unwrap() {
            *state.paused_position_samples.lock().unwrap() = Some(target_sample_index);
        }
        
        // Emit tick event to update UI immediately with the new position
        let current_time_secs = target_sample_index as f64 / state.sample_rate as f64;
        emit_tick_event(app_handle, deck_id, current_time_secs);

    } else {
        log::error!("Audio Thread: Seek: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for seek operation.");
    }
}

pub(crate) fn audio_thread_handle_set_fader_level(
    deck_id: &str,
    level: f32,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    // For Phase 1, fader level is not directly applied to CPAL stream volume.
    // This would require per-sample multiplication in the data callback or a volume effect.
    // We can store the value if needed for later phases, or just log for now.
    if local_states.contains_key(deck_id) {
        let clamped_level = level.clamp(0.0, 1.0);
        // Placeholder: In a later phase, this level would be read by the data callback
        // to scale samples. For now, it's a no-op on audio output.
        log::debug!(
            "Audio Thread: Set fader level for deck '{}' to {} (Note: Not applied in CPAL Phase 1)",
            deck_id, clamped_level
        );
    } else {
        log::warn!("Audio Thread: SetFaderLevel: Deck '{}' not found.", deck_id);
    }
}

pub(crate) fn audio_thread_handle_set_trim_gain(
    deck_id: &str,
    gain: f32, // This is linear gain
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        // `gain` is already linear, as per the command struct change.
        // The `set_trim_gain` tauri command converts dB to linear.
        *state.trim_gain.lock().unwrap() = gain;
        log::debug!(
            "Audio Thread: Set trim_gain (linear) for deck '{}' to {} (Note: Not applied in CPAL Phase 1)",
            deck_id, gain
        );
    } else {
        log::warn!("Audio Thread: SetTrimGain: Deck '{}' not found.", deck_id);
    }
}

pub(crate) fn audio_thread_handle_set_eq(
    deck_id: &str,
    new_params: EqParams,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        *state.eq_params.lock().unwrap() = new_params;
        log::debug!("Audio Thread: Updated EQ params for deck '{}' (Note: Not applied in CPAL Phase 1)", deck_id);
    } else {
        log::warn!("Audio Thread: SetEq: Deck '{}' not found.", deck_id);
    }
}

pub(crate) fn audio_thread_handle_set_cue<R: Runtime>(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    _app_handle: &AppHandle<R>, // app_handle not used for emitting cue specific event in this phase
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        if state.duration == Duration::ZERO {
            log::warn!("Audio Thread: SetCue ignored for deck '{}', track duration is zero (not loaded?).", deck_id);
            return;
        }
        let cue_duration =
            Duration::from_secs_f64(position_seconds.max(0.0).min(state.duration.as_secs_f64()));
        state.cue_point = Some(cue_duration);
        log::info!(
            "Audio Thread: Set cue point for deck '{}' to {:.2}s",
            deck_id, cue_duration.as_secs_f64()
        );
    } else {
        log::error!("Audio Thread: SetCue: Deck '{}' not found.", deck_id);
        // emit_error_event might be too noisy for this if UI handles it.
    }
}

pub(crate) fn audio_thread_handle_cleanup(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    if let Some(mut removed_state) = local_states.remove(deck_id) {
        // Dropping the stream will stop it.
        if let Some(stream) = removed_state.cpal_stream.take() {
            drop(stream); // Explicitly drop to ensure it's handled before log.
        }
        log::info!("Audio Thread: Cleaned up deck '{}' (CPAL stream dropped if existed).", deck_id);
    } else {
        log::warn!("Audio Thread: CleanupDeck: Deck '{}' not found for cleanup.", deck_id);
    }
}

pub(crate) fn audio_thread_handle_set_pitch_rate<R: Runtime>(
    deck_id: &str,
    rate: f32,
    is_major_adjustment: bool, // Renamed from is_manual_adjustment for clarity in this context
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    // In Phase 1, CPAL playback is at a fixed rate (1.0).
    // This handler will just update the state variables for future phases
    // and emit the corresponding UI event. No actual audio rate change occurs.
    if let Some(state) = local_states.get_mut(deck_id) {
        let clamped_new_rate = rate.clamp(0.5, 2.0); // Still clamp for consistency

        if is_major_adjustment { // Typically from UI slider
            state.manual_pitch_rate = clamped_new_rate;
        }
        // current_pitch_rate reflects the rate that *should* be playing.
        // For Phase 1, the actual playback rate is fixed, but we update the state
        // as if it could change, for UI and later phases.
        *state.current_pitch_rate.lock().unwrap() = clamped_new_rate;
        state.last_ui_pitch_rate = Some(clamped_new_rate);

        log::info!(
            "Audio Thread: Set target pitch rate for deck '{}' to {} (Note: Audio playback rate fixed at 1.0 in CPAL Phase 1)",
            deck_id, clamped_new_rate
        );
        emit_pitch_tick_event(app_handle, deck_id, clamped_new_rate);

        // Sync logic (Phase 4/5 - placeholder for now)
        if state.is_master && is_major_adjustment {
            // TODO: Update target rates for slaves in later phases
        }

    } else {
        log::warn!("Audio Thread: SetPitchRate: Deck '{}' not found.", deck_id);
        // emit_error_event(app_handle, deck_id, "Deck not found for pitch rate adjustment.");
    }
} 