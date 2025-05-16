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
use super::time::get_current_playback_time_secs;

// --- Sync Handler Functions ---

// Make the main handler async
pub(crate) async fn audio_thread_handle_enable_sync_async<R: Runtime>(
    slave_deck_id_str: &str,
    master_deck_id_str: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>, 
    app_handle: &AppHandle<R>,
) {
    log::info!("Audio Thread: Handling EnableSync (Phase 4 - Tempo Sync). Slave: {}, Master: {}", slave_deck_id_str, master_deck_id_str);

    // --- Pre-checks for master deck ---
    let master_info = match local_states.get(master_deck_id_str) {
        Some(master_state) => {
            if master_state.duration <= Duration::ZERO {
                log::warn!("Audio Thread: EnableSync: Master deck '{}' is not loaded.", master_deck_id_str);
                emit_error_event(app_handle, slave_deck_id_str, &format!("Master deck '{}' must be loaded to sync", master_deck_id_str));
                return;
            }
            if master_state.original_bpm.is_none() {
                log::warn!("Audio Thread: EnableSync: Master deck '{}' missing BPM.", master_deck_id_str);
                emit_error_event(app_handle, slave_deck_id_str, &format!("Master deck '{}' missing BPM", master_deck_id_str));
                return;
            }
            Some((master_state.original_bpm.unwrap(), *master_state.target_pitch_rate.lock().unwrap()))
        }
        None => {
            log::error!("Audio Thread: EnableSync: Master deck '{}' not found.", master_deck_id_str);
            emit_error_event(app_handle, slave_deck_id_str, &format!("Master deck '{}' not found", master_deck_id_str));
            return;
        }
    };
    let (master_bpm, master_current_pitch) = master_info.unwrap(); // Known to be Some if we reached here

    // --- Pre-checks and setup for slave deck ---
    let calculated_target_rate_for_slave = {
        if let Some(slave_state) = local_states.get_mut(slave_deck_id_str) {
            if slave_state.original_bpm.is_none() {
                log::warn!("Audio Thread: EnableSync: Slave deck '{}' missing BPM.", slave_deck_id_str);
                emit_error_event(app_handle, slave_deck_id_str, "Slave deck missing BPM");
                return;
            }
            let slave_bpm = slave_state.original_bpm.unwrap();
            let target_rate = if slave_bpm.abs() > 1e-6 {
                (master_bpm / slave_bpm) * master_current_pitch
            } else {
                log::warn!("Audio Thread: EnableSync: Slave BPM is zero for '{}'. Cannot calculate rate.", slave_deck_id_str);
                emit_error_event(app_handle, slave_deck_id_str, "Slave deck BPM is zero");
                return;
            };

            slave_state.is_sync_active = true;
            slave_state.is_master = false; // A deck cannot be slave and master
            slave_state.master_deck_id = Some(master_deck_id_str.to_string());
            // Store the pitch rate that was active *before* sync engaged, for restoration on disable_sync
            slave_state.manual_pitch_rate = *slave_state.current_pitch_rate.lock().unwrap(); 
            slave_state.target_pitch_rate_for_bpm_match = target_rate;
            
            log::info!("Tempo Sync for '{}': Target rate {:.4}. Stored manual pitch: {:.4}", slave_deck_id_str, target_rate, slave_state.manual_pitch_rate);
            emit_sync_status_update_event(app_handle, slave_deck_id_str, true, false); // is_synced=true, is_master=false
            target_rate
        } else {
            log::error!("Audio Thread: EnableSync: Slave deck '{}' not found for mutable access.", slave_deck_id_str);
            emit_error_event(app_handle, slave_deck_id_str, "Slave deck not found");
            return;
        }
    };

    // --- Set master deck flags ---
    if slave_deck_id_str != master_deck_id_str { // Ensure a deck doesn't try to make itself master if it was the slave
        if let Some(master_state_mut) = local_states.get_mut(master_deck_id_str) {
            if !master_state_mut.is_master { // Only update if not already master
                log::info!("Setting deck '{}' as master.", master_deck_id_str);
                master_state_mut.is_master = true;
                master_state_mut.is_sync_active = false; // A master is not synced to another master_deck_id
                master_state_mut.master_deck_id = None;
                // Store its own pitch if it becomes master and wasn't before (might have been synced itself)
                master_state_mut.manual_pitch_rate = *master_state_mut.current_pitch_rate.lock().unwrap(); 
                emit_sync_status_update_event(app_handle, master_deck_id_str, false, true); // is_synced=false, is_master=true
            }
        } else {
            log::error!("EnableSync: Failed to get mutable master state '{}' after initial check.", master_deck_id_str);
            // This case should ideally not happen due to checks above.
        }
    }

    // --- Apply the target pitch rate to the slave deck (Tempo Matching) ---
    // This call is NOT user-initiated for the slave deck.
    audio_thread_handle_set_pitch_rate(
        slave_deck_id_str, 
        calculated_target_rate_for_slave, 
        false, // is_user_initiated_change = false
        local_states, 
        app_handle
    );

    log::info!(
        "EnableSync (Phase 4 - Tempo Sync) for slave '{}' to master '{}' complete. Slave is now tempo-matched.", 
        slave_deck_id_str, master_deck_id_str
    );

    // --- Phase 5: One-Shot Phase Alignment ---
    log::debug!(
        "EnableSync (Phase 5): Attempting one-shot phase alignment for slave '{}' to master '{}'",
        slave_deck_id_str, master_deck_id_str
    );

    // Borrow immutably first to get data for phase alignment
    let phase_alignment_params = {
        let master_s_opt = local_states.get(master_deck_id_str);
        let slave_s_opt = local_states.get(slave_deck_id_str);
        if let (Some(master_s), Some(slave_s)) = (master_s_opt, slave_s_opt) {
            Some((
                // Master data
                (
                    get_current_playback_time_secs(master_deck_id_str, master_s), 
                    master_s.original_bpm, 
                    master_s.first_beat_sec, 
                    *master_s.target_pitch_rate.lock().unwrap()
                ),
                // Slave data
                (
                    get_current_playback_time_secs(slave_deck_id_str, slave_s), 
                    slave_s.original_bpm, 
                    slave_s.first_beat_sec, 
                    slave_s.target_pitch_rate_for_bpm_match, // Use the just-set target rate
                    slave_s.sample_rate,
                    *slave_s.is_playing.lock().unwrap() // For updating paused_position_read_head
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
            // slave_pitch is already slave_state.target_pitch_rate_for_bpm_match from the tuple

            if m_bpm.abs() > 1e-6 && s_bpm.abs() > 1e-6 && master_pitch.abs() > 1e-6 && slave_pitch.abs() > 1e-6 && slave_sample_rate_val > 0.0 {
                let master_effective_interval = (60.0 / m_bpm) / master_pitch;
                let slave_effective_interval = (60.0 / s_bpm) / slave_pitch;

                let master_time_since_fbs = (master_current_time_secs - m_fbs as f64).max(0.0);
                let slave_time_since_fbs = (slave_current_time_secs - s_fbs as f64).max(0.0); // Use original slave_current_time_secs

                let master_phase = (master_time_since_fbs / master_effective_interval as f64) % 1.0;
                let slave_phase = (slave_time_since_fbs / slave_effective_interval as f64) % 1.0; // Slave interval for its own phase

                let mut phase_diff = master_phase - slave_phase;
                if phase_diff > 0.5 { phase_diff -= 1.0; }
                else if phase_diff < -0.5 { phase_diff += 1.0; }

                let time_adjustment_secs = phase_diff * slave_effective_interval as f64;
                let sample_adjustment_f64 = time_adjustment_secs * slave_sample_rate_val as f64;

                if sample_adjustment_f64.abs() > 0.001 * slave_sample_rate_val as f64 { // Only adjust if > 1ms
                    if let Some(slave_deck_state_mut_for_seek) = local_states.get_mut(slave_deck_id_str) {
                        let old_read_head = *slave_deck_state_mut_for_seek.current_sample_read_head.lock().unwrap();
                        let new_read_head = old_read_head + sample_adjustment_f64;
                        *slave_deck_state_mut_for_seek.current_sample_read_head.lock().unwrap() = new_read_head.max(0.0);
                        
                        if !slave_is_playing_val { // Use the captured slave_is_playing_val
                            *slave_deck_state_mut_for_seek.paused_position_read_head.lock().unwrap() = Some(new_read_head.max(0.0));
                        }

                        // Invalidate precise timing fields after micro-seek to ensure next time read is accurate
                        *slave_deck_state_mut_for_seek.last_playback_instant.lock().unwrap() = None;
                        *slave_deck_state_mut_for_seek.read_head_at_last_playback_instant.lock().unwrap() = None;

                        log::info!(
                            "EnableSync (Phase 5): Slave '{}' phase micro-seek. MPh: {:.3}, SPh: {:.3}, Diff: {:.3}, TAdj: {:.3}s, SmpAdj: {:.2}. RH: {:.2} -> {:.2}",
                            slave_deck_id_str, master_phase, slave_phase, phase_diff, time_adjustment_secs, sample_adjustment_f64, old_read_head, new_read_head
                        );
                        // Emit tick to reflect immediate change for UI
                        emit_tick_event(app_handle, slave_deck_id_str, new_read_head.max(0.0) / slave_sample_rate_val as f64);
                    } else {
                        log::warn!("EnableSync (Phase 5): Slave '{}' not found for micro-seek update.", slave_deck_id_str);
                    }
                } else {
                    log::info!("EnableSync (Phase 5): Phase alignment for '{}' too small, skipped. Diff: {:.3} ({:.3}s)", slave_deck_id_str, phase_diff, time_adjustment_secs);
                }
            } else {
                log::warn!("EnableSync (Phase 5): Invalid BPM, pitch, or sample rate for phase alignment. M_BPM: {}, S_BPM: {}, M_Pitch: {}, S_Pitch: {}, S_SR: {}", m_bpm, s_bpm, master_pitch, slave_pitch, slave_sample_rate_val);
            }
        } else {
            log::warn!("EnableSync (Phase 5): Missing BPM or FBS for phase alignment for master '{}' or slave '{}'", master_deck_id_str, slave_deck_id_str);
        }
    } else {
        log::warn!("EnableSync (Phase 5): Master or Slave state not found for phase alignment parameter extraction. Master: '{}', Slave: '{}'", master_deck_id_str, slave_deck_id_str);
    }
    // --- End Phase 5: One-Shot Phase Alignment ---
}

pub(crate) fn audio_thread_handle_disable_sync<R: Runtime>(
    deck_id_str: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    log::info!("Audio Thread: Handling DisableSync (Phase 4) for deck: {}", deck_id_str);
    let mut former_master_id: Option<String> = None;
    let pitch_to_restore_this_deck;

    if let Some(deck_state) = local_states.get_mut(deck_id_str) {
        if !deck_state.is_sync_active && !deck_state.is_master {
            log::warn!("DisableSync: Deck '{}' is not currently synced or master.", deck_id_str);
            return;
        }

        pitch_to_restore_this_deck = deck_state.manual_pitch_rate; // Get the pitch to restore
        if deck_state.is_master {
            former_master_id = Some(deck_id_str.to_string());
        }
        
        deck_state.is_sync_active = false;
        deck_state.is_master = false;
        deck_state.master_deck_id = None;
        deck_state.target_pitch_rate_for_bpm_match = 1.0;
        deck_state.pll_integral_error = 0.0;

        log::info!("Deck '{}' sync/master status disabled. Will restore its pitch to: {:.4}", deck_id_str, pitch_to_restore_this_deck);
        emit_sync_status_update_event(app_handle, deck_id_str, false, false);

    } else {
        log::error!("DisableSync: Deck '{}' not found.", deck_id_str);
        emit_error_event(app_handle, deck_id_str, "Deck not found for disable sync");
        return;
    }

    // Restore pitch on the deck that had disable_sync called on it
    // This is a user-initiated change because sync is being manually turned off for this deck.
    audio_thread_handle_set_pitch_rate(deck_id_str, pitch_to_restore_this_deck, true, local_states, app_handle);

    // If the disabled deck was a master, disable sync for all its slaves
    if let Some(master_id) = former_master_id {
        log::info!("Deck '{}' was master. Finding and disabling sync for its slaves...", master_id);
        let slaves_to_disable: Vec<String> = local_states
            .iter()
            .filter(|(_id, state)| state.master_deck_id.as_deref() == Some(&master_id))
            .map(|(id, _)| id.clone())
            .collect();

        if !slaves_to_disable.is_empty() {
            log::info!("Disabling sync for former slaves of '{}': {:?}", master_id, slaves_to_disable);
            for slave_id_str in slaves_to_disable {
                // This is a recursive call, but it processes different deck_ids. 
                // Ensure local_states is handled correctly if it leads to re-borrow issues.
                // For Phase 4, this structure should be okay as set_pitch_rate was simplified.
                audio_thread_handle_disable_sync(&slave_id_str, local_states, app_handle);
            }
        }
    }
}

pub(crate) fn calculate_pll_pitch_updates(
    local_states: &HashMap<String, AudioThreadDeckState>,
    decks_with_current_times: &HashMap<String, (f64, bool)>,
) -> HashMap<String, (f32, f32)> {
    let mut slave_pitch_info: HashMap<String, (f32, f32)> = HashMap::new();
    let deck_ids: Vec<String> = local_states.keys().cloned().collect();

    for deck_id in deck_ids {
        let is_slave_playing_and_synced = local_states.get(&deck_id).map_or(false, |s| s.is_sync_active && *s.is_playing.lock().unwrap());

        if is_slave_playing_and_synced { 
            let slave_data_for_pll = if let Some(s_state) = local_states.get(&deck_id) {
                // Get the SLAVE'S LIVE current time from decks_with_current_times, similar to master
                let live_slave_current_time_for_pll = decks_with_current_times.get(&deck_id).map(|(t, _)| *t);

                Some(( 
                    s_state.master_deck_id.clone(),
                    s_state.original_bpm,
                    s_state.first_beat_sec,
                    s_state.target_pitch_rate_for_bpm_match,
                    live_slave_current_time_for_pll // Use the LIVE slave time
                ))
            } else { None };

            if let Some((
                Some(master_id),
                Some(slave_bpm),
                Some(slave_fbs),
                target_bpm_match_rate,
                Some(live_slave_time) // Ensure live_slave_current_time_for_pll is Some
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
                        // Get the slave's actual current pitch rate for its own effective interval calculation
                        let slave_actual_current_pitch = local_states.get(&deck_id)
                            .map(|s| *s.current_pitch_rate.lock().unwrap())
                            .unwrap_or(target_bpm_match_rate); // Fallback to target if somehow unavailable

                        if master_bpm_val > 1e-6 && slave_bpm > 1e-6 && *master_state.is_playing.lock().unwrap() && slave_actual_current_pitch.abs() > 1e-6 {
                            let master_current_pitch = *master_state.current_pitch_rate.lock().unwrap();
                            let master_effective_interval = (60.0 / master_bpm_val) / master_current_pitch;
                            
                            // Slave effective interval based on its OWN ACTUAL current pitch rate
                            let slave_effective_interval_at_actual_pitch = if slave_actual_current_pitch.abs() > 1e-6 {
                                (60.0 / slave_bpm) / slave_actual_current_pitch
                            } else {
                                log::warn!(
                                    "PLL Warning (sync.rs): Slave '{}' actual current pitch is near zero. Using raw BPM interval for phase.", 
                                    deck_id // Should not happen if playing and synced
                                );
                                60.0 / slave_bpm 
                            };

                            let master_time_since_fbs = (master_current_time_val_live - master_fbs_val as f64).max(0.0);
                            // USE THE LIVE SLAVE TIME HERE for slave_time_since_fbs:
                            let slave_time_since_fbs = (live_slave_time - slave_fbs as f64).max(0.0); 
                            
                            let master_phase = (master_time_since_fbs / master_effective_interval as f64) % 1.0;
                            let slave_phase = (slave_time_since_fbs / slave_effective_interval_at_actual_pitch as f64) % 1.0;
                            
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
    slave_pitch_info
} 