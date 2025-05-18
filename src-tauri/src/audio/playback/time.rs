use super::state::AudioThreadDeckState;
use super::events::{emit_pitch_tick_event, emit_status_update_event, emit_tick_event};
use super::sync;
use crate::audio::config;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Runtime};

/// Gets accurate playback time from audio buffer state
pub(crate) fn get_audio_buffer_accurate_time_secs(
    deck_id: &str,
    deck_state: &AudioThreadDeckState,
) -> Result<f64, crate::audio::errors::PlaybackError> {
    if deck_state.sample_rate == 0.0 {
        return Ok(0.0);
    }

    let is_playing = *deck_state.is_playing.lock().map_err(|_| {
        crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
            "Failed to lock is_playing for deck '{}'.", deck_id
        ))
    })?;

    let read_head = if is_playing {
        *deck_state.current_sample_read_head.lock().map_err(|_| {
            crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
                "Failed to lock current_sample_read_head for deck '{}'.", deck_id
            ))
        })?
    } else {
        deck_state.paused_position_read_head.lock().map_err(|_| {
            crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
                "Failed to lock paused_position_read_head for deck '{}'.", deck_id
            ))
        })?.unwrap_or(0.0)
    };

    let time_secs = read_head / deck_state.sample_rate as f64;
    Ok(time_secs.min(deck_state.duration.as_secs_f64()).max(0.0))
}

/// Processes time slice updates for all decks, emitting UI and sync events as needed
pub(crate) fn process_time_slice_updates<R: Runtime>(
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), crate::audio::errors::PlaybackError> {
    // Collect deck timing info
    let mut deck_times = HashMap::new();
    for (deck_id, deck_state) in local_states.iter() {
        let current_time = get_audio_buffer_accurate_time_secs(deck_id, deck_state)?;
        let is_playing = *deck_state.is_playing.lock().map_err(|_| {
            crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
                "Failed to lock is_playing for deck '{}'.", deck_id
            ))
        })?;
        
        if is_playing || deck_state.is_master {
            let track_ended = !is_playing 
                && deck_state.duration > Duration::ZERO
                && current_time >= deck_state.duration.as_secs_f64() - 0.01;
            deck_times.insert(deck_id.clone(), (current_time, is_playing, track_ended));
        }
    }
    // Process PLL sync corrections
    let pll_times: HashMap<String, (f64, bool)> = deck_times.iter()
        .map(|(id, (time, _, ended))| (id.clone(), (*time, *ended)))
        .collect();
    let pitch_corrections = sync::calculate_pll_pitch_updates(local_states, &pll_times)?;
    
    // Apply PLL corrections
    let dt = Duration::from_millis(config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS).as_secs_f32();
    let mut pitch_updates = Vec::new();
    
    for (deck_id, (p_correction, error)) in &pitch_corrections {
        if let Some(deck_state) = local_states.get(deck_id) {
            if deck_state.is_sync_active {
                // Calculate integral correction
                let integral_error = (deck_state.pll_integral_error + error * dt * sync::PLL_KI)
                    .clamp(-sync::MAX_PLL_INTEGRAL_ERROR, sync::MAX_PLL_INTEGRAL_ERROR);
                
                // Total correction with limits
                let total_correction = (p_correction + integral_error)
                    .clamp(-sync::MAX_PLL_PITCH_ADJUSTMENT, sync::MAX_PLL_PITCH_ADJUSTMENT);
                
                let new_pitch = deck_state.target_pitch_rate_for_bpm_match + total_correction;
                let current_pitch = *deck_state.current_pitch_rate.lock().map_err(|_| {
                    crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
                        "Failed to lock current_pitch_rate for deck '{}'.", deck_id
                    ))
                })?;
                
                if (new_pitch - current_pitch).abs() > 0.00005 {
                    pitch_updates.push((deck_id.clone(), new_pitch, integral_error));
                }
            }
        }
    }
    // Apply pitch updates
    for (deck_id, new_pitch, integral_error) in pitch_updates {
        if let Some(deck_state) = local_states.get_mut(&deck_id) {
            deck_state.pll_integral_error = integral_error;
            let clamped_pitch = new_pitch.clamp(0.5, 2.0);
            *deck_state.target_pitch_rate.lock().map_err(|_| {
                crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
                    "Failed to lock target_pitch_rate for deck '{}'.", deck_id
                ))
            })? = clamped_pitch;
            emit_pitch_tick_event(app_handle, &deck_id, clamped_pitch);
            deck_state.last_ui_pitch_rate = Some(clamped_pitch);
        }
    }
    // Update UI events for all processed decks
    for (deck_id, (current_time, is_playing, track_ended)) in deck_times {
        if let Some(deck_state) = local_states.get_mut(&deck_id) {
            // Emit pitch updates if changed significantly
            let current_pitch = *deck_state.current_pitch_rate.lock().map_err(|_| {
                crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
                    "Failed to lock current_pitch_rate for deck '{}'.", deck_id
                ))
            })?;
            let last_ui_pitch = deck_state.last_ui_pitch_rate.unwrap_or(deck_state.manual_pitch_rate);
            if (current_pitch - last_ui_pitch).abs() > 0.0002 {
                emit_pitch_tick_event(app_handle, &deck_id, current_pitch);
                deck_state.last_ui_pitch_rate = Some(current_pitch);
            }
            
            // Handle track end
            if track_ended && !is_playing && deck_state.sample_rate > 0.0 {
                let final_read_head = deck_state.decoded_samples.len().saturating_sub(1) as f64;
                *deck_state.current_sample_read_head.lock().map_err(|_| {
                    crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
                        "Failed to lock current_sample_read_head for deck '{}'.", deck_id
                    ))
                })? = final_read_head;
                *deck_state.paused_position_read_head.lock().map_err(|_| {
                    crate::audio::errors::PlaybackError::LogicalStateLockError(format!(
                        "Failed to lock paused_position_read_head for deck '{}'.", deck_id
                    ))
                })? = Some(final_read_head);
                emit_status_update_event(app_handle, &deck_id, false);
            }
            
            // Emit timing updates
            if is_playing || track_ended {
                emit_tick_event(app_handle, &deck_id, current_time);
            }
        }
    }
    Ok(())
}
