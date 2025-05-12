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

// --- PLL Constants ---
const PLL_KP: f32 = 0.0075; // Proportional gain for phase correction (Increased from 0.005)
const MAX_PLL_PITCH_ADJUSTMENT: f32 = 0.01; // Max +/- adjustment from PLL (e.g., 1%)

// --- State Management ---

pub struct AppState {
    audio_command_sender: mpsc::Sender<AudioThreadCommand>,
    logical_playback_states: Arc<Mutex<HashMap<String, PlaybackState>>>,
}

impl AppState {
    pub fn new(sender: mpsc::Sender<AudioThreadCommand>) -> Self {
        AppState {
            audio_command_sender: sender,
            logical_playback_states: Arc::new(Mutex::new(HashMap::new())),
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
    // --- Sync Feature Fields ---
    original_bpm: Option<f32>,       // Added
    first_beat_sec: Option<f32>,     // Added
    is_sync_active: bool,            // Added (default: false)
    is_master: bool,                 // Added (default: false)
    master_deck_id: Option<String>,  // Added
    target_pitch_rate_for_bpm_match: f32, // Added (default: 1.0)
    manual_pitch_rate: f32,          // Added (default: 1.0)
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
                                AudioThreadCommand::LoadTrack { deck_id, path, original_bpm, first_beat_sec } => {
                                    audio_thread_handle_load(deck_id, path, original_bpm, first_beat_sec, &mut local_deck_states, &app_handle).await;
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
                                AudioThreadCommand::SetPitchRate { deck_id, rate, is_manual_adjustment } => {
                                    audio_thread_handle_set_pitch_rate(&deck_id, rate, is_manual_adjustment, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::EnableSync { slave_deck_id, master_deck_id } => {
                                    audio_thread_handle_enable_sync(&slave_deck_id, &master_deck_id, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::DisableSync { deck_id } => {
                                    audio_thread_handle_disable_sync(&deck_id, &mut local_deck_states, &app_handle);
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
                // --- Initialize Sync Fields ---
                original_bpm: None,                    // Added
                first_beat_sec: None,                // Added
                is_sync_active: false,               // Added
                is_master: false,                    // Added
                master_deck_id: None,                // Added
                target_pitch_rate_for_bpm_match: 1.0, // Added
                manual_pitch_rate: 1.0,              // Added
            };
            local_states.insert(deck_id.to_string(), deck_state);
            log::info!("Audio Thread: Initialized deck '{}'", deck_id);

            let initial_playback_state = PlaybackState {
                pitch_rate: Some(1.0),
                is_sync_active: false, // Added
                is_master: false,      // Added
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
    original_bpm: Option<f32>,
    first_beat_sec: Option<f32>,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    // --- Logging Step 2 REMOVED ---
    // log::debug!(
    //     "AudioThread handle_load: Processing for Deck '{}', Path '{}', BPM: {:?}, FBS: {:?}",
    //     deck_id,
    //     path, // Path already available
    //     original_bpm,
    //     first_beat_sec
    // );

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

                        // --- Store Metadata and Reset Sync State ---
                        deck_state.original_bpm = original_bpm;      // Added
                        deck_state.first_beat_sec = first_beat_sec;  // Added
                        // --- ADDED LOG ---
                        log::info!(
                            "AudioThread handle_load [POST-SET]: Stored BPM: {:?}, FBS: {:?} for Deck '{}'",
                            deck_state.original_bpm,
                            deck_state.first_beat_sec,
                            deck_id
                        );
                        // --- END ADDED LOG ---
                        deck_state.is_sync_active = false;           // Added
                        deck_state.is_master = false;                // Added
                        deck_state.master_deck_id = None;            // Added
                        deck_state.target_pitch_rate_for_bpm_match = 1.0; // Added
                        deck_state.manual_pitch_rate = 1.0; // Reset manual pitch on load // Added

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
                                    is_sync_active: false, // Ensure this is emitted
                                    is_master: false,      // Ensure this is emitted
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
            is_sync_active: state.is_sync_active,
            is_master: state.is_master,
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
        } else {
            state.paused_position = Some(Duration::ZERO); // Assume starting from beginning if paused before play
        }

        let was_master = state.is_master;
        let was_slave = state.is_sync_active;

        log::info!("Audio Thread: Paused deck '{}', Paused Position: {:?}", deck_id, state.paused_position);

        // --- Emit State Update ---
        // Capture necessary info before potential recursive calls in disable_sync
        let current_time = state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64();
        let duration = state.duration.as_secs_f64();
        let cue_point = state.cue_point.map(|d| d.as_secs_f64());
        let current_pitch = *state.current_pitch_rate.lock().unwrap();

        let logical_states_arc = app_handle.state::<AppState>().logical_playback_states.clone();
        let new_playback_state = PlaybackState {
            is_playing: false,
            current_time,
            duration: Some(duration),
            is_loading: false,
            error: None,
            cue_point_seconds: cue_point,
            pitch_rate: Some(current_pitch),
            // State emitted BEFORE disable sync runs, reflects *paused* state
            is_sync_active: was_slave,
            is_master: was_master,
        };
        update_logical_state(&logical_states_arc, deck_id, new_playback_state.clone());
        emit_state_update(app_handle, deck_id, &new_playback_state);

        // --- Disable Sync Logic ---
        if was_master {
            // If master pauses, disable sync for all its slaves
            log::info!("Master deck '{}' paused. Disabling sync for its slaves.", deck_id);
            let master_id_str = deck_id.to_string(); // Clone needed for closure
            let slaves_to_disable: Vec<String> = local_states
                .iter()
                .filter(|(_id, s)| s.master_deck_id.as_deref() == Some(&master_id_str))
                .map(|(id, _)| id.clone())
                .collect();

            for slave_id in slaves_to_disable {
                log::debug!("Pausing master: Disabling sync for slave '{}'", slave_id);
                // Call disable_sync - needs mutable borrow of local_states
                // Use cloned app_handle if necessary
                audio_thread_handle_disable_sync(&slave_id, local_states, app_handle);
            }
        } else if was_slave {
            // If slave pauses, disable its own sync
            log::info!("Slave deck '{}' paused. Disabling its sync.", deck_id);
            audio_thread_handle_disable_sync(deck_id, local_states, app_handle);
        }
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
            is_sync_active: state.is_sync_active,
            is_master: state.is_master,
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
            pitch_rate: Some(*state.current_pitch_rate.lock().unwrap()),
            is_sync_active: state.is_sync_active,
            is_master: state.is_master,
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
    // --- Phase 1: Standard Time Update & End-of-Track Handling ---
    let mut decks_to_update: HashMap<String, (f64, bool)> = HashMap::new(); // Store (current_time, has_ended)

    for (deck_id, deck_state) in local_states.iter_mut() {
        if deck_state.is_playing && !deck_state.sink.is_paused() && !deck_state.sink.empty() {
            let current_time_secs = get_current_playback_time_secs(deck_state);

            // Ensure config path is correct here
            let end_buffer = Duration::from_millis(
                crate::audio::config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS + 10, // Use config path
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
                decks_to_update.insert(deck_id.clone(), (final_time, true)); // Mark as ended
            } else {
                // Store current time for potential tick event later (don't emit yet)
                 decks_to_update.insert(deck_id.clone(), (current_time_secs, false));
            }
        }
         // Also include non-playing decks for potential PLL target updates if they are master
         else if deck_state.is_master {
             // No time update needed, but ensures master state is available for slaves
         }
    }

    // --- Phase 2: PLL Calculations (Iterate again now that all times are calculated) ---
    let mut slave_pitch_updates: HashMap<String, f32> = HashMap::new();

    // Need to clone keys or collect info immutably first to satisfy borrow checker if modifying within loop
    let deck_ids: Vec<String> = local_states.keys().cloned().collect();

    for deck_id in deck_ids {
         // Re-borrow mutably inside the loop if necessary, or structure access differently
         // This approach requires careful state management to avoid borrow conflicts
         // Let's try accessing master state immutably first

        let is_slave = local_states.get(&deck_id).map_or(false, |s| s.is_sync_active && s.is_playing);

        if is_slave {
            let slave_state_snapshot = if let Some(s) = local_states.get(&deck_id) {
                 // Create a snapshot of necessary slave fields to avoid holding borrow
                 Some((
                     s.master_deck_id.clone(),
                     s.original_bpm,
                     s.first_beat_sec,
                     s.target_pitch_rate_for_bpm_match,
                     *s.current_pitch_rate.lock().unwrap(), // Current actual rate
                     decks_to_update.get(&deck_id).map(|(t, _)| *t) // Current slave time from Phase 1
                 ))
            } else { None };

            if let Some((
                Some(master_id),
                Some(slave_bpm),
                Some(slave_fbs),
                target_bpm_match_rate,
                _current_slave_rate, // We calculate the new rate, don't need the old one here
                Some(slave_current_time)
            )) = slave_state_snapshot {
                // Now get master state info (immutably)
                 if let Some(master_state) = local_states.get(&master_id) {
                    if let (
                        Some(master_bpm),
                        Some(master_fbs),
                        Some(master_current_time) // Get master's current time from Phase 1 map
                     ) = (
                        master_state.original_bpm,
                        master_state.first_beat_sec,
                        decks_to_update.get(&master_id).map(|(t, _)| *t)
                     ) {
                        if master_bpm > 1e-6 && slave_bpm > 1e-6 {
                            // --- Calculate Beat Intervals (seconds per beat) ---
                             // Account for master's current pitch rate affecting its perceived interval
                            let master_current_pitch = *master_state.current_pitch_rate.lock().unwrap();
                            let master_effective_interval = (60.0 / master_bpm) / master_current_pitch;

                            // Calculate slave's effective interval based on its *current* actual rate
                            // let slave_current_actual_pitch = *local_states.get(&deck_id).unwrap().current_pitch_rate.lock().unwrap(); // Re-borrow needed // OLD
                            // let slave_effective_interval = if slave_current_actual_pitch.abs() > 1e-6 { // OLD
                            //     (60.0 / slave_bpm) / slave_current_actual_pitch // OLD
                            // } else { // OLD
                            //      log::warn!("PLL Warning: Slave '{}' current pitch is near zero, using target rate for interval.", deck_id); // OLD
                            //      // Fallback to target rate if actual is zero (shouldn't happen often) // OLD
                            //      (60.0 / slave_bpm) / target_bpm_match_rate // OLD
                            // }; // OLD

                            // **CHANGE**: Use the target rate for BPM matching as the basis for the slave interval
                            let slave_effective_interval = if target_bpm_match_rate.abs() > 1e-6 {
                                (60.0 / slave_bpm) / target_bpm_match_rate
                            } else {
                                log::warn!("PLL Warning: Slave '{}' target BPM match rate is near zero, cannot calculate interval.", deck_id);
                                // Fallback to assuming original interval if target rate is unusable
                                60.0 / slave_bpm
                            };

                            // --- Calculate Phase Error ---
                            // Time elapsed since the first beat for master and slave
                            let master_time_since_fbs = (master_current_time - master_fbs as f64).max(0.0);
                            let slave_time_since_fbs = (slave_current_time - slave_fbs as f64).max(0.0);

                            // Current phase within each track's *own* effective beat interval
                            let master_phase = (master_time_since_fbs / master_effective_interval as f64) % 1.0;
                            let slave_phase = (slave_time_since_fbs / slave_effective_interval as f64) % 1.0; // Use slave's interval

                            let phase_error = slave_phase - master_phase;

                            // Wrap error to [-0.5, 0.5]
                            let signed_error = if phase_error > 0.5 {
                                phase_error - 1.0
                            } else if phase_error < -0.5 {
                                phase_error + 1.0
                            } else {
                                phase_error
                            };

                            // --- Calculate Pitch Correction ---
                            let pitch_correction = (PLL_KP * signed_error as f32)
                                .max(-MAX_PLL_PITCH_ADJUSTMENT)
                                .min(MAX_PLL_PITCH_ADJUSTMENT);

                            // --- Calculate Final Target Pitch ---
                            // Base rate needed to match BPM + phase correction
                            let final_target_pitch = target_bpm_match_rate + pitch_correction;

                            // Clamp final rate to reasonable limits (e.g., 0.5x to 2.0x)
                            let clamped_final_pitch = final_target_pitch.clamp(0.5, 2.0);

                            // Store the calculated pitch update for Phase 3 application
                            slave_pitch_updates.insert(deck_id.clone(), clamped_final_pitch);

                            // Update log for PLL to include slave effective interval
                            log::debug!(
                                "PLL {}: M_BPM={:.2}, S_BPM={:.2}, M_FBS={:.3}, S_FBS={:.3}, M_PITCH={:.3}, S_TARGET_BPM_PITCH={:.3}, M_TIME={:.3}, S_TIME={:.3}, M_EFF_INT={:.4}, S_EFF_INT(target)={:.4}, S_PHASE={:.3}, M_PHASE={:.3}, ERR={:.3}, CORR={:.4}, FINAL_PITCH={:.4}",
                                deck_id, master_bpm, slave_bpm, master_fbs, slave_fbs, master_current_pitch, target_bpm_match_rate, master_current_time, slave_current_time, master_effective_interval, slave_effective_interval, slave_phase, master_phase, signed_error, pitch_correction, clamped_final_pitch
                            );

                        }
                    } else {
                         log::trace!("PLL Skip: Master '{}' missing data (bpm, fbs, or current time).", master_id);
                    }
                 } else {
                      log::warn!("PLL Skip: Master deck '{}' for slave '{}' not found in local_states.", master_id, deck_id);
                      // Consider disabling sync for the slave here?
                 }
            } else {
                 log::trace!("PLL Skip: Slave '{}' missing data (master_id, bpm, fbs, or current time).", deck_id);
            }
        }
    }

    // --- Phase 3: Apply Updates (Pitch, State Emission) ---
    let logical_states_arc = app_handle.state::<AppState>().logical_playback_states.clone();

    for (deck_id, (current_time, has_ended)) in decks_to_update {
        if let Some(deck_state) = local_states.get_mut(&deck_id) { // Borrow mutably here
             let mut final_pitch_for_state = *deck_state.current_pitch_rate.lock().unwrap();

            // Apply PLL pitch update if calculated
            if let Some(&new_pitch) = slave_pitch_updates.get(&deck_id) {
                // Compare with current rate before applying to avoid redundant updates/logs
                 let old_rate = *deck_state.current_pitch_rate.lock().unwrap();
                 if (new_pitch - old_rate).abs() > 1e-5 { // Tolerance for float comparison
                    { // Scope for lock
                         let mut rate_lock = deck_state.current_pitch_rate.lock().unwrap();
                         *rate_lock = new_pitch;
                    }
                    deck_state.sink.set_speed(new_pitch); // Apply to sink
                     final_pitch_for_state = new_pitch; // Use the new pitch for the state update
                     // log::trace!("PLL Applied: Deck '{}' pitch set to {:.4}", deck_id, new_pitch);
                 }
            }

            // Update logical state and emit events
             let mut states = logical_states_arc.lock().unwrap();
             if let Some(logical_state) = states.get_mut(&deck_id) {
                 logical_state.current_time = current_time;
                 logical_state.pitch_rate = Some(final_pitch_for_state); // Reflect current actual pitch
                 logical_state.is_playing = deck_state.is_playing; // Update is_playing status
                 logical_state.is_sync_active = deck_state.is_sync_active; // Ensure sync flags are current
                 logical_state.is_master = deck_state.is_master;

                 // Emit full state update if pitch changed, track ended, or potentially periodically for safety
                 let pitch_changed = (logical_state.pitch_rate.unwrap_or(1.0) - final_pitch_for_state).abs() > 1e-5;

                 // Determine if a full update is warranted
                 let should_emit_full_update = has_ended || pitch_changed ||
                                              (deck_state.is_sync_active && deck_state.is_playing) || // Keep emitting for active slaves
                                              deck_state.is_master;
                                             // Add checks here if we track previous sync/master flags and they changed

                 if should_emit_full_update {
                     // Ensure is_playing reflects actual state IF track ended
                     if has_ended {
                        logical_state.is_playing = false;
                     }
                     let state_snapshot = logical_state.clone(); // Clone the corrected state
                     emit_state_update(app_handle, &deck_id, &state_snapshot);
                 } else if deck_state.is_playing { // Deck is playing but doesn't need full update (e.g., not synced)
                     // Emit only the tick event for performance
                     emit_tick_event(app_handle, &deck_id, current_time);
                     // Update logical state's time without emitting full state again
                     logical_state.current_time = current_time; // Already set above, but confirms intent
                 }
                 // If not playing and no other condition triggered full update, no event is emitted for this deck.
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
    is_manual_adjustment: bool,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    let mut is_slave_manual_override = false;

    if let Some(state) = local_states.get_mut(deck_id) {
        // --- Check for manual override on synced slave ---
        if state.is_sync_active && is_manual_adjustment {
            log::warn!(
                "Manual pitch adjustment received for synced slave '{}'. Disabling sync.",
                deck_id
            );
            is_slave_manual_override = true;
        }

        let clamped_new_rate = rate.clamp(0.5, 2.0);
        let old_rate: f32;

        // --- Store Manual Rate (only if manually adjusted) ---
        if is_manual_adjustment {
             state.manual_pitch_rate = clamped_new_rate;
             log::debug!("Storing manual pitch rate for {}: {}", deck_id, clamped_new_rate);
        }

        let current_true_audio_time_secs: f64;
        if state.is_playing {
            if let Some(start_time) = state.playback_start_time {
                let elapsed_since_last_start = start_time.elapsed();
                let rate_during_segment = *state.current_pitch_rate.lock().unwrap();
                
                let audio_advanced_this_segment_secs = elapsed_since_last_start.as_secs_f64() * rate_during_segment as f64;
                let base_audio_time_at_segment_start_secs = state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64();
                
                current_true_audio_time_secs = (base_audio_time_at_segment_start_secs + audio_advanced_this_segment_secs)
                                               .min(state.duration.as_secs_f64());
            } else { 
                current_true_audio_time_secs = state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64();
            }
        } else {
            current_true_audio_time_secs = state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64();
        }

        {
            let mut rate_lock = state.current_pitch_rate.lock().unwrap();
            old_rate = *rate_lock;
            *rate_lock = clamped_new_rate;
        }

        state.paused_position = Some(Duration::from_secs_f64(current_true_audio_time_secs));

        state.sink.set_speed(clamped_new_rate);

        if state.is_playing {
            state.playback_start_time = Some(Instant::now());
        } else {
            state.playback_start_time = None;
        }

        log::info!(
            "Audio Thread: Set pitch rate for deck '{}' from {} to {} at audio time {:.2}s",
            deck_id,
            old_rate,
            clamped_new_rate, // Use the final clamped rate for logging
            current_true_audio_time_secs
        );

        // --- Emit State Update (always happens) ---
        let logical_states_arc = app_handle.state::<AppState>().logical_playback_states.clone();
        let mut locked_states_guard = logical_states_arc.lock().unwrap();

        if let Some(logical_state_ref_mut) = locked_states_guard.get_mut(deck_id) {
            logical_state_ref_mut.pitch_rate = Some(clamped_new_rate);
            logical_state_ref_mut.current_time = current_true_audio_time_secs;
            // Preserve sync/master status unless overridden
            logical_state_ref_mut.is_sync_active = state.is_sync_active && !is_slave_manual_override;
            logical_state_ref_mut.is_master = state.is_master;

            // Preserve other fields
            logical_state_ref_mut.is_playing = state.is_playing;
            logical_state_ref_mut.duration = Some(state.duration.as_secs_f64());
            logical_state_ref_mut.is_loading = false;
            logical_state_ref_mut.error = None;
            logical_state_ref_mut.cue_point_seconds = state.cue_point.map(|d| d.as_secs_f64());

            emit_state_update(app_handle, deck_id, logical_state_ref_mut);
        } else {
            log::warn!("SetPitchRate: Logical state not found for deck '{}' during update emission.", deck_id);
        }
        // Release lock quickly
        drop(locked_states_guard);

        // --- Handle Slave Manual Override (disable sync after state updates) ---
        if is_slave_manual_override {
             // Call disable_sync now that other operations are complete
             let deck_id_clone = deck_id.to_string(); // Clone needed if disable borrows mut again
             audio_thread_handle_disable_sync(&deck_id_clone, local_states, app_handle);
             // Note: disable_sync will emit its own final state update for the deck
             return; // Avoid further processing in this function call
        }

        // --- Inform Slaves if Master Rate Changed ---
        if state.is_master {
            let master_bpm = state.original_bpm;
            if master_bpm.is_none() {
                log::warn!("Master deck '{}' changed rate but is missing BPM. Cannot update slaves.", deck_id);
                return;
            }
            let master_new_rate = clamped_new_rate;
            let master_id_str = deck_id.to_string();

            // Collect slave IDs to avoid borrowing issues while iterating
            let slave_ids: Vec<String> = local_states.iter()
                .filter(|(_id, s)| s.is_sync_active && s.master_deck_id.as_deref() == Some(deck_id))
                .map(|(id, _)| id.clone())
                .collect();

            log::debug!("Master '{}' changed rate. Updating targets for slaves: {:?}", master_id_str, slave_ids);

            for slave_id in slave_ids {
                 // Get slave state mutably within the loop
                 if let Some(slave_state) = local_states.get_mut(&slave_id) {
                     if let Some(slave_bpm) = slave_state.original_bpm {
                         if slave_bpm.abs() > 1e-6 {
                            let new_target_rate = (master_bpm.unwrap() / slave_bpm) * master_new_rate;
                            slave_state.target_pitch_rate_for_bpm_match = new_target_rate;
                             log::debug!("Updated target BPM match rate for slave '{}' to: {:.4}", slave_id, new_target_rate);
                             // The PLL will pick this up on the next tick and adjust the actual rate.
                         } else {
                              log::warn!("Cannot update target rate for slave '{}', its BPM is zero.", slave_id);
                         }
                     } else {
                         log::warn!("Cannot update target rate for slave '{}', missing BPM.", slave_id);
                     }
                 } else {
                      log::warn!("Failed to get mutable state for slave '{}' while updating targets.", slave_id);
                 }
            }
        }

    } else {
        log::warn!("Audio Thread: SetPitchRate: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for pitch rate adjustment.");
    }
}

// --- Placeholder Sync Handler Functions ---

fn audio_thread_handle_enable_sync(
    slave_deck_id: &str,
    master_deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    log::info!(
        "Audio Thread: Handling EnableSync. Slave: {}, Master: {}",
        slave_deck_id,
        master_deck_id
    );

    // --- Step 1: Get Master Info (Immutable) & Validate ---
    let master_info = if let Some(master_state) = local_states.get(master_deck_id) {
        // --- ADDED LOG ---
        log::info!(
            "AudioThread enable_sync [PRE-CHECK MASTER]: Checking master '{}'. Found BPM: {:?}",
            master_deck_id,
            master_state.original_bpm
        );
        // --- END ADDED LOG ---
        // --- Logging Step 4 REMOVED ---
        // log::debug!(
        //     "AudioThread enable_sync: Checking master '{}'. Found BPM: {:?}, FBS: {:?}, Playing: {}, Loaded: {}", 
        //     master_deck_id, 
        //     master_state.original_bpm, 
        //     master_state.first_beat_sec, 
        //     master_state.is_playing,
        //     master_state.duration > Duration::ZERO
        // );
        // Check if master is loaded (duration > 0)
        if master_state.duration <= Duration::ZERO {
            log::warn!(
                "Audio Thread: EnableSync: Master deck '{}' is not loaded (duration is zero).",
                master_deck_id
            );
            emit_error_event(
                app_handle,
                slave_deck_id,
                &format!("Master deck '{}' must be loaded to sync", master_deck_id),
            );
            return;
        }
        // Check if master has BPM
        if master_state.original_bpm.is_none() {
            log::warn!(
                "Audio Thread: EnableSync: Master deck '{}' missing BPM metadata.",
                master_deck_id
            );
            emit_error_event(
                app_handle,
                slave_deck_id,
                &format!("Master deck '{}' missing BPM", master_deck_id),
            );
            return;
        }
        Some((
            master_state.original_bpm.unwrap(), // Known Some
            *master_state.current_pitch_rate.lock().unwrap(),
            master_state.is_playing // Pass is_playing status
        ))
    } else {
        log::error!(
            "Audio Thread: EnableSync: Master deck '{}' not found.",
            master_deck_id
        );
        emit_error_event(
            app_handle,
            slave_deck_id,
            &format!("Master deck '{}' not found", master_deck_id),
        );
        return;
    };

    // Exit if master validation failed
    let (master_bpm, master_current_pitch, _master_is_playing) = match master_info {
        Some(info) => info,
        None => return, // Error already emitted
    };

    // --- Step 2: Get Slave Mutably, Validate, Calculate Rate, Set Initial Flags ---
    let target_rate = {
        if let Some(slave_state) = local_states.get_mut(slave_deck_id) {
            // --- ADDED LOG ---
             log::info!(
                "AudioThread enable_sync [PRE-CHECK SLAVE]: Checking slave '{}'. Found BPM: {:?}",
                slave_deck_id,
                slave_state.original_bpm
            );
            // --- END ADDED LOG ---
            // --- Added: Validate Slave BPM ---
            if slave_state.original_bpm.is_none() {
                 log::warn!(
                    "Audio Thread: EnableSync: Slave deck \'{}\' missing BPM metadata.",
                    slave_deck_id
                );
                emit_error_event(
                    app_handle,
                    slave_deck_id,
                    "Slave deck missing BPM",
                );
                return; // Exit if slave BPM is missing
            }
            // --- End Added ---

            let slave_bpm = slave_state.original_bpm.unwrap(); // Safe to unwrap after check

            log::debug!(
                "Sync Enable [Step 2]: Validated {} -> {}. Master BPM: {}, Slave BPM: {}",
                slave_deck_id,
                master_deck_id,
                master_bpm,
                slave_bpm
            );

            // Calculate Target Rate
            let calculated_target_rate = if slave_bpm.abs() > 1e-6 {
                (master_bpm / slave_bpm) * master_current_pitch
            } else {
                log::warn!(
                    "Audio Thread: EnableSync: Slave BPM is zero for '{}'. Cannot calculate rate.",
                    slave_deck_id
                );
                emit_error_event(
                    app_handle,
                    slave_deck_id,
                    "Slave deck BPM is zero",
                );
                return;
            };

            // Set Initial Slave Flags
            slave_state.is_sync_active = true;
            slave_state.is_master = false;
            slave_state.master_deck_id = Some(master_deck_id.to_string());
            slave_state.target_pitch_rate_for_bpm_match = calculated_target_rate;
            slave_state.manual_pitch_rate = *slave_state.current_pitch_rate.lock().unwrap();
            log::debug!("Sync Enable [Step 2]: Stored manual pitch rate for {}: {}", slave_deck_id, slave_state.manual_pitch_rate);
            log::info!("Target BPM match rate for {}: {:.4}", slave_deck_id, calculated_target_rate);

            calculated_target_rate // Return the rate for the next step
        } else {
            log::error!(
                "Audio Thread: EnableSync: Slave deck '{}' not found for mutable access.",
                slave_deck_id
            );
            emit_error_event(app_handle, slave_deck_id, "Slave deck not found");
            return;
        }
    }; // Slave mutable borrow ends here

    // --- Step 3: Calculate Initial Seek Adjustment ---
    let slave_seek_target_time_secs = {
        // Need current times and rates again - requires accessing the states
        let master_current_time = local_states.get(master_deck_id).map(get_current_playback_time_secs).unwrap_or(0.0);
        let slave_current_time = local_states.get(slave_deck_id).map(get_current_playback_time_secs).unwrap_or(0.0);

        // Get necessary metadata again (could be passed from Step 1/2 if refactored)
        let (master_fbs, master_pitch) = local_states.get(master_deck_id)
            .map(|s| (s.first_beat_sec, *s.current_pitch_rate.lock().unwrap()))
            .unwrap_or((None, 1.0));
        let (slave_fbs, slave_pitch, slave_bpm_opt) = local_states.get(slave_deck_id)
            .map(|s| (s.first_beat_sec, *s.current_pitch_rate.lock().unwrap(), s.original_bpm))
            .unwrap_or((None, 1.0, None));

        if let (Some(m_fbs), Some(s_fbs), Some(s_bpm)) = (master_fbs, slave_fbs, slave_bpm_opt) {
            if master_bpm > 1e-6 && s_bpm > 1e-6 && master_pitch.abs() > 1e-6 && slave_pitch.abs() > 1e-6 {
                let master_effective_interval = (60.0 / master_bpm) / master_pitch;
                let slave_effective_interval = (60.0 / s_bpm) / slave_pitch;

                let master_time_since_fbs = (master_current_time - m_fbs as f64).max(0.0);
                let slave_time_since_fbs = (slave_current_time - s_fbs as f64).max(0.0);

                let master_phase = (master_time_since_fbs / master_effective_interval as f64) % 1.0;
                let slave_phase = (slave_time_since_fbs / slave_effective_interval as f64) % 1.0;

                let phase_diff = master_phase - slave_phase;
                let wrapped_phase_diff = if phase_diff > 0.5 {
                    phase_diff - 1.0
                } else if phase_diff < -0.5 {
                    phase_diff + 1.0
                } else {
                    phase_diff
                };

                let time_adjustment_secs = wrapped_phase_diff * slave_effective_interval as f64;
                let calculated_seek_target = slave_current_time + time_adjustment_secs;

                 log::info!(
                    "Beat Align {}: MTime={:.3}, STime={:.3}, MPh={:.3}, SPh={:.3}, Diff={:.3}, Adjust={:.3}s, Target={:.3}s",
                    slave_deck_id,
                    master_current_time,
                    slave_current_time,
                    master_phase,
                    slave_phase,
                    wrapped_phase_diff,
                    time_adjustment_secs,
                    calculated_seek_target
                 );

                Some(calculated_seek_target)
            } else {
                 log::warn!("Beat Align Skip: Invalid BPM or pitch rate for calc.");
                 None
            }
        } else {
            log::warn!("Beat Align Skip: Missing First Beat Sec for master or slave.");
            None
        }
    };

    // --- Step 4: Apply Initial Seek (if calculated) ---
    if let Some(seek_target) = slave_seek_target_time_secs {
         // Re-add log
        log::debug!("Sync Enable [Step 4]: Applying initial seek for {} to {:.3}s", slave_deck_id, seek_target);
        // Need mutable borrow again for seek
        audio_thread_handle_seek(
            slave_deck_id,
            seek_target,
            local_states, // Pass mutable ref
            app_handle
        );
        // Note: audio_thread_handle_seek will update paused_position and playback_start_time
    } else {
         log::warn!("Could not calculate beat alignment seek for '{}'. Syncing BPM only.", slave_deck_id);
    }

    // --- Step 5: Get Master Mutably, Set Master Flag (if different) ---
    if slave_deck_id != master_deck_id {
        if let Some(master_state_mut) = local_states.get_mut(master_deck_id) {
            if !master_state_mut.is_master {
                log::info!("Setting deck '{}' as master.", master_deck_id);
                master_state_mut.is_master = true;
                master_state_mut.is_sync_active = false; // Cannot be both
                // Emit update for master state change
                let logical_states_arc_master = app_handle.state::<AppState>().logical_playback_states.clone();
                let master_update_state = PlaybackState {
                    is_master: true,
                    is_sync_active: false,
                    // Copy other fields from its current logical state
                    ..(logical_states_arc_master.lock().unwrap().get(master_deck_id).cloned().unwrap_or_default())
                };
                update_logical_state(&logical_states_arc_master, master_deck_id, master_update_state.clone());
                emit_state_update(app_handle, master_deck_id, &master_update_state);
            }
        } else {
            log::error!("EnableSync: Failed to get mutable master state '{}' after initial check?!", master_deck_id);
            // Attempt to roll back slave state? This is tricky.
            // For now, log the error and proceed; the slave might be synced to a non-existent master temporarily.
            // A more robust solution might involve queuing or better state management.
        }
    } // Master mutable borrow ends here

    // --- Step 6: Apply Target Pitch Rate ---
    // Cloning ID necessary because set_pitch_rate might borrow local_states mutably again
    let slave_id_clone = slave_deck_id.to_string();
    audio_thread_handle_set_pitch_rate(
        &slave_id_clone,
        target_rate,
        false,
        local_states, // Pass the mutable reference
        app_handle,
    );
    // Re-add log
    log::debug!("Sync Enable [Step 6]: Applied target rate {:.4} to {}", target_rate, slave_id_clone);
    // The set_pitch_rate function should emit the final state update for the slave
}

fn audio_thread_handle_disable_sync(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    log::info!(
        "Audio Thread: Handling DisableSync for deck: {}",
        deck_id
    );

    if let Some(deck_state) = local_states.get_mut(deck_id) {
        if !deck_state.is_sync_active && !deck_state.is_master {
            log::warn!("DisableSync: Deck '{}' is not currently synced or master.", deck_id);
            return;
        }

        let was_master = deck_state.is_master;
        let pitch_to_restore = deck_state.manual_pitch_rate; // Get the stored manual rate

        // --- Reset Flags ---
        deck_state.is_sync_active = false;
        deck_state.is_master = false;
        deck_state.master_deck_id = None;
        deck_state.target_pitch_rate_for_bpm_match = 1.0; // Reset target

        log::info!("Deck '{}' sync disabled. Restoring pitch to: {}", deck_id, pitch_to_restore);

        // --- Revert Pitch ---
        // Call set_pitch_rate to apply the stored manual rate and emit updates
        // Cloning ID to satisfy borrow checker
        let deck_id_clone = deck_id.to_string();
        audio_thread_handle_set_pitch_rate(
            &deck_id_clone,
            pitch_to_restore,
            false,
            local_states, // Pass mutable reference
            app_handle,
        );
        // Note: set_pitch_rate emits the necessary state update

        // --- Handle Master Change Side Effects (Moved inside the block) ---
        if was_master { // Now this read is guaranteed if we are in this block
            log::info!("Deck '{}' was master. Checking slaves...", deck_id);
            // Find any slaves that were synced to this deck and disable them too
            let slaves_to_disable: Vec<String> = local_states
                .iter()
                .filter(|(_id, state)| state.master_deck_id.as_deref() == Some(deck_id))
                .map(|(id, _)| id.clone())
                .collect();

            if !slaves_to_disable.is_empty() {
                log::info!("Disabling sync for former slaves of '{}': {:?}", deck_id, slaves_to_disable);
                for slave_id in slaves_to_disable {
                     // Need to call recursively/iteratively. Be careful with mutable borrows.
                     // It's safer to queue these actions or handle them differently if complex interactions arise.
                     // Direct recursive call can work if set_pitch_rate doesn't cause issues.
                     audio_thread_handle_disable_sync(&slave_id, local_states, app_handle);
                }
            }
        }
    } else {
        log::error!("DisableSync: Deck '{}' not found.", deck_id);
        emit_error_event(app_handle, deck_id, "Deck not found for disable sync");
        return;
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
    original_bpm: Option<f32>,
    first_beat_sec: Option<f32>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    // --- Logging Step 1 --- 
    log::info!(
        "CMD load_track: Received for Deck '{}', Path '{}', BPM: {:?}, FBS: {:?}",
        deck_id,
        path,
        original_bpm,
        first_beat_sec
    );
    // Original log::info! remains below for comparison if needed
    log::info!(
        "CMD: Load track '{}' for deck: {}. BPM: {:?}, First Beat: {:?}",
        path,
        deck_id,
        original_bpm,
        first_beat_sec
    );

    // Pass metadata along in the command
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::LoadTrack {
            deck_id,
            path,
            original_bpm,
            first_beat_sec,
        })
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
        .send(AudioThreadCommand::SetPitchRate { deck_id, rate, is_manual_adjustment: true })
        .await
        .map_err(|e| e.to_string())
}

// --- New Sync Commands ---

#[tauri::command]
pub async fn enable_sync(
    slave_deck_id: String,
    master_deck_id: String,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::info!(
        "CMD: Enable sync for slave '{}' with master '{}'",
        slave_deck_id,
        master_deck_id
    );
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::EnableSync {
            slave_deck_id,
            master_deck_id,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn disable_sync(deck_id: String, app_state: State<'_, AppState>) -> Result<(), String> {
    log::info!("CMD: Disable sync for deck '{}'", deck_id);
    app_state
        .audio_command_sender
        .send(AudioThreadCommand::DisableSync { deck_id })
        .await
        .map_err(|e| e.to_string())
}
