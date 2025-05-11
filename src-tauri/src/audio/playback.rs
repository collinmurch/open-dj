use rodio::{OutputStream, OutputStreamHandle, Sink, Source, buffer::SamplesBuffer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::mpsc;

use crate::audio::config::INITIAL_TRIM_GAIN;
use crate::audio::effects::EqSource;
use crate::audio::errors::PlaybackError;
use crate::audio::types::{AudioThreadCommand, EqParams, PlaybackState};

// --- State Management ---

pub struct AppState {
    audio_command_sender: mpsc::Sender<AudioThreadCommand>,
    logical_playback_states: Arc<Mutex<HashMap<String, PlaybackState>>>,
    app_handle: AppHandle,
}

impl AppState {
    pub fn new(sender: mpsc::Sender<AudioThreadCommand>, app_handle: AppHandle) -> Self {
        AppState {
            audio_command_sender: sender,
            logical_playback_states: Arc::new(Mutex::new(HashMap::new())),
            app_handle,
        }
    }
}

struct AudioThreadDeckState {
    sink: Sink,
    decoded_samples: Arc<Vec<f32>>,
    sample_rate: f32,
    playback_start_time: Option<Instant>,
    paused_position: Option<Duration>,
    duration: Duration,
    is_playing: bool,
    eq_params: Arc<Mutex<EqParams>>,
    trim_gain: Arc<Mutex<f32>>,
    cue_point: Option<Duration>,
    current_pitch_rate: Arc<Mutex<f32>>,
}

// --- Event Emitter Helpers ---

fn emit_state_update<R: Runtime>(app_handle: &AppHandle<R>, deck_id: &str, state: &PlaybackState) {
    let payload = crate::audio::types::PlaybackUpdateEventPayload {
        deck_id: deck_id.to_string(),
        state: state.clone(),
    };
    if let Err(e) = app_handle.emit("playback://update", payload) {
        log::error!("Failed to emit playback://update for {}: {}", deck_id, e);
    }
}

fn emit_tick_event<R: Runtime>(app_handle: &AppHandle<R>, deck_id: &str, current_time: f64) {
    let event_payload = crate::audio::types::PlaybackTickEventPayload {
        deck_id: deck_id.to_string(),
        current_time,
    };
    if let Err(e) = app_handle.emit("playback://tick", event_payload) {
        log::warn!("Failed to emit playback://tick for {}: {}", deck_id, e);
    }
}

fn emit_error_event<R: Runtime>(app_handle: &AppHandle<R>, deck_id: &str, error_message: &str) {
    let payload = crate::audio::types::PlaybackErrorEventPayload {
        deck_id: deck_id.to_string(),
        error: error_message.to_string(),
    };
    if let Err(e) = app_handle.emit("playback://error", payload) {
        log::error!("Failed to emit playback://error for {}: {}", deck_id, e);
    }
}

// --- Logical State Update ---
fn update_logical_state(
    logical_states_arc: &Arc<Mutex<HashMap<String, PlaybackState>>>,
    deck_id: &str,
    new_state: PlaybackState,
) {
    let mut states = logical_states_arc.lock().unwrap_or_else(|poisoned| {
        log::error!("Logical playback states mutex was poisoned! Recovering.");
        poisoned.into_inner()
    });
    states.insert(deck_id.to_string(), new_state);
}

// --- Audio Thread Implementation ---

pub fn run_audio_thread(app_handle: AppHandle, mut receiver: mpsc::Receiver<AudioThreadCommand>) {
    // ... (no changes in the beginning of this function)
    log::info!("Audio Thread: Starting...");

    log::info!("Audio Thread: Calling OutputStream::try_default()...");
    let (_stream, handle) = match OutputStream::try_default() {
        Ok(tuple) => tuple,
        Err(e) => {
            log::error!(
                "Audio Thread: Failed to get output stream: {}. Thread exiting.",
                e
            );
            return;
        }
    };
    log::info!("Audio Thread: Stream and Handle obtained.");

    let mut local_deck_states: HashMap<String, AudioThreadDeckState> = HashMap::new();

    log::info!("Audio Thread: Building Tokio current_thread runtime...");
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            log::error!("Audio Thread: Failed to build Tokio runtime: {}", e);
            return;
        }
    };

    rt.block_on(async move {
        log::info!("Audio thread entering main loop.");
        let mut should_shutdown = false;
        let mut time_update_interval = tokio::time::interval(
            // Ensure this path is correct after moving config.rs
            Duration::from_millis(crate::audio::config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS)
        );

        while !should_shutdown {
            tokio::select! {
                maybe_command = receiver.recv() => {
                    match maybe_command {
                        Some(command) => {
                            log::debug!("Audio Thread Received: {:?}", command);
                            match command {
                                AudioThreadCommand::InitDeck(deck_id) => {
                                    audio_thread_handle_init(&deck_id, &mut local_deck_states, &handle, &app_handle);
                                }
                                AudioThreadCommand::LoadTrack { deck_id, path } => {
                                    audio_thread_handle_load(deck_id, path, &mut local_deck_states, &app_handle).await;
                                }
                                AudioThreadCommand::Play(deck_id) => {
                                    audio_thread_handle_play(&deck_id, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::Pause(deck_id) => {
                                    audio_thread_handle_pause(&deck_id, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::Seek { deck_id, position_seconds } => {
                                    audio_thread_handle_seek(&deck_id, position_seconds, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::SetFaderLevel { deck_id, level } => {
                                    audio_thread_handle_set_fader_level(&deck_id, level, &mut local_deck_states);
                                }
                                AudioThreadCommand::SetTrimGain { deck_id, gain } => {
                                    audio_thread_handle_set_trim_gain(&deck_id, gain, &mut local_deck_states);
                                }
                                AudioThreadCommand::SetEq { deck_id, params } => {
                                    audio_thread_handle_set_eq(&deck_id, params, &mut local_deck_states);
                                }
                                AudioThreadCommand::SetCue { deck_id, position_seconds } => {
                                    audio_thread_handle_set_cue(&deck_id, position_seconds, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::CleanupDeck(deck_id) => {
                                    audio_thread_handle_cleanup(&deck_id, &mut local_deck_states);
                                }
                                AudioThreadCommand::Shutdown(shutdown_complete_tx) => {
                                    log::info!("Audio Thread: Shutdown received. Cleaning up decks.");
                                    local_deck_states.clear();
                                    should_shutdown = true;
                                    if shutdown_complete_tx.send(()).is_err() {
                                         log::error!("Audio Thread: Failed to send shutdown completion signal.");
                                    }
                                }
                                AudioThreadCommand::SetPitchRate { deck_id, rate } => {
                                    audio_thread_handle_set_pitch_rate(&deck_id, rate, &mut local_deck_states, &app_handle);
                                }
                            }
                        }
                        None => {
                           log::info!("Audio Thread: Command channel closed. Exiting loop.");
                           should_shutdown = true;
                        }
                    }
                }
                _ = time_update_interval.tick(), if !should_shutdown => {
                    audio_thread_handle_time_update(&mut local_deck_states, &app_handle);
                }
            }
        }
        log::info!("Audio thread loop finished.");
    });
    log::info!("Audio thread has stopped.");
}

// --- Private Handler Functions for Audio Thread Commands ---

fn audio_thread_handle_init(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    audio_handle: &OutputStreamHandle,
    app_handle: &AppHandle,
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
            };
            local_states.insert(deck_id.to_string(), deck_state);
            log::info!("Audio Thread: Initialized deck '{}'", deck_id);

            let initial_playback_state = PlaybackState {
                pitch_rate: Some(1.0),
                ..PlaybackState::default()
            };
            let logical_states_arc = app_handle
                .state::<AppState>()
                .logical_playback_states
                .clone();
            update_logical_state(&logical_states_arc, deck_id, initial_playback_state.clone());
            emit_state_update(app_handle, deck_id, &initial_playback_state);
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
            let error_state = PlaybackState {
                error: Some(format!("Sink creation failed: {}", e)),
                ..Default::default()
            };
            let logical_states_arc = app_handle
                .state::<AppState>()
                .logical_playback_states
                .clone();
            update_logical_state(&logical_states_arc, deck_id, error_state.clone());
            emit_state_update(app_handle, deck_id, &error_state);
        }
    }
}

async fn audio_thread_handle_load(
    deck_id: String,
    path: String,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    log::debug!("Audio Thread: Handling LoadTrack for '{}'", deck_id);

    let logical_states_arc = app_handle
        .state::<AppState>()
        .logical_playback_states
        .clone();

    let initial_loading_state = PlaybackState {
        is_loading: true,
        ..logical_states_arc
            .lock()
            .unwrap()
            .get(&deck_id)
            .cloned()
            .unwrap_or_default()
    };
    update_logical_state(&logical_states_arc, &deck_id, initial_loading_state.clone());
    emit_state_update(app_handle, &deck_id, &initial_loading_state);

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
        crate::audio::decoding::decode_file_to_mono_samples(&path_clone)
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

                        let buffer = SamplesBuffer::new(
                            1,
                            rate as u32,
                            (*deck_state.decoded_samples).clone(),
                        );

                        match EqSource::new(buffer, eq_params_arc.clone(), trim_gain_arc.clone()) {
                            Ok(unwrapped_eq_source) => {
                                // Reset pitch rate for the new track to 1.0
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

                                let final_state = PlaybackState {
                                    is_playing: false,
                                    is_loading: false,
                                    current_time: 0.0,
                                    duration: Some(duration.as_secs_f64()),
                                    error: None,
                                    cue_point_seconds: None,
                                    pitch_rate: Some(1.0),
                                };
                                update_logical_state(
                                    &logical_states_arc,
                                    &deck_id,
                                    final_state.clone(),
                                );
                                emit_state_update(app_handle, &deck_id, &final_state);
                            }
                            Err(eq_creation_error) => {
                                let err_msg = format!(
                                    "Failed to create EQ source for deck '{}': {:?}",
                                    deck_id, eq_creation_error
                                );
                                log::error!("Audio Thread: {}", err_msg);

                                let error_state = PlaybackState {
                                    is_loading: false,
                                    error: Some(err_msg.clone()),
                                    ..Default::default()
                                };
                                update_logical_state(
                                    &logical_states_arc,
                                    &deck_id,
                                    error_state.clone(),
                                );
                                emit_state_update(app_handle, &deck_id, &error_state);
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

                        let error_state = PlaybackState {
                            is_loading: false,
                            error: Some(err_string.clone()),
                            ..Default::default()
                        };
                        update_logical_state(&logical_states_arc, &deck_id, error_state.clone());
                        emit_state_update(app_handle, &deck_id, &error_state);
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

            let error_state = PlaybackState {
                is_loading: false,
                error: Some(error_msg.clone()),
                ..Default::default()
            };
            update_logical_state(&logical_states_arc, &deck_id, error_state.clone());
            emit_state_update(app_handle, &deck_id, &error_state);

            if let Some(deck_state) = local_states.get_mut(&deck_id) {
                // Not positive we need to reset the decoded samples on error
                deck_state.decoded_samples = Arc::new(Vec::new());
            }
        }
    }
}

fn audio_thread_handle_play(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        if state.sink.empty() {
            log::warn!(
                "Audio Thread: Play ignored for deck '{}', sink is empty.",
                deck_id
            );
            let error_state = PlaybackState {
                error: Some("Cannot play: No track loaded or track is empty.".to_string()),
                ..Default::default()
            };
            let logical_states_arc = app_handle
                .state::<AppState>()
                .logical_playback_states
                .clone();
            update_logical_state(&logical_states_arc, deck_id, error_state.clone());
            emit_state_update(app_handle, deck_id, &error_state);
            return;
        }
        state.sink.play();
        state.is_playing = true;
        if state.paused_position.is_some() {
            // Resuming from pause
            state.playback_start_time = Some(Instant::now());
        } else {
            // Starting from beginning or after seek
            state.playback_start_time = Some(Instant::now());
        }
        log::info!("Audio Thread: Playing deck '{}'", deck_id);
        let logical_states_arc = app_handle
            .state::<AppState>()
            .logical_playback_states
            .clone();
        let current_time = get_current_playback_time_secs(state);
        let new_playback_state = PlaybackState {
            is_playing: true,
            current_time,
            duration: Some(state.duration.as_secs_f64()),
            is_loading: false,
            error: None,
            cue_point_seconds: state.cue_point.map(|d| d.as_secs_f64()),
            pitch_rate: Some(*state.current_pitch_rate.lock().unwrap()),
        };
        update_logical_state(&logical_states_arc, deck_id, new_playback_state.clone());
        emit_state_update(app_handle, deck_id, &new_playback_state);
    } else {
        log::error!("Audio Thread: Play: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for play operation.");
    }
}

fn audio_thread_handle_pause(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        state.sink.pause();
        state.is_playing = false;
        if let Some(start_time) = state.playback_start_time.take() {
            let elapsed_wall = Instant::now() - start_time;
            let current_rate = *state.current_pitch_rate.lock().unwrap();
            let audio_progress_secs = elapsed_wall.as_secs_f64() * current_rate as f64;
            let base_audio_time_secs = state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64();
            state.paused_position = Some(Duration::from_secs_f64((base_audio_time_secs + audio_progress_secs).min(state.duration.as_secs_f64())));
        }
        log::info!("Audio Thread: Paused deck '{}'", deck_id);
        let logical_states_arc = app_handle
            .state::<AppState>()
            .logical_playback_states
            .clone();
        let current_time = get_current_playback_time_secs(state);
        let new_playback_state = PlaybackState {
            is_playing: false,
            current_time,
            duration: Some(state.duration.as_secs_f64()),
            is_loading: false,
            error: None,
            cue_point_seconds: state.cue_point.map(|d| d.as_secs_f64()),
            pitch_rate: Some(*state.current_pitch_rate.lock().unwrap()),
        };
        update_logical_state(&logical_states_arc, deck_id, new_playback_state.clone());
        emit_state_update(app_handle, deck_id, &new_playback_state);
    } else {
        log::error!("Audio Thread: Pause: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for pause operation.");
    }
}

fn audio_thread_handle_seek(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        if state.sink.empty() || state.decoded_samples.is_empty() || state.sample_rate == 0.0 {
            log::warn!(
                "Audio Thread: Seek ignored for deck '{}', no track loaded or invalid state.",
                deck_id
            );
            let error_state = PlaybackState {
                error: Some("Cannot seek: No track loaded or track is invalid.".to_string()),
                ..Default::default()
            };
            let logical_states_arc = app_handle
                .state::<AppState>()
                .logical_playback_states
                .clone();
            update_logical_state(&logical_states_arc, deck_id, error_state.clone());
            emit_state_update(app_handle, deck_id, &error_state);
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
            state.duration // Clamp to end of track
        } else {
            seek_duration
        };

        // Recreate the source with EqSource and Speed to apply seek and pitch correctly
        let new_source = SamplesBuffer::new(1, state.sample_rate as u32, state.decoded_samples.to_vec());
        let eq_source = match EqSource::new(new_source, state.eq_params.clone(), state.trim_gain.clone()) {
            Ok(eq) => eq,
            Err(e) => {
                log::error!("Failed to create EqSource for seek: {:?}", e);
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

        state.paused_position = Some(final_seek_duration); // Update paused position to the seek target

        if state.is_playing {
            state.sink.play();
            state.playback_start_time = Some(Instant::now()); // Wall time for new segment
        } else {
            state.sink.pause();
            state.playback_start_time = None;
        }
        // ... update logical state, current_time should be final_seek_duration.as_secs_f64()
        let logical_states_arc = app_handle
            .state::<AppState>()
            .logical_playback_states
            .clone();
        let new_playback_state = PlaybackState {
            is_playing: state.is_playing,
            current_time: final_seek_duration.as_secs_f64(),
            duration: Some(state.duration.as_secs_f64()),
            is_loading: false,
            error: None,
            cue_point_seconds: state.cue_point.map(|d| d.as_secs_f64()),
            pitch_rate: Some(current_dynamic_rate),
        };
        update_logical_state(&logical_states_arc, deck_id, new_playback_state.clone());
        emit_state_update(app_handle, deck_id, &new_playback_state);
    } else {
        log::error!("Audio Thread: Seek: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for seek operation.");
    }
}

fn audio_thread_handle_set_fader_level(
    deck_id: &str,
    level: f32,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        let clamped_level = level.clamp(0.0, 1.0); // Ensure level is within 0-1 range
        state.sink.set_volume(clamped_level);
        log::debug!(
            "Audio Thread: Set fader level for deck '{}' to {}",
            deck_id,
            clamped_level
        );
    } else {
        log::warn!("Audio Thread: SetFaderLevel: Deck '{}' not found.", deck_id);
    }
}

fn audio_thread_handle_set_trim_gain(
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

fn audio_thread_handle_set_eq(
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

fn audio_thread_handle_set_cue(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        if state.duration == Duration::ZERO {
            log::warn!(
                "Audio Thread: SetCue ignored for deck '{}', track duration is zero (not loaded?).",
                deck_id
            );
            let error_state = PlaybackState {
                error: Some(
                    "Cannot set cue: Track not fully loaded or has no duration.".to_string(),
                ),
                ..Default::default()
            };
            let logical_states_arc = app_handle
                .state::<AppState>()
                .logical_playback_states
                .clone();
            update_logical_state(&logical_states_arc, deck_id, error_state.clone());
            emit_state_update(app_handle, deck_id, &error_state);

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

        let logical_states_arc = app_handle
            .state::<AppState>()
            .logical_playback_states
            .clone();
        let current_time = get_current_playback_time_secs(state); // get current time for accurate state update
        let new_playback_state = PlaybackState {
            is_playing: state.is_playing,
            current_time,
            duration: Some(state.duration.as_secs_f64()),
            is_loading: false,
            error: None,
            cue_point_seconds: Some(cue_duration.as_secs_f64()),
            pitch_rate: Some(1.0),
        };
        update_logical_state(&logical_states_arc, deck_id, new_playback_state.clone());
        emit_state_update(app_handle, deck_id, &new_playback_state);
    } else {
        log::error!("Audio Thread: SetCue: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for set cue operation.");
    }
}

fn get_current_playback_time_secs(deck_state: &AudioThreadDeckState) -> f64 {
    if deck_state.is_playing {
        if let Some(start_time) = deck_state.playback_start_time {
            let elapsed = start_time.elapsed();
            let current_rate = *deck_state.current_pitch_rate.lock().unwrap();
            let audio_advanced_secs = elapsed.as_secs_f64() * current_rate as f64;
            let base_audio_time_secs = deck_state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64();
            return (base_audio_time_secs + audio_advanced_secs).min(deck_state.duration.as_secs_f64());
        }
    } else if let Some(paused_pos) = deck_state.paused_position {
        return paused_pos
            .as_secs_f64()
            .min(deck_state.duration.as_secs_f64());
    }
    0.0
}

fn audio_thread_handle_time_update(
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    for (deck_id, deck_state) in local_states.iter_mut() {
        if deck_state.is_playing && !deck_state.sink.is_paused() && !deck_state.sink.empty() {
            let current_time_secs = get_current_playback_time_secs(deck_state);

            // Ensure config path is correct here
            let end_buffer = Duration::from_millis(
                crate::audio::config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS + 10,
            );
            let has_ended = deck_state.duration > Duration::ZERO
                && (Duration::from_secs_f64(current_time_secs) + end_buffer >= deck_state.duration);

            if has_ended {
                log::info!(
                    "Audio Thread: Track finished for '{}' based on time update.",
                    deck_id
                );
                deck_state.sink.pause();
                deck_state.is_playing = false;
                deck_state.playback_start_time = None;
                let final_time = deck_state.duration.as_secs_f64();
                deck_state.paused_position = Some(Duration::from_secs_f64(final_time));

                let state_update = PlaybackState {
                    current_time: final_time,
                    is_playing: false,
                    duration: Some(deck_state.duration.as_secs_f64()),
                    is_loading: false,
                    error: None,
                    cue_point_seconds: deck_state.cue_point.map(|d| d.as_secs_f64()),
                    pitch_rate: Some(1.0),
                };
                emit_state_update(app_handle, deck_id, &state_update);
            } else {
                emit_tick_event(app_handle, deck_id, current_time_secs);
            }
        }
    }
}

fn audio_thread_handle_cleanup(
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

fn audio_thread_handle_set_pitch_rate(
    deck_id: &str,
    rate: f32,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    if let Some(state) = local_states.get_mut(deck_id) {
        let clamped_rate = rate.clamp(0.5, 2.0);
        let old_rate: f32;
        {
            let mut rate_lock = state.current_pitch_rate.lock().unwrap();
            old_rate = *rate_lock;
            *rate_lock = clamped_rate;
        }
        // Calculate current playback position in audio seconds BEFORE changing speed
        let current_audio_time = get_current_playback_time_secs(state);
        // Update paused_position to the current audio time
        state.paused_position = Some(Duration::from_secs_f64(current_audio_time));
        // Set the new speed on the sink
        state.sink.set_speed(clamped_rate);
        // Reset playback_start_time for the new speed segment
        if state.is_playing {
            state.playback_start_time = Some(Instant::now());
        } else {
            state.playback_start_time = None;
        }
        log::info!("Audio Thread: Set pitch rate for deck '{}' to {}", deck_id, clamped_rate);

        let logical_states_arc = app_handle.state::<AppState>().logical_playback_states.clone();
        let mut locked_states_guard = logical_states_arc.lock().unwrap(); // LOCK ONCE here

        if let Some(logical_state_ref_mut) = locked_states_guard.get_mut(deck_id) {
            logical_state_ref_mut.pitch_rate = Some(clamped_rate);
            logical_state_ref_mut.current_time = current_audio_time;
            emit_state_update(app_handle, deck_id, logical_state_ref_mut);
        }

    } else {
        log::warn!("Audio Thread: SetPitchRate: Deck '{}' not found.", deck_id);
    }
}

// --- Tauri Commands ---

#[tauri::command]
pub async fn init_player(deck_id: String, app_state: State<'_, AppState>) -> Result<(), String> {
    log::info!("CMD: Init player for deck: {}", deck_id);
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::InitDeck(deck_id.clone()))
        .await
        .map_err(|e| {
            log::error!("Failed to send InitDeck command for {}: {}", deck_id, e);
            e.to_string()
        })?;

    // Initialize logical state
    let initial_state = PlaybackState {
        pitch_rate: Some(1.0),
        ..PlaybackState::default()
    };
    update_logical_state(&app_state.logical_playback_states, &deck_id, initial_state);
    Ok(())
}

#[tauri::command]
pub async fn load_track(
    deck_id: String,
    path: String,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("CMD: Load track '{}' for deck: {}", path, deck_id);

    let loading_state = PlaybackState {
        is_loading: true,
        pitch_rate: Some(1.0),
        ..PlaybackState::default()
    };
    update_logical_state(
        &app_state.logical_playback_states,
        &deck_id,
        loading_state.clone(),
    );

    emit_state_update(&app_state.app_handle, &deck_id, &loading_state);

    app_state
        .audio_command_sender
        .send(AudioThreadCommand::LoadTrack { deck_id, path })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn play_track(deck_id: String, app_state: State<'_, AppState>) -> Result<(), String> {
    log::info!("CMD: Play track for deck: {}", deck_id);
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::Play(deck_id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pause_track(deck_id: String, app_state: State<'_, AppState>) -> Result<(), String> {
    log::info!("CMD: Pause track for deck: {}", deck_id);
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::Pause(deck_id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn seek_track(
    deck_id: String,
    position_seconds: f64,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!(
        "CMD: Seek track for deck: {} to {}s",
        deck_id,
        position_seconds
    );
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::Seek {
            deck_id,
            position_seconds,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_playback_state(
    deck_id: String,
    app_state: State<'_, AppState>,
) -> Result<PlaybackState, String> {
    log::debug!("CMD: Get playback state for deck: {}", deck_id);
    let states = app_state
        .logical_playback_states
        .lock()
        .unwrap_or_else(|poisoned| {
            log::error!("CMD: GetPlaybackState: Mutex poisoned!");
            poisoned.into_inner()
        });
    states.get(&deck_id).cloned().ok_or_else(|| {
        log::warn!(
            "CMD: GetPlaybackState: No state found for deck '{}'",
            deck_id
        );
        format!("No playback state found for deck '{}'", deck_id)
    })
}

#[tauri::command]
pub async fn set_fader_level(
    deck_id: String,
    level: f32,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::debug!("CMD: Set fader level for deck {}: {}", deck_id, level);
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::SetFaderLevel { deck_id, level })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_trim_gain(
    deck_id: String,
    gain_db: f32,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::debug!("CMD: Set trim gain for deck {} to {} dB", deck_id, gain_db);
    // Convert dB to linear gain multiplier
    // Formula: gain_linear = 10^(gain_db / 20)
    // Ensure gain_db is not excessively negative to avoid issues with powf, though UI usually limits this.
    // A very small positive linear gain is better than zero if gain_db is extremely low.
    let linear_gain = if gain_db <= -96.0 {
        // effectively silence / very very quiet
        0.0
    } else {
        10.0f32.powf(gain_db / 20.0)
    };

    log::debug!(
        "CMD: Converted trim gain for deck {} from {} dB to {} linear",
        deck_id,
        gain_db,
        linear_gain
    );

    app_state
        .audio_command_sender
        .send(AudioThreadCommand::SetTrimGain {
            deck_id,
            gain: linear_gain,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_eq_params(
    deck_id: String,
    params: EqParams,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::debug!("CMD: Set EQ params for deck {}: {:?}", deck_id, params);
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::SetEq { deck_id, params })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_cue_point(
    deck_id: String,
    position_seconds: f64,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!(
        "CMD: Set cue point for deck {}: {}s",
        deck_id,
        position_seconds
    );
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::SetCue {
            deck_id,
            position_seconds,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cleanup_player(deck_id: String, app_state: State<'_, AppState>) -> Result<(), String> {
    log::info!("CMD: Cleanup player for deck: {}", deck_id);
    // Remove from logical state first
    {
        let mut states = app_state
            .logical_playback_states
            .lock()
            .unwrap_or_else(|poisoned| {
                log::error!("CMD: CleanupPlayer: Mutex poisoned!");
                poisoned.into_inner()
            });
        states.remove(&deck_id);
    }

    // Then send command to audio thread
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::CleanupDeck(deck_id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_pitch_rate(
    deck_id: String,
    rate: f32,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!("CMD: Set pitch rate for deck {}: {}", deck_id, rate);
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::SetPitchRate { deck_id, rate })
        .await
        .map_err(|e| e.to_string())
}
