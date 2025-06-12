use super::*;

pub(crate) fn audio_thread_handle_set_fader_level(
    deck_id: &str,
    level: f32,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) -> Result<(), PlaybackError> {
    let state = local_states
        .get_mut(deck_id)
        .ok_or_else(|| PlaybackError::DeckNotFound {
            deck_id: deck_id.to_string(),
        })?;
    let clamped_level = level.clamp(0.0, 1.0);
    state.channel_fader_level.store(clamped_level, Ordering::Relaxed);
    log::debug!(
        "Audio Thread: Set channel_fader_level for deck '{}' to {}",
        deck_id,
        clamped_level
    );
    Ok(())
}

pub(crate) fn audio_thread_handle_set_trim_gain(
    deck_id: &str,
    gain: f32,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) -> Result<(), PlaybackError> {
    let state = local_states
        .get_mut(deck_id)
        .ok_or_else(|| PlaybackError::DeckNotFound {
            deck_id: deck_id.to_string(),
        })?;
    state.target_trim_gain.store(gain, Ordering::Relaxed);
    log::debug!(
        "Audio Thread: Set target_trim_gain (linear) for deck '{}' to {}",
        deck_id,
        gain
    );
    Ok(())
}

pub(crate) fn audio_thread_handle_set_eq(
    deck_id: &str,
    new_params: EqParams,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) -> Result<(), PlaybackError> {
    let state = local_states
        .get_mut(deck_id)
        .ok_or_else(|| PlaybackError::DeckNotFound {
            deck_id: deck_id.to_string(),
        })?;
    *state.target_eq_params.lock().map_err(|_| {
        PlaybackError::LogicalStateLockError(format!(
            "Failed to lock target_eq_params for deck '{}'.",
            deck_id
        ))
    })? = new_params;
    log::debug!(
        "Audio Thread: Updated target_eq_params for deck '{}'",
        deck_id
    );
    Ok(())
}

pub(crate) fn audio_thread_handle_set_cue<R: Runtime>(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    _app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let state = local_states
        .get_mut(deck_id)
        .ok_or_else(|| PlaybackError::DeckNotFound {
            deck_id: deck_id.to_string(),
        })?;
    if state.duration == Duration::ZERO {
        log::warn!(
            "Audio Thread: SetCue ignored for deck '{}', track duration is zero (not loaded?).",
            deck_id
        );
        return Ok(());
    }
    let cue_duration =
        Duration::from_secs_f64(position_seconds.max(0.0).min(state.duration.as_secs_f64()));
    state.cue_point = Some(cue_duration);
    log::info!(
        "Audio Thread: Set cue point for deck '{}' to {:.2}s",
        deck_id,
        cue_duration.as_secs_f64()
    );
    Ok(())
}

pub(crate) fn audio_thread_handle_set_pitch_rate<R: Runtime>(
    deck_id: &str,
    rate: f32,
    is_user_initiated_change: bool,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let mut master_new_target_pitch_for_slaves: Option<f32> = None;
    let state = local_states
        .get_mut(deck_id)
        .ok_or_else(|| PlaybackError::DeckNotFound {
            deck_id: deck_id.to_string(),
        })?;
    let clamped_new_target_rate = rate.clamp(0.5, 2.0);
    let old_current_pitch_rate = state.current_pitch_rate.load(Ordering::Relaxed);
    if is_user_initiated_change {
        state.manual_pitch_rate = clamped_new_target_rate;
        if state.is_master {
            master_new_target_pitch_for_slaves = Some(clamped_new_target_rate);
        }
    }
    state.target_pitch_rate.store(clamped_new_target_rate, Ordering::Relaxed);
    if !is_user_initiated_change {
        state.current_pitch_rate.store(clamped_new_target_rate, Ordering::Relaxed);
        if (clamped_new_target_rate - old_current_pitch_rate).abs() > 1e-5 {
            *state.last_playback_instant.lock().map_err(|_| {
                PlaybackError::LogicalStateLockError(format!(
                    "Failed to lock last_playback_instant for deck '{}'.",
                    deck_id
                ))
            })? = None;
            *state
                .read_head_at_last_playback_instant
                .lock()
                .map_err(|_| {
                    PlaybackError::LogicalStateLockError(format!(
                        "Failed to lock read_head_at_last_playback_instant for deck '{}'.",
                        deck_id
                    ))
                })? = None;
            log::info!(
                "Audio Thread: Invalidated precise timing for deck '{}' due to system pitch change from {:.4} to {:.4}.",
                deck_id,
                old_current_pitch_rate,
                clamped_new_target_rate
            );
        }
        log::info!(
            "Audio Thread: Snapped current_pitch_rate for deck '{}' to {} (System-initiated change for sync/tempo).",
            deck_id,
            clamped_new_target_rate
        );
        emit_pitch_tick_event(app_handle, deck_id, clamped_new_target_rate);
        state.last_ui_pitch_rate = Some(clamped_new_target_rate);
        log::info!(
            "Audio Thread: Set target_pitch_rate and SNAPPED current_pitch_rate for deck '{}' to {} (System change).",
            deck_id,
            clamped_new_target_rate
        );
    } else {
        state.last_ui_pitch_rate = Some(clamped_new_target_rate);
        log::info!(
            "Audio Thread: Set target_pitch_rate for deck '{}' to {} (User initiated).",
            deck_id,
            clamped_new_target_rate
        );
    }
    if let Some(master_new_target_pitch) = master_new_target_pitch_for_slaves {
        let master_deck_id_str = deck_id.to_string();
        let master_original_bpm = local_states.get(deck_id).and_then(|s| s.original_bpm);
        if let Some(master_bpm) = master_original_bpm {
            let mut slave_updates: Vec<(String, f32)> = Vec::new();
            for (id, state) in local_states.iter() {
                if state.is_sync_active
                    && state.master_deck_id.as_deref() == Some(&master_deck_id_str)
                {
                    if let Some(slave_bpm) = state.original_bpm {
                        if slave_bpm.abs() > 1e-6 {
                            let new_target_rate_for_slave =
                                (master_bpm / slave_bpm) * master_new_target_pitch;
                            slave_updates.push((id.clone(), new_target_rate_for_slave));
                        }
                    }
                }
            }
            for (slave_id_str, new_target_rate_for_slave) in slave_updates {
                if let Some(slave_state) = local_states.get_mut(&slave_id_str) {
                    slave_state.target_pitch_rate_for_bpm_match = new_target_rate_for_slave;
                    slave_state.target_pitch_rate.store(new_target_rate_for_slave.clamp(0.5, 2.0), Ordering::Relaxed);
                    log::info!(
                        "Audio Thread: Master '{}' target pitch change, slave '{}' new target_pitch_rate: {:.4}",
                        master_deck_id_str,
                        slave_id_str,
                        new_target_rate_for_slave
                    );
                    emit_pitch_tick_event(
                        app_handle,
                        &slave_id_str,
                        new_target_rate_for_slave.clamp(0.5, 2.0),
                    );
                    slave_state.last_ui_pitch_rate =
                        Some(new_target_rate_for_slave.clamp(0.5, 2.0));
                } else {
                    log::warn!(
                        "Audio Thread: Slave '{}' not found during master pitch update propagation.",
                        slave_id_str
                    );
                }
            }
        } else {
            log::warn!(
                "Audio Thread: Master '{}' missing BPM, cannot update slave target pitches.",
                deck_id
            );
        }
    }
    Ok(())
}