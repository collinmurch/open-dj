use super::*;

pub(crate) fn audio_thread_handle_play<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let state = local_states
        .get_mut(deck_id)
        .ok_or_else(|| PlaybackError::DeckNotFound {
            deck_id: deck_id.to_string(),
        })?;
    if state.cpal_stream.is_none() {
        log::warn!(
            "Audio Thread: Play ignored for deck '{}', no CPAL stream (track not loaded?).",
            deck_id
        );
        emit_error_event(app_handle, deck_id, "Cannot play: Track not loaded.");
        return Ok(());
    }
    if state.decoded_samples.is_empty() {
        log::warn!(
            "Audio Thread: Play ignored for deck '{}', decoded samples are empty.",
            deck_id
        );
        emit_error_event(app_handle, deck_id, "Cannot play: Track data is empty.");
        return Ok(());
    }
    state
        .cpal_stream
        .as_ref()
        .unwrap()
        .play()
        .map_err(PlaybackError::CpalPlayStreamError)?;
    
    let paused_position = state.paused_position_read_head.load(Ordering::Relaxed);
    if paused_position > 0.0 {
        state.current_sample_read_head.store(paused_position, Ordering::Relaxed);
        log::info!("Audio Thread: Restored read head for deck '{}' from paused position: {:.2}", deck_id, paused_position);
    }

    state.is_playing.store(true, Ordering::Relaxed);
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
    log::info!("Audio Thread: Playing deck '{}' via CPAL", deck_id);
    
    // If this is deck B, start cue output
    if deck_id == "B" {
        if let Err(e) = start_deck_b_cue_output(local_states) {
            log::error!("Audio Thread: Failed to start cue output for deck B: {}", e);
        }
    }
    
    emit_status_update_event(app_handle, deck_id, true);
    Ok(())
}

pub(crate) fn audio_thread_handle_pause<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let state = local_states
        .get_mut(deck_id)
        .ok_or_else(|| PlaybackError::DeckNotFound {
            deck_id: deck_id.to_string(),
        })?;
    state.is_playing.store(false, Ordering::Relaxed);
    if state.cpal_stream.is_none() {
        log::warn!(
            "Audio Thread: Pause ignored for deck '{}', no CPAL stream.",
            deck_id
        );
        emit_status_update_event(app_handle, deck_id, false);
        return Ok(());
    }
    state
        .cpal_stream
        .as_ref()
        .unwrap()
        .pause()
        .map_err(PlaybackError::CpalPauseStreamError)?;
    let current_idx = state.current_sample_read_head.load(Ordering::Relaxed);
    state.paused_position_read_head.store(current_idx, Ordering::Relaxed);
    log::info!(
        "Audio Thread: Paused deck '{}' via CPAL at sample {}",
        deck_id,
        current_idx
    );
    
    // If this is deck B, stop cue output
    if deck_id == "B" {
        if let Err(e) = stop_cue_output() {
            log::error!("Audio Thread: Failed to stop cue output for deck B: {}", e);
        }
    }
    
    emit_status_update_event(app_handle, deck_id, false);

    let any_deck_synced_or_master = local_states
        .values()
        .any(|s| s.is_sync_active || s.is_master);
    if any_deck_synced_or_master {
        log::info!("Deck '{}' paused - disabling sync for all decks", deck_id);
        let deck_ids: Vec<String> = local_states.keys().cloned().collect();
        for id in deck_ids {
            let _ = crate::audio::playback::sync::audio_thread_handle_disable_sync(
                &id,
                local_states,
                app_handle,
            );
        }
    }

    Ok(())
}

pub(crate) fn audio_thread_handle_seek<R: Runtime>(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    let state = local_states
        .get_mut(deck_id)
        .ok_or_else(|| PlaybackError::DeckNotFound {
            deck_id: deck_id.to_string(),
        })?;
    if state.decoded_samples.is_empty() || state.sample_rate == 0.0 {
        log::warn!(
            "Audio Thread: Seek ignored for deck '{}', no track loaded or invalid sample rate.",
            deck_id
        );
        return Ok(());
    }
    let total_samples = state.decoded_samples.len();
    let sample_rate_f64 = state.sample_rate as f64;
    let target_sample_float = position_seconds * sample_rate_f64;
    let mut target_sample_index = target_sample_float.round() as usize;
    if target_sample_index >= total_samples {
        log::warn!(
            "Audio Thread: Seek position {:.2}s (sample {}) beyond duration for deck '{}'. Clamping to end.",
            position_seconds,
            target_sample_index,
            deck_id
        );
        target_sample_index = total_samples.saturating_sub(1);
    } else {
        target_sample_index = target_sample_index.max(0);
    }
    state.current_sample_read_head.store(target_sample_index as f64, Ordering::Relaxed);
    *state.seek_fade_state.lock().map_err(|_| {
        PlaybackError::LogicalStateLockError(format!(
            "Failed to lock seek_fade_state for deck '{}'.",
            deck_id
        ))
    })? = Some(0.0);
    if !state.is_playing.load(Ordering::Relaxed) {
        state.paused_position_read_head.store(target_sample_index as f64, Ordering::Relaxed);
    }

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

    let inv_sample_rate = 1.0 / sample_rate_f64;
    let current_time_secs = target_sample_index as f64 * inv_sample_rate;
    emit_tick_event(app_handle, deck_id, current_time_secs);
    Ok(())
}