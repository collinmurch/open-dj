pub(crate) const PLL_KP: f32 = 0.001; // Further reduced from 0.002
pub(crate) const MAX_PLL_PITCH_ADJUSTMENT: f32 = 0.04; // Max +/- adjustment from PLL (e.g., 4%), increased from 0.01
pub(crate) const PLL_KI: f32 = 0.0015; // Increased from 0.0005, now larger than KP
pub(crate) const MAX_PLL_INTEGRAL_ERROR: f32 = 5.0; // Max accumulated error for I-term clamping

use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Runtime};

use super::state::AudioThreadDeckState;
use super::events::{emit_error_event, emit_sync_status_update_event, emit_pitch_tick_event};
use super::handlers::{audio_thread_handle_seek, audio_thread_handle_set_pitch_rate};
use super::time::get_current_playback_time_secs;

// --- Sync Handler Functions ---

pub(crate) fn audio_thread_handle_enable_sync<R: Runtime>(
    slave_deck_id: &str,
    master_deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    log::info!("Audio Thread: Handling EnableSync. Slave: {}, Master: {}", slave_deck_id, master_deck_id);

    let master_info = if let Some(master_state) = local_states.get(master_deck_id) {
        log::info!("AudioThread enable_sync [PRE-CHECK MASTER]: Checking master '{}'. Found BPM: {:?}", master_deck_id, master_state.original_bpm);
        if master_state.duration <= Duration::ZERO {
            log::warn!("Audio Thread: EnableSync: Master deck '{}' is not loaded (duration is zero).", master_deck_id);
            emit_error_event(app_handle, slave_deck_id, &format!("Master deck '{}' must be loaded to sync", master_deck_id));
            return;
        }
        if master_state.original_bpm.is_none() {
            log::warn!("Audio Thread: EnableSync: Master deck '{}' missing BPM metadata.", master_deck_id);
            emit_error_event(app_handle, slave_deck_id, &format!("Master deck '{}' missing BPM", master_deck_id));
            return;
        }
        Some((
            master_state.original_bpm.unwrap(),
            *master_state.current_pitch_rate.lock().unwrap(),
            master_state.is_playing
        ))
    } else {
        log::error!("Audio Thread: EnableSync: Master deck '{}' not found.", master_deck_id);
        emit_error_event(app_handle, slave_deck_id, &format!("Master deck '{}' not found", master_deck_id));
        return;
    };

    let (master_bpm, master_current_pitch, _master_is_playing) = match master_info {
        Some(info) => info,
        None => return,
    };

    // Step 1: Determine and store target rate for slave, update slave state flags
    let target_rate_for_slave = {
        if let Some(slave_state) = local_states.get_mut(slave_deck_id) {
            log::info!("AudioThread enable_sync [PRE-CHECK SLAVE]: Checking slave '{}'. Found BPM: {:?}", slave_deck_id, slave_state.original_bpm);
            if slave_state.original_bpm.is_none() {
                log::warn!("Audio Thread: EnableSync: Slave deck '{}' missing BPM metadata.", slave_deck_id);
                emit_error_event(app_handle, slave_deck_id, "Slave deck missing BPM");
                return;
            }
            let slave_bpm = slave_state.original_bpm.unwrap();
            let calculated_target_rate = if slave_bpm.abs() > 1e-6 {
                (master_bpm / slave_bpm) * master_current_pitch
            } else {
                log::warn!("Audio Thread: EnableSync: Slave BPM is zero for '{}'. Cannot calculate rate.", slave_deck_id);
                emit_error_event(app_handle, slave_deck_id, "Slave deck BPM is zero");
                return;
            };
            slave_state.is_sync_active = true;
            slave_state.is_master = false;
            slave_state.master_deck_id = Some(master_deck_id.to_string());
            slave_state.manual_pitch_rate = *slave_state.current_pitch_rate.lock().unwrap(); // Store pre-sync pitch
            slave_state.target_pitch_rate_for_bpm_match = calculated_target_rate;
            log::info!("Target BPM match rate for {}: {:.4}. Stored manual pitch: {:.4}", slave_deck_id, calculated_target_rate, slave_state.manual_pitch_rate);
            emit_sync_status_update_event(app_handle, slave_deck_id, true, false);
            calculated_target_rate
        } else {
            log::error!("Audio Thread: EnableSync: Slave deck '{}' not found for mutable access.", slave_deck_id);
            emit_error_event(app_handle, slave_deck_id, "Slave deck not found");
            return;
        }
    };

    // Step 2: Set master deck flags (if not already master)
    if slave_deck_id != master_deck_id {
        if let Some(master_state_mut) = local_states.get_mut(master_deck_id) {
            if !master_state_mut.is_master {
                log::info!("Setting deck '{}' as master.", master_deck_id);
                master_state_mut.is_master = true;
                master_state_mut.is_sync_active = false;
                master_state_mut.manual_pitch_rate = *master_state_mut.current_pitch_rate.lock().unwrap(); // Store its own pitch if it becomes master
                emit_sync_status_update_event(app_handle, master_deck_id, false, true);
            }
        } else {
            log::error!("EnableSync: Failed to get mutable master state '{}' after initial check?!", master_deck_id);
        }
    }

    // Step 3: Apply the target pitch rate to the slave deck (Major Adjustment)
    // This sets the slave's speed correctly and re-bases its time anchors.
    log::debug!("Sync Enable [Step 3]: Applying target pitch rate {:.4} to slave '{}' (Major Adjustment)", target_rate_for_slave, slave_deck_id);
    let slave_id_clone_for_pitch = slave_deck_id.to_string();
    audio_thread_handle_set_pitch_rate(&slave_id_clone_for_pitch, target_rate_for_slave, true, local_states, app_handle);
    log::debug!("Sync Enable [Step 3 End]: Finished applying target pitch rate to slave '{}'", slave_id_clone_for_pitch);


    // Step 4: Calculate initial seek for slave for phase alignment
    // Now that slave is at the correct target pitch, get its current time for phase calculation.
    log::debug!("Sync Enable [Step 4]: Calculating initial seek for slave '{}' for phase alignment", slave_deck_id);
    let slave_seek_target_time_secs = {
        // Master's current time and pitch are live
        let master_current_time = local_states.get(master_deck_id).map(|s| get_current_playback_time_secs(s)).unwrap_or(0.0);
        let (master_fbs, master_actual_pitch, master_bpm_val_opt) = local_states.get(master_deck_id)
            .map(|s| (s.first_beat_sec, *s.current_pitch_rate.lock().unwrap(), s.original_bpm))
            .unwrap_or((None, 1.0, None));

        // Slave's current time is live (after pitch set), its pitch is target_rate_for_slave (which is current_pitch_rate now)
        let slave_current_time_after_pitch_set = local_states.get(slave_deck_id).map(|s| get_current_playback_time_secs(s)).unwrap_or(0.0);
        let (slave_fbs, slave_actual_pitch, slave_bpm_val_opt) = local_states.get(slave_deck_id)
            .map(|s| (s.first_beat_sec, *s.current_pitch_rate.lock().unwrap(), s.original_bpm))
            .unwrap_or((None, 1.0, None));

        log::info!("Sync Enable [Debug FBS]: Master '{}' first_beat_sec: {:?}", master_deck_id, master_fbs);
        log::info!("Sync Enable [Debug FBS]: Slave '{}' first_beat_sec: {:?}", slave_deck_id, slave_fbs);
        log::info!("Sync Enable [Debug Times for Phase]: MTime={:.3}, STime(post-pitch)={:.3}", master_current_time, slave_current_time_after_pitch_set);
        log::info!("Sync Enable [Debug Pitches for Phase]: MPitch={:.4}, SPitch(actual)={:.4}, Expected Target Slave Pitch={:.4}", master_actual_pitch, slave_actual_pitch, target_rate_for_slave);


        if let (Some(m_fbs), Some(s_fbs), Some(m_bpm), Some(s_bpm)) = (master_fbs, slave_fbs, master_bpm_val_opt, slave_bpm_val_opt) {
            if m_bpm > 1e-6 && s_bpm > 1e-6 && master_actual_pitch.abs() > 1e-6 && slave_actual_pitch.abs() > 1e-6 {
                let master_effective_interval = (60.0 / m_bpm) / master_actual_pitch;
                let slave_effective_interval = (60.0 / s_bpm) / slave_actual_pitch; // Use actual current slave pitch

                let master_time_since_fbs = (master_current_time - m_fbs as f64).max(0.0);
                let slave_time_since_fbs = (slave_current_time_after_pitch_set - s_fbs as f64).max(0.0);
                
                let master_phase = (master_time_since_fbs / master_effective_interval as f64) % 1.0;
                let slave_phase = (slave_time_since_fbs / slave_effective_interval as f64) % 1.0;
                
                let phase_diff = master_phase - slave_phase;
                let wrapped_phase_diff = if phase_diff > 0.5 { phase_diff - 1.0 } else if phase_diff < -0.5 { phase_diff + 1.0 } else { phase_diff };
                
                let time_adjustment_secs = wrapped_phase_diff * slave_effective_interval as f64;
                // The seek target is relative to the slave's current time *after its pitch has been set*
                let calculated_seek_target = slave_current_time_after_pitch_set + time_adjustment_secs;
                
                log::info!("Beat Align {}: MTime={:.3}, STime(PostPitch)={:.3}, MPh={:.3}, SPh={:.3}, Diff={:.3}, Adjust={:.3}s, TargetSeek={:.3}s", slave_deck_id, master_current_time, slave_current_time_after_pitch_set, master_phase, slave_phase, wrapped_phase_diff, time_adjustment_secs, calculated_seek_target);
                Some(calculated_seek_target)
            } else { log::warn!("Beat Align Skip: Invalid BPM or pitch rate for phase calc. M_BPM: {:?}, S_BPM: {:?}, M_Pitch: {:.4}, S_Pitch: {:.4}", m_bpm, s_bpm, master_actual_pitch, slave_actual_pitch); None }
        } else { log::warn!("Beat Align Skip: Missing First Beat Sec or BPM for master or slave. M_FBS: {:?}, S_FBS: {:?}, M_BPM: {:?}, S_BPM: {:?}", master_fbs, slave_fbs, master_bpm_val_opt, slave_bpm_val_opt); None }
    };

    // Step 5: Apply the calculated seek for phase alignment
    if let Some(seek_target) = slave_seek_target_time_secs {
        log::debug!("Sync Enable [Step 5]: Applying phase alignment seek for slave '{}' to {:.3}s", slave_deck_id, seek_target);
        audio_thread_handle_seek(slave_deck_id, seek_target, local_states, app_handle);
        log::debug!("Sync Enable [Step 5 End]: Finished applying phase alignment seek for slave '{}'", slave_deck_id);
    } else {
        log::warn!("Sync Enable [Step 5 Skip]: Could not calculate beat alignment seek for '{}'. Syncing BPM only.", slave_deck_id);
    }
    log::info!("Sync Enable for {} to {} complete. Slave should now be at target pitch and phase aligned.", slave_deck_id, master_deck_id);
}

pub(crate) fn audio_thread_handle_disable_sync<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    log::info!("Audio Thread: Handling DisableSync for deck: {}", deck_id);
    if let Some(deck_state) = local_states.get_mut(deck_id) {
        if !deck_state.is_sync_active && !deck_state.is_master {
            log::warn!("DisableSync: Deck '{}' is not currently synced or master.", deck_id);
            return;
        }
        let was_master = deck_state.is_master;
        let pitch_to_restore = deck_state.manual_pitch_rate;
        deck_state.is_sync_active = false;
        deck_state.is_master = false;
        deck_state.master_deck_id = None;
        deck_state.target_pitch_rate_for_bpm_match = 1.0; // Reset this.
        deck_state.pll_integral_error = 0.0; // Reset integral error
        log::info!("Deck '{}' sync disabled. Restoring pitch to: {}", deck_id, pitch_to_restore);
        emit_sync_status_update_event(app_handle, deck_id, false, false); // is_synced = false, is_master = false

        let deck_id_clone = deck_id.to_string();
        audio_thread_handle_set_pitch_rate(&deck_id_clone, pitch_to_restore, false, local_states, app_handle);
        emit_pitch_tick_event(app_handle, &deck_id_clone, pitch_to_restore);


        if was_master {
            log::info!("Deck '{}' was master. Checking slaves...", deck_id);
            let slaves_to_disable: Vec<String> = local_states
                .iter()
                .filter(|(_id, state)| state.master_deck_id.as_deref() == Some(deck_id))
                .map(|(id, _)| id.clone())
                .collect();
            if !slaves_to_disable.is_empty() {
                log::info!("Disabling sync for former slaves of '{}': {:?}", deck_id, slaves_to_disable);
                for slave_id in slaves_to_disable {
                     audio_thread_handle_disable_sync(&slave_id, local_states, app_handle);
                }
            }
        }
    } else {
        log::error!("DisableSync: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for disable sync");
    }
}

pub(crate) fn calculate_pll_pitch_updates(
    local_states: &HashMap<String, AudioThreadDeckState>,
    decks_with_current_times: &HashMap<String, (f64, bool)>,
) -> HashMap<String, (f32, f32)> {
    let mut slave_pitch_info: HashMap<String, (f32, f32)> = HashMap::new();
    let deck_ids: Vec<String> = local_states.keys().cloned().collect();

    for deck_id in deck_ids {
        let is_slave_playing_and_synced = local_states.get(&deck_id).map_or(false, |s| s.is_sync_active && s.is_playing);

        if is_slave_playing_and_synced { 
            let slave_data_for_pll = if let Some(s_state) = local_states.get(&deck_id) {
                // --- Calculate a STABILIZED slave_current_time for phase calculation ---
                let stabilized_slave_current_time_for_pll = 
                    if s_state.is_playing {
                        if let Some(start_time_anchor) = s_state.playback_start_time {
                            let wall_clock_elapsed = start_time_anchor.elapsed();
                            // Use target_pitch_rate_for_bpm_match for this slave's time calculation for PLL
                            let audio_advanced_at_target_rate = wall_clock_elapsed.as_secs_f64() * s_state.target_pitch_rate_for_bpm_match as f64;
                            let base_pos_at_anchor = s_state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64();
                            (base_pos_at_anchor + audio_advanced_at_target_rate).min(s_state.duration.as_secs_f64())
                        } else { 
                            s_state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64()
                        }
                    } else { 
                        s_state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64()
                    };
                // --- END STABILIZED slave_current_time calculation ---
                Some(( 
                    s_state.master_deck_id.clone(),
                    s_state.original_bpm,
                    s_state.first_beat_sec,
                    s_state.target_pitch_rate_for_bpm_match,
                    stabilized_slave_current_time_for_pll // Use the stabilized time
                ))
            } else { None };

            if let Some((
                Some(master_id),
                Some(slave_bpm),
                Some(slave_fbs),
                target_bpm_match_rate,
                stabilized_slave_time // Renamed from previous Some(stabilized_slave_current_time_for_pll)
            )) = slave_data_for_pll {
                if let Some(master_state) = local_states.get(&master_id) {
                    if let (
                        Some(master_bpm_val),
                        Some(master_fbs_val),
                        Some(master_current_time_val_live) // Live master time from get_current_playback_time_secs
                    ) = (
                        master_state.original_bpm,
                        master_state.first_beat_sec,
                        decks_with_current_times.get(&master_id).map(|(t, _)| *t) // Master time is live
                    ) {
                        if master_bpm_val > 1e-6 && slave_bpm > 1e-6 && master_state.is_playing {
                            let master_current_pitch = *master_state.current_pitch_rate.lock().unwrap();
                            let master_effective_interval = (60.0 / master_bpm_val) / master_current_pitch;
                            
                            let slave_effective_interval_at_target_bpm_match = if target_bpm_match_rate.abs() > 1e-6 {
                                (60.0 / slave_bpm) / target_bpm_match_rate
                            } else {
                                log::warn!(
                                    "PLL Warning (sync.rs): Slave '{}' target BPM match rate is near zero. Using raw BPM interval.", 
                                    deck_id
                                );
                                60.0 / slave_bpm 
                            };

                            let master_time_since_fbs = (master_current_time_val_live - master_fbs_val as f64).max(0.0);
                            // USE THE STABILIZED SLAVE TIME HERE for slave_time_since_fbs:
                            let slave_time_since_fbs = (stabilized_slave_time - slave_fbs as f64).max(0.0); 
                            
                            let master_phase = (master_time_since_fbs / master_effective_interval as f64) % 1.0;
                            let slave_phase = (slave_time_since_fbs / slave_effective_interval_at_target_bpm_match as f64) % 1.0;
                            
                            // Corrected phase error definition
                            let phase_error = master_phase - slave_phase; // Master minus Slave
                            let signed_error = if phase_error > 0.5 {
                                phase_error - 1.0
                            } else if phase_error < -0.5 {
                                phase_error + 1.0
                            } else {
                                phase_error
                            };

                            // Corrected proportional correction (removed negation)
                            let proportional_correction = signed_error as f32 * PLL_KP;
                            
                            slave_pitch_info.insert(deck_id.clone(), (proportional_correction, signed_error as f32));

                            log::debug!(
                                "PLL CALC {}: M_BPM={:.2}, S_BPM={:.2}, M_FBS={:.3}, S_FBS={:.3}, M_PITCH={:.3}, S_TARGET_BPM_PITCH={:.3}, M_TIME(Master Live)={:.3}, S_TIME(Stabilized)={:.3}, M_EFF_INT={:.4}, S_EFF_INT(target)={:.4}, S_PHASE={:.3}, M_PHASE={:.3}, ERR={:.3}, SIGNED_ERR={:.3} CORR={:.4}",
                                deck_id, master_bpm_val, slave_bpm, master_fbs_val, slave_fbs, master_current_pitch, target_bpm_match_rate, 
                                master_current_time_val_live, stabilized_slave_time, master_effective_interval, slave_effective_interval_at_target_bpm_match, 
                                slave_phase, master_phase, phase_error, signed_error, proportional_correction
                            );
                        } else { log::trace!("PLL CALC Skip for {}: Master '{}' missing data (bpm, fbs, time) or not playing.", deck_id, master_id);}
                    } else { log::trace!("PLL CALC Skip for {}: Master deck '{}' data incomplete in decks_with_current_times.", deck_id, master_id);}
                } else { log::warn!("PLL CALC Skip: Master deck '{}' for slave '{}' not found in local_states.", master_id, deck_id); }
            } else { log::trace!("PLL CALC Skip: Slave '{}' missing critical data (master_id, own_bpm, own_fbs, own_current_time, or target_bpm_match_rate).", deck_id); }
        }
    }
    slave_pitch_info
} 