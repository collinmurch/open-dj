pub(crate) const PLL_KP: f32 = 0.001; // Reduced from 0.002
pub(crate) const MAX_PLL_PITCH_ADJUSTMENT: f32 = 0.04; // Max +/- adjustment from PLL (e.g., 4%), increased from 0.01
pub(crate) const PLL_KI: f32 = 0.0015; // Reduced from 0.003
pub(crate) const MAX_PLL_INTEGRAL_ERROR: f32 = 5.0; // Max accumulated error for I-term clamping

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

        // --- Phase 5: One-Shot Phase Alignment ---
        log::debug!(
            "EnableSync (Phase 5): Attempting one-shot phase alignment for slave '{}' to master '{}'",
            slave_deck_id_str, master_deck_id_str
        );

        let phase_alignment_params = {
            let master_s_opt = local_states.get(master_deck_id_str);
            let slave_s_opt = local_states.get(slave_deck_id_str);
            if let (Some(master_s), Some(slave_s)) = (master_s_opt, slave_s_opt) {
                let master_calculated_time = {
                    let head_pos = if *master_s.is_playing.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing for master deck '{}'.", master_deck_id_str)))? {
                        *master_s.current_sample_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_sample_read_head for master deck '{}'.", master_deck_id_str)))?
                    } else {
                        master_s.paused_position_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock paused_position_read_head for master deck '{}'.", master_deck_id_str)))?.unwrap_or(0.0)
                    };
                    if master_s.sample_rate > 1e-6 {
                        (head_pos / master_s.sample_rate as f64)
                            .min(master_s.duration.as_secs_f64())
                            .max(0.0)
                    } else { 0.0 }
                };
                let slave_calculated_time = {
                    let head_pos = if *slave_s.is_playing.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing for slave deck '{}'.", slave_deck_id_str)))? {
                        *slave_s.current_sample_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_sample_read_head for slave deck '{}'.", slave_deck_id_str)))?
                    } else {
                        slave_s.paused_position_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock paused_position_read_head for slave deck '{}'.", slave_deck_id_str)))?.unwrap_or(0.0)
                    };
                    if slave_s.sample_rate > 1e-6 {
                        (head_pos / slave_s.sample_rate as f64)
                            .min(slave_s.duration.as_secs_f64())
                            .max(0.0)
                    } else { 0.0 }
                };
                Some((
                    (
                        master_calculated_time,
                        master_s.original_bpm,
                        master_s.first_beat_sec,
                        *master_s.target_pitch_rate.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock target_pitch_rate for master deck '{}'.", master_deck_id_str)))?
                    ),
                    (
                        slave_calculated_time,
                        slave_s.original_bpm,
                        slave_s.first_beat_sec,
                        slave_s.target_pitch_rate_for_bpm_match,
                        slave_s.sample_rate,
                        *slave_s.is_playing.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock is_playing for slave deck '{}'.", slave_deck_id_str)))?
                    )
                ))
            } else {
                None
            }
        };

        if let Some(((master_current_time_secs, m_bpm_opt, m_fbs_opt, master_pitch),
                      (slave_current_time_secs, s_bpm_opt, s_fbs_opt, slave_pitch, slave_sample_rate_val, slave_is_playing_val))) = phase_alignment_params {
            log::trace!(
                "PhaseAlign CALC INPUTS Master ('{}'): CurrentTimeS {:.4}, BPM {:?}, FBS {:?}, Pitch {:.4}",
                master_deck_id_str, master_current_time_secs, m_bpm_opt, m_fbs_opt, master_pitch
            );
            log::trace!(
                "PhaseAlign CALC INPUTS Slave ('{}'): CurrentTimeS {:.4}, BPM {:?}, FBS {:?}, TargetPitch {:.4}, SampleRate {}, IsPlaying {}",
                slave_deck_id_str, slave_current_time_secs, s_bpm_opt, s_fbs_opt, slave_pitch, slave_sample_rate_val, slave_is_playing_val
            );
            if let (Some(m_bpm), Some(m_fbs), Some(s_bpm), Some(s_fbs)) = (
                m_bpm_opt,
                m_fbs_opt,
                s_bpm_opt,
                s_fbs_opt,
            ) {
                if m_bpm.abs() > 1e-6 && s_bpm.abs() > 1e-6 && master_pitch.abs() > 1e-6 && slave_pitch.abs() > 1e-6 && slave_sample_rate_val > 0.0 {
                    let master_effective_interval = (60.0 / m_bpm) / master_pitch;
                    let slave_effective_interval = (60.0 / s_bpm) / slave_pitch;
                    let master_time_since_fbs = (master_current_time_secs - m_fbs as f64).max(0.0);
                    let slave_time_since_fbs = (slave_current_time_secs - s_fbs as f64).max(0.0);
                    let master_phase = (master_time_since_fbs / master_effective_interval as f64) % 1.0;
                    let slave_phase = (slave_time_since_fbs / slave_effective_interval as f64) % 1.0;
                    let mut phase_diff = master_phase - slave_phase;
                    if phase_diff > 0.5 { phase_diff -= 1.0; }
                    else if phase_diff < -0.5 { phase_diff += 1.0; }
                    let time_adjustment_secs = phase_diff * slave_effective_interval as f64;
                    let sample_adjustment_f64 = time_adjustment_secs * slave_sample_rate_val as f64;
                    // --- PATCH: Only apply micro-seek if adjustment is significant ---
                    const PHASE_ADJUSTMENT_THRESHOLD_SECS: f64 = 0.03; // 30ms
                    if sample_adjustment_f64.abs() > PHASE_ADJUSTMENT_THRESHOLD_SECS * slave_sample_rate_val as f64 {
                        if let Some(slave_deck_state_mut_for_seek) = local_states.get_mut(slave_deck_id_str) {
                            let old_read_head = *slave_deck_state_mut_for_seek.current_sample_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_sample_read_head for slave deck '{}'.", slave_deck_id_str)))?;
                            let new_read_head = old_read_head + sample_adjustment_f64;
                            *slave_deck_state_mut_for_seek.current_sample_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock current_sample_read_head for slave deck '{}'.", slave_deck_id_str)))? = new_read_head.max(0.0);
                            if !slave_is_playing_val {
                                *slave_deck_state_mut_for_seek.paused_position_read_head.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock paused_position_read_head for slave deck '{}'.", slave_deck_id_str)))? = Some(new_read_head.max(0.0));
                            }
                            *slave_deck_state_mut_for_seek.last_playback_instant.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock last_playback_instant for slave deck '{}'.", slave_deck_id_str)))? = None;
                            *slave_deck_state_mut_for_seek.read_head_at_last_playback_instant.lock().map_err(|_| crate::audio::errors::PlaybackError::LogicalStateLockError(format!("Failed to lock read_head_at_last_playback_instant for slave deck '{}'.", slave_deck_id_str)))? = None;
                            log::info!(
                                "EnableSync (Phase 5): Slave '{}' phase micro-seek. MPh: {:.3}, SPh: {:.3}, Diff: {:.3}, TAdj: {:.3}s, SmpAdj: {:.2}. RH: {:.2} -> {:.2}",
                                slave_deck_id_str, master_phase, slave_phase, phase_diff, time_adjustment_secs, sample_adjustment_f64, old_read_head, new_read_head
                            );
                            emit_tick_event(app_handle, slave_deck_id_str, new_read_head.max(0.0) / slave_sample_rate_val as f64);
                        } else {
                            log::warn!("EnableSync (Phase 5): Slave '{}' not found for micro-seek update.", slave_deck_id_str);
                        }
                    } else {
                        log::info!("EnableSync (Phase 5): Phase alignment skipped for '{}' (|adj| = {:.3}s, letting PLL handle fine sync)", slave_deck_id_str, time_adjustment_secs);
                    }
                    // --- END PATCH ---
                } else {
                    log::warn!("EnableSync (Phase 5): Invalid BPM, pitch, or sample rate for phase alignment. M_BPM: {}, S_BPM: {}, M_Pitch: {}, S_Pitch: {}, S_SR: {}", m_bpm, s_bpm, master_pitch, slave_pitch, slave_sample_rate_val);
                }
            } else {
                log::warn!("EnableSync (Phase 5): Missing BPM or FBS for phase alignment for master '{}' or slave '{}'", master_deck_id_str, slave_deck_id_str);
            }
        } else {
            log::warn!("EnableSync (Phase 5): Master or Slave state not found for phase alignment parameter extraction. Master: '{}', Slave: '{}'", master_deck_id_str, slave_deck_id_str);
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

/// Calculates PLL pitch updates for all synced slave decks.
/// Returns a map of deck_id to (proportional_correction, signed_error).
pub(crate) fn calculate_pll_pitch_updates(
    local_states: &HashMap<String, AudioThreadDeckState>,
    decks_with_current_times: &HashMap<String, (f64, bool)>,
) -> Result<HashMap<String, (f32, f32)>, crate::audio::errors::PlaybackError> {
    let mut slave_pitch_info: HashMap<String, (f32, f32)> = HashMap::new();
    let deck_ids: Vec<String> = local_states.keys().cloned().collect();
    for deck_id in deck_ids {
        let is_slave_playing_and_synced = local_states.get(&deck_id).map_or(false, |s| s.is_sync_active && s.is_playing.lock().map(|v| *v).unwrap_or(false));
        if is_slave_playing_and_synced {
            let slave_data_for_pll = if let Some(s_state) = local_states.get(&deck_id) {
                let live_slave_current_time_for_pll = decks_with_current_times.get(&deck_id).map(|(t, _)| *t);
                Some(( 
                    s_state.master_deck_id.clone(),
                    s_state.original_bpm,
                    s_state.first_beat_sec,
                    s_state.target_pitch_rate_for_bpm_match,
                    live_slave_current_time_for_pll
                ))
            } else { None };
            if let Some((
                Some(master_id),
                Some(slave_bpm),
                Some(slave_fbs),
                target_bpm_match_rate,
                Some(live_slave_time)
            )) = slave_data_for_pll {
                if let Some(master_state) = local_states.get(&master_id) {
                    if let (
                        Some(master_bpm_val),
                        Some(master_fbs_val),
                        Some(master_current_time_val_live)
                    ) = (
                        master_state.original_bpm,
                        master_state.first_beat_sec,
                        decks_with_current_times.get(&master_id).map(|(t, _)| *t)
                    ) {
                        let slave_actual_current_pitch = local_states.get(&deck_id)
                            .map(|s| s.current_pitch_rate.lock().map(|v| *v).unwrap_or(target_bpm_match_rate))
                            .unwrap_or(target_bpm_match_rate);
                        if master_bpm_val > 1e-6 && slave_bpm > 1e-6 && master_state.is_playing.lock().map(|v| *v).unwrap_or(false) && slave_actual_current_pitch.abs() > 1e-6 {
                            let master_current_pitch = master_state.current_pitch_rate.lock().map(|v| *v).unwrap_or(1.0);
                            let master_effective_interval = (60.0 / master_bpm_val) / master_current_pitch;
                            let slave_effective_interval_at_actual_pitch = if slave_actual_current_pitch.abs() > 1e-6 {
                                (60.0 / slave_bpm) / slave_actual_current_pitch
                            } else {
                                log::warn!(
                                    "PLL Warning (sync.rs): Slave '{}' actual current pitch is near zero. Using raw BPM interval for phase.", 
                                    deck_id
                                );
                                60.0 / slave_bpm 
                            };
                            let master_time_since_fbs = (master_current_time_val_live - master_fbs_val as f64).max(0.0);
                            let slave_time_since_fbs = (live_slave_time - slave_fbs as f64).max(0.0); 
                            let master_phase = (master_time_since_fbs / master_effective_interval as f64) % 1.0;
                            let slave_phase = (slave_time_since_fbs / slave_effective_interval_at_actual_pitch as f64) % 1.0;
                            let phase_error = master_phase - slave_phase;
                            let signed_error = if phase_error > 0.5 {
                                phase_error - 1.0
                            } else if phase_error < -0.5 {
                                phase_error + 1.0
                            } else {
                                phase_error
                            };
                            let proportional_correction = signed_error as f32 * PLL_KP;
                            slave_pitch_info.insert(deck_id.clone(), (proportional_correction, signed_error as f32));
                            log::debug!(
                                "PLL CALC {}: M_BPM={:.2}, S_BPM={:.2}, M_FBS={:.3}, S_FBS={:.3}, M_PITCH(actual)={:.3}, S_PITCH(actual)={:.3}, Target_S_PITCH={:.3}, M_TIME(Live)={:.3}, S_TIME(Live)={:.3}, M_EFF_INT={:.4}, S_EFF_INT(actual)={:.4}, S_PHASE={:.3}, M_PHASE={:.3}, ERR={:.3}, SIGNED_ERR={:.3} CORR={:.4}",
                                deck_id, master_bpm_val, slave_bpm, master_fbs_val, slave_fbs, 
                                master_current_pitch, slave_actual_current_pitch, target_bpm_match_rate, 
                                master_current_time_val_live, live_slave_time, 
                                master_effective_interval, slave_effective_interval_at_actual_pitch, 
                                slave_phase, master_phase, phase_error, signed_error, proportional_correction
                            );
                        } else { log::trace!("PLL CALC Skip for {}: Master '{}' missing data (bpm, fbs, time) or not playing, or slave actual pitch is zero.", deck_id, master_id);}
                    } else { log::trace!("PLL CALC Skip for {}: Master deck '{}' data incomplete in decks_with_current_times.", deck_id, master_id);}
                } else { log::warn!("PLL CALC Skip: Master deck '{}' for slave '{}' not found in local_states.", master_id, deck_id); }
            } else { log::trace!("PLL CALC Skip: Slave '{}' missing critical data (master_id, own_bpm, own_fbs, own_current_time, or target_bpm_match_rate).", deck_id); }
        }
    }
    Ok(slave_pitch_info)
} 