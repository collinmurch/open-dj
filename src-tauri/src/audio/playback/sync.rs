pub(crate) const PLL_KP: f32 = 0.001; // Reduced for stability
pub(crate) const MAX_PLL_PITCH_ADJUSTMENT: f32 = 0.02; // Reduced for gentler corrections
pub(crate) const PLL_KI: f32 = 0.0005; // Reduced for stability
pub(crate) const MAX_PLL_INTEGRAL_ERROR: f32 = 2.0; // Reduced to prevent windup

use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Runtime};

use super::state::AudioThreadDeckState;
use super::events::{emit_error_event, emit_sync_status_update_event};
use crate::audio::playback::events::emit_tick_event;
use super::handlers::{audio_thread_handle_set_pitch_rate};


// --- Sync Handler Functions ---

/// Handles enabling sync for a slave deck to a master deck, including tempo and phase alignment.
/// Returns an error if a lock cannot be acquired or a deck is not found.
pub(crate) async fn audio_thread_handle_enable_sync_async<R: Runtime>(
    slave_deck_id_str: &str,
    master_deck_id_str: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>, 
    app_handle: &AppHandle<R>,
) -> Result<(), crate::audio::errors::PlaybackError> {
    let master_info = match local_states.get(master_deck_id_str) {
        Some(master_state) => {
            if master_state.duration <= Duration::ZERO {
                log::warn!("Audio Thread: EnableSync: Master deck '{}' is not loaded.", master_deck_id_str);
                emit_error_event(app_handle, slave_deck_id_str, &format!("Master deck '{}' must be loaded to sync", master_deck_id_str));
                return Ok(());
            }
            if master_state.original_bpm.is_none() {
                log::warn!("Audio Thread: EnableSync: Master deck '{}' missing BPM.", master_deck_id_str);
                emit_error_event(app_handle, slave_deck_id_str, &format!("Master deck '{}' missing BPM", master_deck_id_str));
                return Ok(());
            }
            Some((master_state.original_bpm.unwrap(), *master_state.target_pitch_rate.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock target_pitch_rate for master deck '{}'.", master_deck_id_str)))?))
        }
        None => {
            log::error!("Audio Thread: EnableSync: Master deck '{}' not found.", master_deck_id_str);
            emit_error_event(app_handle, slave_deck_id_str, &format!("Master deck '{}' not found", master_deck_id_str));
            return Ok(());
        }
    };
    if let Some((master_bpm, master_current_pitch)) = master_info {
        let calculated_target_rate_for_slave = {
            if let Some(slave_state) = local_states.get_mut(slave_deck_id_str) {
                if slave_state.original_bpm.is_none() {
                    log::warn!("Audio Thread: EnableSync: Slave deck '{}' missing BPM.", slave_deck_id_str);
                    emit_error_event(app_handle, slave_deck_id_str, "Slave deck missing BPM");
                    return Ok(());
                }
                let slave_bpm = slave_state.original_bpm.unwrap();
                let target_rate = if slave_bpm.abs() > 1e-6 {
                    (master_bpm / slave_bpm) * master_current_pitch
                } else {
                    log::warn!("Audio Thread: EnableSync: Slave BPM is zero for '{}'. Cannot calculate rate.", slave_deck_id_str);
                    emit_error_event(app_handle, slave_deck_id_str, "Slave deck BPM is zero");
                    return Ok(());
                };
                slave_state.is_sync_active = true;
                slave_state.is_master = false;
                slave_state.master_deck_id = Some(master_deck_id_str.to_string());
                slave_state.manual_pitch_rate = *slave_state.current_pitch_rate.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_pitch_rate for slave deck '{}'.", slave_deck_id_str)))?; 
                slave_state.target_pitch_rate_for_bpm_match = target_rate;
                log::info!("Tempo Sync for '{}': Target rate {:.4}. Stored manual pitch: {:.4}", slave_deck_id_str, target_rate, slave_state.manual_pitch_rate);
                emit_sync_status_update_event(app_handle, slave_deck_id_str, true, false);
                target_rate
            } else {
                log::error!("Audio Thread: EnableSync: Slave deck '{}' not found for mutable access.", slave_deck_id_str);
                emit_error_event(app_handle, slave_deck_id_str, "Slave deck not found");
                return Ok(());
            }
        };
        if slave_deck_id_str != master_deck_id_str {
            if let Some(master_state_mut) = local_states.get_mut(master_deck_id_str) {
                if !master_state_mut.is_master {
                    log::info!("Setting deck '{}' as master.", master_deck_id_str);
                    master_state_mut.is_master = true;
                    master_state_mut.is_sync_active = false;
                    master_state_mut.master_deck_id = None;
                    master_state_mut.manual_pitch_rate = *master_state_mut.current_pitch_rate.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_pitch_rate for master deck '{}'.", master_deck_id_str)))?;
                    emit_sync_status_update_event(app_handle, master_deck_id_str, false, true);
                }
            } else {
                log::error!("EnableSync: Failed to get mutable master state '{}' after initial check.", master_deck_id_str);
            }
        }
        audio_thread_handle_set_pitch_rate(
            slave_deck_id_str, 
            calculated_target_rate_for_slave, 
            false,
            local_states, 
            app_handle
        )?;

        log::info!(
            "EnableSync (Phase 4 - Tempo Sync) for slave '{}' to master '{}' complete. Slave is now tempo-matched.", 
            slave_deck_id_str, master_deck_id_str
        );

        // --- Phase 5: Simple Direct Phase Alignment ---
        log::info!("EnableSync: Performing direct phase alignment for slave '{}' to master '{}'", slave_deck_id_str, master_deck_id_str);

        // Get current positions directly - no complex timing needed
        if let (Some(master_state), Some(slave_state)) = (local_states.get(master_deck_id_str), local_states.get(slave_deck_id_str)) {
            if let (Some(m_bpm), Some(m_fbs), Some(s_bpm), Some(s_fbs)) = (
                master_state.original_bpm,
                master_state.first_beat_sec, 
                slave_state.original_bpm,
                slave_state.first_beat_sec
            ) {
                // Use accurate audio buffer timing for both decks
                let master_time = super::time::get_audio_buffer_accurate_time_secs(master_deck_id_str, master_state)?;
                let slave_time = super::time::get_audio_buffer_accurate_time_secs(slave_deck_id_str, slave_state)?;
                
                // Calculate beat intervals and phases
                let master_beat_interval = 60.0 / m_bpm as f64;
                let slave_beat_interval = 60.0 / s_bpm as f64;
                let master_phase = ((master_time - m_fbs as f64) / master_beat_interval).fract();
                let slave_phase = ((slave_time - s_fbs as f64) / slave_beat_interval).fract();
                
                // Calculate phase difference with wrapping
                let mut phase_diff = slave_phase - master_phase;
                if phase_diff > 0.5 { phase_diff -= 1.0; }
                if phase_diff < -0.5 { phase_diff += 1.0; }
                
                let time_adjustment = phase_diff * slave_beat_interval;
                let sample_adjustment = time_adjustment * slave_state.sample_rate as f64;
                
                log::info!(
                    "Phase Sync: Master={:.3}° Slave={:.3}° Diff={:.3}° Adj={:.1}ms",
                    master_phase * 360.0, slave_phase * 360.0, phase_diff * 360.0, time_adjustment * 1000.0
                );
                
                // Apply adjustment if significant (>100 samples ~2ms at 44.1kHz)
                if sample_adjustment.abs() > 100.0 {
                    if let Some(slave_state_mut) = local_states.get_mut(slave_deck_id_str) {
                        let current_read_head = *slave_state_mut.current_sample_read_head.lock().map_err(|_| 
                            crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock slave read head")))?;
                        let new_read_head = (current_read_head - sample_adjustment).max(0.0);
                        *slave_state_mut.current_sample_read_head.lock().map_err(|_| 
                            crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock slave read head for adjustment")))? = new_read_head;
                        
                        let is_playing = *slave_state_mut.is_playing.lock().map_err(|_| 
                            crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing")))?;
                        if !is_playing {
                            *slave_state_mut.paused_position_read_head.lock().map_err(|_| 
                                crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock paused position")))? = Some(new_read_head);
                        }
                        
                        emit_tick_event(app_handle, slave_deck_id_str, new_read_head / slave_state_mut.sample_rate as f64);
                        log::info!("Applied phase adjustment: {:.1} samples", sample_adjustment);
                    }
                }
            }
        }
        Ok(())
    } else {
        Ok(())
    }
}

/// Handles disabling sync for a deck, restoring manual pitch and updating slaves if needed.
/// Returns an error if a lock cannot be acquired or a deck is not found.
pub(crate) fn audio_thread_handle_disable_sync<R: Runtime>(
    deck_id_str: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), crate::audio::errors::PlaybackError> {
    log::info!("Audio Thread: Handling DisableSync for deck: {}", deck_id_str);

    let (pitch_to_restore_this_deck, was_master_before_disable, id_of_former_master_if_slave) = {
        let deck_state = local_states.get_mut(deck_id_str).ok_or_else(|| crate::audio::errors::PlaybackError::DeckNotFound { deck_id: deck_id_str.to_string() })?;
        
        if !deck_state.is_sync_active && !deck_state.is_master {
            log::warn!("DisableSync: Deck '{}' is not currently synced or master. No action needed.", deck_id_str);
            return Ok(());
        }

        let pitch = deck_state.manual_pitch_rate;
        let was_master_flag = deck_state.is_master;
        let former_master_id = if !was_master_flag && deck_state.is_sync_active {
            deck_state.master_deck_id.clone()
        } else {
            None
        };

        deck_state.is_sync_active = false;
        deck_state.is_master = false;
        deck_state.master_deck_id = None;
        deck_state.target_pitch_rate_for_bpm_match = 1.0; // Reset BPM match target
        deck_state.pll_integral_error = 0.0; // Reset PLL error
        (pitch, was_master_flag, former_master_id)
    };

    log::info!("Deck '{}' sync/master status disabled. Will restore its pitch to: {:.4}", deck_id_str, pitch_to_restore_this_deck);
    emit_sync_status_update_event(app_handle, deck_id_str, false, false);
    audio_thread_handle_set_pitch_rate(deck_id_str, pitch_to_restore_this_deck, true, local_states, app_handle)?;

    let mut potential_masters_to_demote: Vec<String> = Vec::new();

    if was_master_before_disable {
        log::info!("Deck '{}' was master. Finding and disabling sync for its former slaves...", deck_id_str);
        let slaves_of_this_deck: Vec<String> = local_states
            .iter()
            .filter(|(id, state)| *id != deck_id_str && state.master_deck_id.as_deref() == Some(deck_id_str))
            .map(|(id, _)| id.clone())
            .collect();
        
        if !slaves_of_this_deck.is_empty() {
            log::info!("Disabling sync for former slaves of '{}': {:?}", deck_id_str, slaves_of_this_deck);
            for slave_id_str_of_disabled_master in slaves_of_this_deck {
                // Recursive call. This will handle chains and also add the slaves' original masters (if any) to potential_masters_to_demote if they become relevant.
                audio_thread_handle_disable_sync(&slave_id_str_of_disabled_master, local_states, app_handle)?;
            }
        }
    } else if let Some(fm_id) = id_of_former_master_if_slave {
        // This deck was a slave. Its former master is a candidate for demotion if it has no other slaves.
        potential_masters_to_demote.push(fm_id);
    }

    // Process demotions for masters that might have lost their last slave
    for master_to_check_id in potential_masters_to_demote {
        // Check if this master_to_check_id still has any active slaves in the current state of local_states
        let master_still_has_active_slaves = local_states.iter().any(|(_id, state)| {
            state.is_sync_active && state.master_deck_id.as_deref() == Some(&master_to_check_id)
        });

        if !master_still_has_active_slaves {
            // Only demote if it's actually still marked as master. It might have been demoted by another recursive call.
            let should_demote_this_master = local_states.get(&master_to_check_id)
                .map_or(false, |s| s.is_master);

            if should_demote_this_master {
                 log::info!("Audio Thread: Former master '{}' (of a now-disabled slave) no longer has other active slaves. Disabling its master status.", master_to_check_id);
                 audio_thread_handle_disable_sync(&master_to_check_id, local_states, app_handle)?;
            }
        }
    }
    Ok(())
}

/// Calculates PLL pitch corrections for synced slave decks
pub(crate) fn calculate_pll_pitch_updates(
    local_states: &HashMap<String, AudioThreadDeckState>,
    deck_times: &HashMap<String, (f64, bool)>,
) -> Result<HashMap<String, (f32, f32)>, crate::audio::errors::PlaybackError> {
    let mut corrections = HashMap::new();
    
    for (deck_id, deck_state) in local_states {
        let is_synced_and_playing = deck_state.is_sync_active 
            && deck_state.is_playing.lock().map(|v| *v).unwrap_or(false);
        if !is_synced_and_playing { continue; }
        let Some(master_id) = &deck_state.master_deck_id else { continue; };
        let Some(slave_bpm) = deck_state.original_bpm else { continue; };
        let Some(slave_fbs) = deck_state.first_beat_sec else { continue; };
        let Some(slave_time) = deck_times.get(deck_id).map(|(t, _)| *t) else { continue; };
        
        let Some(master_state) = local_states.get(master_id) else { continue; };
        let Some(master_bpm) = master_state.original_bpm else { continue; };
        let Some(master_fbs) = master_state.first_beat_sec else { continue; };
        let Some(master_time) = deck_times.get(master_id).map(|(t, _)| *t) else { continue; };
        
        let master_playing = master_state.is_playing.lock().map(|v| *v).unwrap_or(false);
        if !master_playing { continue; }
        
        // Calculate phases using original BPM intervals
        let master_beat_interval = 60.0 / master_bpm as f64;
        let slave_beat_interval = 60.0 / slave_bpm as f64;
        let master_phase = ((master_time - master_fbs as f64).max(0.0) / master_beat_interval) % 1.0;
        let slave_phase = ((slave_time - slave_fbs as f64).max(0.0) / slave_beat_interval) % 1.0;
        
        // Calculate phase error with wrapping
        let mut error = slave_phase - master_phase;
        if error > 0.5 { error -= 1.0; }
        if error < -0.5 { error += 1.0; }
        
        let correction = error as f32 * PLL_KP;
        corrections.insert(deck_id.clone(), (correction, error as f32));
    }
    
    Ok(corrections)
} 