use super::*;

pub(crate) fn audio_thread_handle_init<R: Runtime>(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) -> Result<(), PlaybackError> {
    if local_states.contains_key(deck_id) {
        log::warn!(
            "Audio Thread: InitDeck: Deck '{}' already exists. No action taken.",
            deck_id
        );
        emit_error_event(app_handle, deck_id, "Deck already initialized.");
        return Ok(());
    }

    let initial_eq_params = EqParams::default();
    let initial_current_eq_params_shared = Arc::new(Mutex::new(initial_eq_params.clone()));
    let initial_target_eq_params_shared = Arc::new(Mutex::new(initial_eq_params.clone()));

    let initial_linear_trim_gain = INITIAL_TRIM_GAIN;
    let initial_pitch_val = 1.0f32;

    let placeholder_sr = 44100.0;
    let default_coeffs = effects::calculate_low_shelf(placeholder_sr, 0.0).unwrap_or_else(|e| {
        log::warn!(
            "Failed to create default low_shelf coeffs: {}. Using default flat Coefficients.",
            e
        );
        biquad::Coefficients {
            a1: 0.0,
            a2: 0.0,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
        }
    });

    let low_shelf_filter = Arc::new(Mutex::new(DirectForm1::<f32>::new(default_coeffs)));
    let mid_peak_filter = Arc::new(Mutex::new(DirectForm1::<f32>::new(
        effects::calculate_mid_peak(placeholder_sr, 0.0).unwrap_or(default_coeffs),
    )));
    let high_shelf_filter = Arc::new(Mutex::new(DirectForm1::<f32>::new(
        effects::calculate_high_shelf(placeholder_sr, 0.0).unwrap_or(default_coeffs),
    )));
    let last_eq_params = Arc::new(Mutex::new(EqParams::default()));

    let deck_state = AudioThreadDeckState {
        cpal_stream: None,
        decoded_samples: Arc::new(Vec::new()),
        sample_rate: 0.0,
        current_sample_read_head: Arc::new(AtomicF64::new(0.0)),
        paused_position_read_head: Arc::new(AtomicF64::new(0.0)),
        duration: Duration::ZERO,
        is_playing: Arc::new(AtomicBool::new(false)),
        current_eq_params: initial_current_eq_params_shared,
        target_eq_params: initial_target_eq_params_shared,
        current_trim_gain: Arc::new(AtomicF32::new(initial_linear_trim_gain)),
        target_trim_gain: Arc::new(AtomicF32::new(initial_linear_trim_gain)),
        cue_point: None,
        current_pitch_rate: Arc::new(AtomicF32::new(initial_pitch_val)),
        target_pitch_rate: Arc::new(AtomicF32::new(initial_pitch_val)),
        last_ui_pitch_rate: Some(1.0),
        original_bpm: None,
        first_beat_sec: None,
        is_sync_active: false,
        is_master: false,
        master_deck_id: None,
        target_pitch_rate_for_bpm_match: 1.0,
        manual_pitch_rate: 1.0,
        pll_integral_error: 0.0,
        low_shelf_filter,
        mid_peak_filter,
        high_shelf_filter,
        last_eq_params,
        cached_low_coeffs: Arc::new(Mutex::new(None)),
        cached_mid_coeffs: Arc::new(Mutex::new(None)),
        cached_high_coeffs: Arc::new(Mutex::new(None)),
        output_sample_rate: None,
        last_playback_instant: Arc::new(Mutex::new(None)),
        read_head_at_last_playback_instant: Arc::new(Mutex::new(None)),
        seek_fade_state: Arc::new(Mutex::new(None)),
        channel_fader_level: Arc::new(AtomicF32::new(1.0f32)),
        last_pitch_event_time: Arc::new(Mutex::new(None)),
        last_emit_frame: Arc::new(AtomicU64::new(0u64)),
    };
    local_states.insert(deck_id.to_string(), deck_state);
    log::info!("Audio Thread: Initialized deck '{}' for CPAL", deck_id);

    emit_load_update_event(app_handle, deck_id, 0.0, None, None, None);
    emit_status_update_event(app_handle, deck_id, false);
    emit_sync_status_update_event(app_handle, deck_id, false, false);
    emit_pitch_tick_event(app_handle, deck_id, 1.0);
    Ok(())
}

pub(crate) fn audio_thread_handle_cleanup(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) -> Result<(), PlaybackError> {
    if let Some(state) = local_states.remove(deck_id) {
        if let Some(stream) = state.cpal_stream {
            drop(stream);
        }
        log::info!("Audio Thread: Cleaned up deck '{}'", deck_id);
    } else {
        log::warn!("Audio Thread: Deck '{}' not found for cleanup", deck_id);
    }
    Ok(())
}