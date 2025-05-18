use std::time::{Duration, Instant};
use super::state::AudioThreadDeckState;

// Added imports for the new function
use std::collections::HashMap;
use tauri::{AppHandle, Runtime};
use crate::audio::config;
use super::events::{emit_status_update_event, emit_tick_event, emit_pitch_tick_event};
use super::sync::{self};

/// Returns the current playback time in seconds for a given deck.
/// Returns an error if a lock cannot be acquired.
pub(crate) fn get_current_playback_time_secs(deck_id: &str, deck_state: &AudioThreadDeckState) -> Result<f64, crate::audio::errors::PlaybackError> {
    let is_playing = *deck_state.is_playing.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing for deck '{}'.", deck_id)))?;
    let source_sample_rate = deck_state.sample_rate;

    if source_sample_rate == 0.0 { return Ok(0.0); }

    if is_playing {
        let maybe_last_playback_instant = *deck_state.last_playback_instant.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock last_playback_instant for deck '{}'.", deck_id)))?;
        let maybe_read_head_at_last_instant = *deck_state.read_head_at_last_playback_instant.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock read_head_at_last_playback_instant for deck '{}'.", deck_id)))?;
        let maybe_output_sample_rate = deck_state.output_sample_rate; // Not Arc<Mutex>
        let current_pitch_rate_val = *deck_state.current_pitch_rate.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_pitch_rate for deck '{}'.", deck_id)))?;

        if let (Some(last_playback_instant), Some(read_head_at_last_instant), Some(output_sample_rate_val)) = 
            (maybe_last_playback_instant, maybe_read_head_at_last_instant, maybe_output_sample_rate) {
            if source_sample_rate == 0.0 { 
                return Ok(read_head_at_last_instant / 1.0);
            }

            let now = Instant::now();
            let elapsed_since_last_cb_playback_secs = if now > last_playback_instant {
                now.duration_since(last_playback_instant).as_secs_f64()
            } else {
                0.0
            };

            let estimated_source_samples_advanced = elapsed_since_last_cb_playback_secs * (source_sample_rate as f64) * current_pitch_rate_val as f64;
            let estimated_current_read_head = read_head_at_last_instant + estimated_source_samples_advanced;

            log::trace!(
                "Deck '{}' TimeCalc (ESTIMATED): IsPlaying {}, LastBI {:?}, RH@LastBI {:.2}, OutSR(unused) {}, SrcSR {}, Pitch {:.4}, ElapsedS {:.4}, EstAdv {:.2}, EstRH {:.2}", 
                deck_id, is_playing, 
                last_playback_instant, read_head_at_last_instant, output_sample_rate_val, source_sample_rate, current_pitch_rate_val,
                elapsed_since_last_cb_playback_secs, estimated_source_samples_advanced, estimated_current_read_head
            );

            return Ok((estimated_current_read_head / source_sample_rate as f64).min(deck_state.duration.as_secs_f64()));
        } else {
            let current_read_head_pos = *deck_state.current_sample_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_sample_read_head for deck '{}'.", deck_id)))?;
            log::trace!(
                "Deck '{}' TimeCalc: Using DIRECT fallback (missing precise timing info). RH {:.2}, SrcSR {}, IsPlaying {}", 
                deck_id, current_read_head_pos, source_sample_rate, is_playing
            );
            return Ok((current_read_head_pos / source_sample_rate as f64).min(deck_state.duration.as_secs_f64()));
        }
    } else {
        let current_read_head_pos = deck_state.paused_position_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock paused_position_read_head for deck '{}'.", deck_id)))?.unwrap_or(0.0);
        return Ok((current_read_head_pos / source_sample_rate as f64).min(deck_state.duration.as_secs_f64()));
    }
}

/// Processes time slice updates for all decks, emitting UI and sync events as needed.
/// Returns an error if a lock cannot be acquired.
pub(crate) fn process_time_slice_updates<R: Runtime>(
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), crate::audio::errors::PlaybackError> {
    let mut decks_to_process: HashMap<String, (f64, bool, bool)> = HashMap::new();
    for (deck_id, deck_state) in local_states.iter() {
        let current_time_secs = get_current_playback_time_secs(deck_id, deck_state)?;
        let is_logically_playing = *deck_state.is_playing.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing for deck '{}'.", deck_id)))?;
        let is_master_deck = deck_state.is_master;
        if is_logically_playing || is_master_deck {
            let has_ended_in_ui_timeline = !is_logically_playing && deck_state.duration > Duration::ZERO && 
                                          current_time_secs >= deck_state.duration.as_secs_f64() - 0.01;
            decks_to_process.insert(deck_id.clone(), (current_time_secs, is_logically_playing, has_ended_in_ui_timeline));
        }
    }
    let decks_for_pll: HashMap<String, (f64, bool)> = decks_to_process.iter()
        .map(|(id, (time, _, ended_ui))| (id.clone(), (*time, *ended_ui)))
        .collect();
    let slave_pitch_info_map = sync::calculate_pll_pitch_updates(local_states, &decks_for_pll)?;
    let mut pll_pitch_updates_to_apply: Vec<(String, f32)> = Vec::new();
    for (deck_id_str, (_current_time, _is_logically_playing, _has_ended_in_ui_timeline)) in &decks_to_process {
        let deck_id = deck_id_str.as_str();
        if let Some(deck_state_for_pll_calc) = local_states.get(deck_id) { 
            if deck_state_for_pll_calc.is_sync_active { 
                if let Some(&(proportional_correction, signed_error)) = slave_pitch_info_map.get(deck_id) {
                    let dt = Duration::from_millis(config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS).as_secs_f32();
                    let current_integral_error = deck_state_for_pll_calc.pll_integral_error;
                    let new_integral_error = (current_integral_error + signed_error * dt * sync::PLL_KI).clamp(-sync::MAX_PLL_INTEGRAL_ERROR, sync::MAX_PLL_INTEGRAL_ERROR);
                    let integral_correction = new_integral_error; 
                    let total_correction = proportional_correction + integral_correction;
                    let capped_total_correction = total_correction.clamp(-sync::MAX_PLL_PITCH_ADJUSTMENT, sync::MAX_PLL_PITCH_ADJUSTMENT);
                    let new_pitch_rate_from_pll = deck_state_for_pll_calc.target_pitch_rate_for_bpm_match + capped_total_correction;
                    let current_engine_pitch = *deck_state_for_pll_calc.current_pitch_rate.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_pitch_rate for deck '{}'.", deck_id)))?;
                    const MIN_PLL_ADJUSTMENT_TO_APPLY: f32 = 0.0001; 
                    if (new_pitch_rate_from_pll - current_engine_pitch).abs() > MIN_PLL_ADJUSTMENT_TO_APPLY {
                        pll_pitch_updates_to_apply.push((deck_id.to_string(), new_pitch_rate_from_pll));
                        log::trace!(
                            "PLL Calc {}: TargetRate {:.4}, P_Corr {:.4}, I_Error_New {:.4}, TotalCorr {:.4}, CappedCorr {:.4}, QueuedPitch {:.4}",
                            deck_id, deck_state_for_pll_calc.target_pitch_rate_for_bpm_match, proportional_correction, 
                            new_integral_error, total_correction, capped_total_correction, new_pitch_rate_from_pll
                        );
                    }
                }
            }
        }
    }
    for (deck_id_to_update, new_pitch_rate) in pll_pitch_updates_to_apply {
        if let Some(deck_state_to_update) = local_states.get_mut(&deck_id_to_update) {
            if let Some(&(_ , signed_error_for_update)) = slave_pitch_info_map.get(&deck_id_to_update) {
                 let dt = Duration::from_millis(config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS).as_secs_f32();
                 deck_state_to_update.pll_integral_error = (deck_state_to_update.pll_integral_error + signed_error_for_update * dt * sync::PLL_KI).clamp(-sync::MAX_PLL_INTEGRAL_ERROR, sync::MAX_PLL_INTEGRAL_ERROR);
            }
            let clamped_new_target_rate = new_pitch_rate.clamp(0.5, 2.0);
            *deck_state_to_update.target_pitch_rate.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock target_pitch_rate for deck '{}'.", deck_id_to_update)))? = clamped_new_target_rate;
            emit_pitch_tick_event(app_handle, &deck_id_to_update, clamped_new_target_rate);
            deck_state_to_update.last_ui_pitch_rate = Some(clamped_new_target_rate);
            log::trace!("PLL Applied for {}: New Target Pitch Rate {:.4}, Updated Integral Err: {:.4}", 
                deck_id_to_update, clamped_new_target_rate, deck_state_to_update.pll_integral_error);
        }
    }
    for (deck_id_str, (current_time, is_logically_playing, has_ended_in_ui_timeline)) in decks_to_process {
        if let Some(deck_state) = local_states.get_mut(&deck_id_str) {
            let final_engine_pitch = *deck_state.current_pitch_rate.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_pitch_rate for deck '{}'.", deck_id_str)))?;
            const UI_PITCH_UPDATE_THRESHOLD: f32 = 0.0005;
            let pitch_previously_sent_to_ui = deck_state.last_ui_pitch_rate.unwrap_or(deck_state.manual_pitch_rate);
            if (final_engine_pitch - pitch_previously_sent_to_ui).abs() > UI_PITCH_UPDATE_THRESHOLD {
                emit_pitch_tick_event(app_handle, &deck_id_str, final_engine_pitch);
                deck_state.last_ui_pitch_rate = Some(final_engine_pitch);
            }
            if has_ended_in_ui_timeline && !is_logically_playing {
                if deck_state.sample_rate > 0.0 { 
                    let final_read_head = deck_state.decoded_samples.len().saturating_sub(1) as f64;
                    *deck_state.current_sample_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_sample_read_head for deck '{}'.", deck_id_str)))? = final_read_head;
                    *deck_state.paused_position_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock paused_position_read_head for deck '{}'.", deck_id_str)))? = Some(final_read_head);
                }
                emit_status_update_event(app_handle, &deck_id_str, false);
            }
            if is_logically_playing || has_ended_in_ui_timeline {
                emit_tick_event(app_handle, &deck_id_str, current_time);
            }
        }
    }
    Ok(())
} 