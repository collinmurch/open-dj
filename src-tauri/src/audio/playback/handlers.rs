use std::collections::HashMap;
use std::sync::{Arc, Mutex}; // Mutex for EqParams, trim_gain, current_pitch_rate
use std::time::{Duration, Instant};

use rodio::{buffer::SamplesBuffer, Sink, Source};
use tauri::{AppHandle, Runtime};

use crate::audio::config::INITIAL_TRIM_GAIN; // Changed from {self, INITIAL_TRIM_GAIN}
use crate::audio::decoding;
use crate::audio::effects::EqSource;
use crate::audio::errors::PlaybackError; // Potentially for constructing errors, though emit_error_event takes string
use crate::audio::types::EqParams;     // Used by handlers

use super::state::AudioThreadDeckState;
use super::events::*; // For calling emit_..._event functions
use super::sync; // Added for calculate_pll_pitch_updates
use super::time::get_current_playback_time_secs; // Added for this function

// --- Private Handler Functions for Audio Thread Commands ---

pub(crate) fn audio_thread_handle_init<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    audio_handle: &rodio::OutputStreamHandle,
    app_handle: &AppHandle<R>,
) {
    if local_states.contains_key(deck_id) {
        log::warn!(
            "Audio Thread: InitDeck: Deck '{}' already exists. No action taken.",
            deck_id
        );
        emit_error_event(app_handle, deck_id, "Deck already initialized.");
        return;
    }

    match Sink::try_new(audio_handle) {
        Ok(sink) => {
            let initial_eq_params = Arc::new(Mutex::new(EqParams::default()));
            let initial_trim_gain = Arc::new(Mutex::new(INITIAL_TRIM_GAIN));
            let initial_pitch_rate = Arc::new(Mutex::new(1.0f32));

            let deck_state = AudioThreadDeckState {
                sink,
                decoded_samples: Arc::new(Vec::new()),
                sample_rate: 0.0,
                playback_start_time: None,
                paused_position: None,
                duration: Duration::ZERO,
                is_playing: false,
                eq_params: initial_eq_params,
                trim_gain: initial_trim_gain,
                cue_point: None,
                current_pitch_rate: initial_pitch_rate.clone(),
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
            log::info!("Audio Thread: Initialized deck '{}'", deck_id);

            emit_load_update_event(app_handle, deck_id, 0.0, None, None, None);
            emit_status_update_event(app_handle, deck_id, false);
            emit_sync_status_update_event(app_handle, deck_id, false, false);
            emit_pitch_tick_event(app_handle, deck_id, 1.0);
        }
        Err(e) => {
            log::error!(
                "Audio Thread: Failed to create sink for deck '{}': {:?}",
                deck_id,
                e
            );
            emit_error_event(
                app_handle,
                deck_id,
                &format!("Failed to initialize audio sink: {}", e),
            );
        }
    }
}

pub(crate) async fn audio_thread_handle_load<R: Runtime>(
    deck_id: String,
    path: String,
    original_bpm: Option<f32>,
    first_beat_sec: Option<f32>,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    let (eq_params_arc, trim_gain_arc) = match local_states.get(&deck_id) {
        Some(state) => (state.eq_params.clone(), state.trim_gain.clone()),
        None => {
            let err = PlaybackError::DeckNotFound {
                deck_id: deck_id.to_string(),
            };
            log::error!("Audio Thread: LoadTrack: {:?}", err);
            emit_error_event(app_handle, &deck_id, &err.to_string());
            return;
        }
    };

    let path_clone = path.clone();
    let decode_handle = tokio::task::spawn_blocking(move || {
        decoding::decode_file_to_mono_samples(&path_clone)
    });

    match decode_handle.await {
        Ok(decode_result) => {
            if let Some(deck_state) = local_states.get_mut(&deck_id) {
                match decode_result {
                    Ok((samples, rate)) => {
                        let duration = Duration::from_secs_f64(samples.len() as f64 / rate as f64);
                        log::info!(
                            "Audio Thread: Decoded '{}'. Duration: {:?}, Rate: {}, Samples: {}",
                            path,
                            duration,
                            rate,
                            samples.len()
                        );

                        deck_state.decoded_samples = Arc::new(samples);
                        deck_state.sample_rate = rate;
                        deck_state.duration = duration;
                        deck_state.cue_point = None;
                        deck_state.original_bpm = original_bpm;
                        deck_state.first_beat_sec = first_beat_sec;
                        log::info!(
                            "AudioThread handle_load [POST-SET]: Stored BPM: {:?}, FBS: {:?} for Deck '{}'",
                            deck_state.original_bpm,
                            deck_state.first_beat_sec,
                            deck_id
                        );
                        deck_state.is_sync_active = false;
                        deck_state.is_master = false;
                        deck_state.master_deck_id = None;
                        deck_state.target_pitch_rate_for_bpm_match = 1.0;
                        deck_state.manual_pitch_rate = 1.0;
                        deck_state.last_ui_pitch_rate = Some(1.0);

                        let buffer = SamplesBuffer::new(
                            1,
                            rate as u32,
                            (*deck_state.decoded_samples).clone(),
                        );

                        match EqSource::new(buffer, eq_params_arc.clone(), trim_gain_arc.clone()) {
                            Ok(unwrapped_eq_source) => {
                                {
                                    let mut rate_lock = deck_state.current_pitch_rate.lock().unwrap();
                                    *rate_lock = 1.0f32;
                                }
                                let speed = *deck_state.current_pitch_rate.lock().unwrap();
                                let speed_source = unwrapped_eq_source.speed(speed);

                                deck_state.sink.stop();
                                deck_state.sink.clear();
                                deck_state.sink.append(speed_source.convert_samples::<f32>());
                                deck_state.sink.set_speed(speed);
                                deck_state.sink.pause();
                                deck_state.sink.set_volume(1.0);

                                deck_state.is_playing = false;
                                deck_state.playback_start_time = None;
                                deck_state.paused_position = Some(Duration::ZERO);
                                deck_state.last_ui_pitch_rate = Some(1.0);

                                let current_duration_secs = duration.as_secs_f64();
                                emit_load_update_event(app_handle, &deck_id, current_duration_secs, None, original_bpm, first_beat_sec);
                                emit_status_update_event(app_handle, &deck_id, false);
                                emit_sync_status_update_event(app_handle, &deck_id, false, false);
                                emit_pitch_tick_event(app_handle, &deck_id, 1.0);
                            }
                            Err(eq_creation_error) => {
                                let err_msg = format!(
                                    "Failed to create EQ source for deck '{}': {:?}",
                                    deck_id, eq_creation_error
                                );
                                log::error!("Audio Thread: {}", err_msg);
                                emit_error_event(app_handle, &deck_id, &err_msg);
                            }
                        }
                    }
                    Err(e_decode) => {
                        let err = PlaybackError::PlaybackDecodeError {
                            deck_id: deck_id.to_string(),
                            source: e_decode,
                        };
                        log::error!("Audio Thread: Decode failed: {:?}", err);
                        let err_string = err.to_string();
                        emit_error_event(app_handle, &deck_id, &err_string);
                    }
                }
            } else {
                log::error!(
                    "Audio Thread: Deck '{}' disappeared after decode attempt?!",
                    deck_id
                );
            }
        }
        Err(join_error) => {
            log::error!(
                "Audio Thread: Decode task panicked for deck '{}': {}",
                deck_id,
                join_error
            );
            let error_msg = format!("Audio decoding task failed: {}", join_error);
            emit_error_event(app_handle, &deck_id, &error_msg);
            if let Some(deck_state) = local_states.get_mut(&deck_id) {
                deck_state.decoded_samples = Arc::new(Vec::new());
            }
        }
    }
}

pub(crate) fn audio_thread_handle_play<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        if state.sink.empty() {
            log::warn!(
                "Audio Thread: Play ignored for deck '{}', sink is empty.",
                deck_id
            );
            emit_error_event(app_handle, deck_id, "Cannot play: No track loaded or track is empty.");
            return; // Return after emitting error
        }
        state.sink.play();
        state.is_playing = true;
        if state.paused_position.is_some() {
            state.playback_start_time = Some(Instant::now());
        } else {
            state.playback_start_time = Some(Instant::now());
        }
        log::info!("Audio Thread: Playing deck '{}'", deck_id);
        emit_status_update_event(app_handle, deck_id, true);
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
        state.sink.pause();
        state.is_playing = false;
        let current_time_secs = get_current_playback_time_secs(state);
        state.paused_position = Some(Duration::from_secs_f64(current_time_secs));
        state.playback_start_time = None;

        let was_master = state.is_master;
        let was_slave = state.is_sync_active;

        log::info!("Audio Thread: Paused deck '{}', Paused Position: {:?}", deck_id, state.paused_position);
        emit_status_update_event(app_handle, deck_id, false);

        if was_master {
            log::info!("Master deck '{}' paused. Disabling sync for its slaves.", deck_id);
            let master_id_str = deck_id.to_string();
            let slaves_to_disable: Vec<String> = local_states
                .iter()
                .filter(|(_id, s)| s.master_deck_id.as_deref() == Some(&master_id_str))
                .map(|(id, _)| id.clone())
                .collect();

            for slave_id in slaves_to_disable {
                log::debug!("Pausing master: Disabling sync for slave '{}'", slave_id);
                sync::audio_thread_handle_disable_sync(&slave_id, local_states, app_handle);
            }
        } else if was_slave {
            log::info!("Slave deck '{}' paused. Disabling its sync.", deck_id);
            sync::audio_thread_handle_disable_sync(deck_id, local_states, app_handle);
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
        if state.sink.empty() || state.decoded_samples.is_empty() || state.sample_rate == 0.0 {
            log::warn!(
                "Audio Thread: Seek ignored for deck '{}', no track loaded or invalid state.",
                deck_id
            );
            // No error event needed here as it's a warning for a valid but no-op state.
            return;
        }

        let seek_duration = Duration::from_secs_f64(position_seconds.max(0.0));
        let final_seek_duration = if seek_duration > state.duration {
            log::warn!(
                "Audio Thread: Seek position {:.2}s beyond duration {:.2}s for deck '{}'. Clamping to duration.",
                position_seconds,
                state.duration.as_secs_f64(),
                deck_id
            );
            state.duration
        } else {
            seek_duration
        };

        let new_source = SamplesBuffer::new(1, state.sample_rate as u32, state.decoded_samples.to_vec());
        let eq_source = match EqSource::new(new_source, state.eq_params.clone(), state.trim_gain.clone()) {
            Ok(eq) => eq,
            Err(e) => {
                log::error!("Failed to create EqSource for seek: {:?}", e);
                // Error during EQ source creation, should probably emit
                emit_error_event(app_handle, deck_id, &format!("Failed to create EQ source for seek: {}", e));
                return;
            }
        };
        let current_dynamic_rate = *state.current_pitch_rate.lock().unwrap();
        let speed_source = eq_source.speed(current_dynamic_rate);
        state.sink.stop();
        state.sink.clear();
        state.sink.append(
            speed_source
                .skip_duration(final_seek_duration)
                .convert_samples::<f32>()
        );
        state.sink.set_speed(current_dynamic_rate);
        state.paused_position = Some(final_seek_duration);

        if state.is_playing {
            state.sink.play();
            state.playback_start_time = Some(Instant::now());
        } else {
            state.sink.pause();
            state.playback_start_time = None;
        }
        emit_tick_event(app_handle, deck_id, final_seek_duration.as_secs_f64());
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
    if let Some(state) = local_states.get_mut(deck_id) {
        let clamped_level = level.clamp(0.0, 1.0);
        state.sink.set_volume(clamped_level);
        log::debug!(
            "Audio Thread: Set fader level for deck '{}' to {}",
            deck_id,
            clamped_level
        );
    } else {
        log::warn!("Audio Thread: SetFaderLevel: Deck '{}' not found.", deck_id);
        // No error event needed for a non-existent deck for fader typically.
    }
}

pub(crate) fn audio_thread_handle_set_trim_gain(
    deck_id: &str,
    gain: f32,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        let mut trim_gain_lock = state.trim_gain.lock().expect("Failed to lock trim_gain");
        *trim_gain_lock = gain;
        log::debug!(
            "Audio Thread: Set trim_gain (linear) for deck '{}' to {}",
            deck_id,
            gain
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
        let mut eq_params_lock = state
            .eq_params
            .lock()
            .expect("Failed to lock eq_params for update");
        *eq_params_lock = new_params;
        log::debug!("Audio Thread: Updated EQ params for deck '{}'", deck_id);
    } else {
        log::warn!("Audio Thread: SetEq: Deck '{}' not found.", deck_id);
    }
}

pub(crate) fn audio_thread_handle_set_cue<R: Runtime>(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        if state.duration == Duration::ZERO {
            log::warn!(
                "Audio Thread: SetCue ignored for deck '{}', track duration is zero (not loaded?).",
                deck_id
            );
            // Optional: emit_error_event(app_handle, deck_id, "Cannot set cue: Track not loaded or empty.");
            return;
        }
        let cue_duration =
            Duration::from_secs_f64(position_seconds.max(0.0).min(state.duration.as_secs_f64()));
        state.cue_point = Some(cue_duration);
        log::info!(
            "Audio Thread: Set cue point for deck '{}' to {:.2}s",
            deck_id,
            cue_duration.as_secs_f64()
        );
        // Optionally emit an event that the cue point was updated if the UI needs to know
        // emit_cue_update_event(app_handle, deck_id, Some(cue_duration.as_secs_f64()));
    } else {
        log::error!("Audio Thread: SetCue: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for set cue operation.");
    }
}

pub(crate) fn audio_thread_handle_cleanup(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    if local_states.remove(deck_id).is_some() {
        log::info!("Audio Thread: Cleaned up deck '{}'", deck_id);
    } else {
        log::warn!(
            "Audio Thread: CleanupDeck: Deck '{}' not found for cleanup.",
            deck_id
        );
    }
}

pub(crate) fn audio_thread_handle_set_pitch_rate<R: Runtime>(
    deck_id: &str,
    rate: f32,
    is_major_adjustment: bool,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    let mut is_slave_manual_override_detected = false;

    if let Some(state) = local_states.get_mut(deck_id) {
        if state.is_sync_active && is_major_adjustment {
            if (rate - state.target_pitch_rate_for_bpm_match).abs() > 1e-4 { 
                log::warn!("Major pitch adjustment (manual override) received for synced slave '{}' (rate: {} vs target_bpm_match: {}). Disabling sync.", deck_id, rate, state.target_pitch_rate_for_bpm_match);
                is_slave_manual_override_detected = true;
            } else {
                log::debug!("Major pitch adjustment for synced slave '{}' matches target_bpm_match_rate. Likely initial sync.", deck_id);
            }
        }

        let clamped_new_rate = rate.clamp(0.5, 2.0);
        
        if is_major_adjustment {
             state.manual_pitch_rate = clamped_new_rate;
             state.last_ui_pitch_rate = Some(clamped_new_rate);
             log::debug!("Storing manual pitch rate for {}: {} AND UI pitch rate: {}", deck_id, clamped_new_rate, clamped_new_rate);
        }

        if is_major_adjustment {
            let current_true_audio_time_secs: f64 = get_current_playback_time_secs(state);
            let old_rate_for_log: f32;
            {
                let mut rate_lock = state.current_pitch_rate.lock().unwrap();
                old_rate_for_log = *rate_lock;
                *rate_lock = clamped_new_rate;
            }
            state.paused_position = Some(Duration::from_secs_f64(current_true_audio_time_secs));
            state.sink.set_speed(clamped_new_rate);

            if state.is_playing {
                state.playback_start_time = Some(Instant::now());
            } else {
                state.playback_start_time = None;
            }
            log::info!(
                "Audio Thread: Set pitch rate (MAJOR RE-BASE) for deck '{}' to {} at audio time {:.3}s. Old rate: {}",
                deck_id, clamped_new_rate, current_true_audio_time_secs, old_rate_for_log
            );
            emit_pitch_tick_event(app_handle, deck_id, clamped_new_rate);
        } else {
            let old_rate_for_log: f32;
            {
                let mut rate_lock = state.current_pitch_rate.lock().unwrap();
                old_rate_for_log = *rate_lock; 
                *rate_lock = clamped_new_rate;
            }
            state.sink.set_speed(clamped_new_rate);
            log::trace!(
                "Audio Thread: Set pitch rate (MINOR/PLL) for deck '{}' to {}. Old rate: {}",
                deck_id, clamped_new_rate, old_rate_for_log
            );
        }

    } else {
        log::warn!("Audio Thread: SetPitchRate: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for pitch rate adjustment.");
        return;
    }

    if is_slave_manual_override_detected {
         let deck_id_clone = deck_id.to_string();
         sync::audio_thread_handle_disable_sync(&deck_id_clone, local_states, app_handle);
    }
    
    if let Some(state_after_pitch_set) = local_states.get(deck_id) {
        if state_after_pitch_set.is_master {
            let master_bpm_opt = state_after_pitch_set.original_bpm;
            if master_bpm_opt.is_none() {
                log::warn!("Master deck '{}' changed rate but is missing BPM. Cannot update slaves.", deck_id);
                return;
            }
            let master_current_actual_pitch = *state_after_pitch_set.current_pitch_rate.lock().unwrap();
            let master_id_str = deck_id.to_string();
            
            let slave_ids_to_update: Vec<String> = local_states.iter()
                .filter_map(|(id, s)| {
                    if s.is_sync_active && s.master_deck_id.as_deref() == Some(&master_id_str) {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect();

            if !slave_ids_to_update.is_empty() {
                log::debug!("Master '{}' actual pitch is now {}. Updating targets for slaves: {:?}", master_id_str, master_current_actual_pitch, slave_ids_to_update);
                for slave_id_for_update in slave_ids_to_update {
                    if let Some(slave_state_for_target_update) = local_states.get_mut(&slave_id_for_update) {
                        if let Some(slave_bpm) = slave_state_for_target_update.original_bpm {
                            if slave_bpm.abs() > 1e-6 && master_bpm_opt.unwrap().abs() > 1e-6 {
                                let new_target_rate_for_slave = (master_bpm_opt.unwrap() / slave_bpm) * master_current_actual_pitch;
                                slave_state_for_target_update.target_pitch_rate_for_bpm_match = new_target_rate_for_slave;
                                log::debug!("Updated target BPM match rate for slave '{}' to: {:.4}", slave_id_for_update, new_target_rate_for_slave);
                            } else { log::warn!("Cannot update target rate for slave '{}', its or master's BPM is zero.", slave_id_for_update); }
                        } else { log::warn!("Cannot update target rate for slave '{}', missing BPM.", slave_id_for_update); }
                    } else { log::warn!("Failed to get mutable state for slave '{}' while updating target.", slave_id_for_update); }
                }
            }
        }
    }
} 