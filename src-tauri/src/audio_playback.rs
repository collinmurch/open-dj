// src-tauri/src/audio_playback.rs

use std::collections::HashMap;
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio_effects::EqSource;
use crate::playback_types::{AudioThreadCommand, EqParams, PlaybackState};
use crate::errors::{AudioDecodingError, PlaybackError}; // Import custom errors

use rodio::{OutputStream, OutputStreamHandle, Sink, buffer::SamplesBuffer};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_NULL, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::mpsc;
use crate::config; // Added import for config

// --- State Management ---

// Logical state managed by Tauri (for immediate UI feedback)
pub struct AppState {
    // Sender to the dedicated audio thread
    audio_command_sender: mpsc::Sender<AudioThreadCommand>,
    // Stores the *logical* state reflected in the UI
    logical_playback_states: Arc<Mutex<HashMap<String, PlaybackState>>>,
}

impl AppState {
    // Constructor now takes the sender
    pub fn new(sender: mpsc::Sender<AudioThreadCommand>) -> Self {
        AppState {
            audio_command_sender: sender,
            logical_playback_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// Data managed locally ONLY within the audio thread
struct AudioThreadDeckState {
    // Keep the sink for playback control
    sink: Sink,
    // Store the raw audio data for seeking/rebuilding
    decoded_samples: Arc<Vec<f32>>, // Use Arc for potential cheap cloning
    sample_rate: f32,
    // Playback timing state
    playback_start_time: Option<Instant>,
    paused_position: Option<Duration>,
    duration: Duration,
    is_playing: bool,
    // Shared EQ parameters
    eq_params: Arc<Mutex<EqParams>>,
    trim_gain: Arc<Mutex<f32>>, // Add trim gain state (linear)
    cue_point: Option<Duration>, // Added cue point state
}

// --- Decoding Logic (Moved from audio_processor.rs) ---

/// Decodes an audio file to mono f32 samples for playback.
/// Made public within the crate (`pub(crate)`) for use by audio_processor.
pub(crate) fn decode_audio_for_playback(path: &str) -> Result<(Vec<f32>, f32), AudioDecodingError> {
    let file = File::open(path).map_err(|e| AudioDecodingError::FileOpenError { path: path.to_string(), source: e })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let hint = Hint::new();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AudioDecodingError::FormatError { path: path.to_string(), source: e })?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| AudioDecodingError::NoSuitableTrack { path: path.to_string() })?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| AudioDecodingError::MissingSampleRate { path: path.to_string() })? as f32;
    let channels = track
        .codec_params
        .channels
        .ok_or_else(|| AudioDecodingError::MissingChannelInfo { path: path.to_string() })?
        .count();
    let codec_params = track.codec_params.clone();

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| AudioDecodingError::DecoderCreationError { path: path.to_string(), source: e })?;

    let mut samples: Vec<f32> = Vec::with_capacity(1024 * 256);
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        match format.next_packet() {
            Ok(packet) => {
                if packet.track_id() != track_id {
                    continue;
                }
                match decoder.decode(&packet) {
                    Ok(audio_buf) => {
                        if sample_buf.is_none() {
                            sample_buf = Some(SampleBuffer::<f32>::new(
                                audio_buf.capacity() as u64,
                                *audio_buf.spec(),
                            ));
                        }
                        if let Some(buf) = sample_buf.as_mut() {
                            buf.copy_interleaved_ref(audio_buf);
                            let raw_samples = buf.samples();
                            if channels > 1 {
                                samples.extend(
                                    raw_samples
                                        .chunks_exact(channels)
                                        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32),
                                );
                            } else {
                                samples.extend_from_slice(raw_samples);
                            }
                        }
                    }
                    Err(SymphoniaError::DecodeError(err_desc)) => {
                        log::warn!("Playback Decode: Ignoring decode error in '{}': {}", path, err_desc);
                    }
                    Err(e) => {
                        return Err(AudioDecodingError::FatalDecodeError { path: path.to_string(), source: e });
                    }
                }
            }
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                log::debug!("Playback Decode: Reached EOF for '{}'", path);
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                log::warn!(
                    "Playback Decode: Decoder reset required unexpectedly for '{}'",
                    path
                );
                break; // Treat as error/EOF
            }
            Err(e) => {
                return Err(AudioDecodingError::PacketReadIoError { path: path.to_string(), source: e });
            }
        }
    }

    decoder.finalize();
    log::debug!(
        "Playback Decode: Decoded {} mono samples at {} Hz for '{}'",
        samples.len(),
        sample_rate,
        path
    );
    if samples.is_empty() {
        return Err(AudioDecodingError::NoSamplesDecoded { path: path.to_string() });
    }

    Ok((samples, sample_rate))
}

// --- Helper Functions ---

fn emit_state_update<R: Runtime>(
    manager: &(impl Manager<R> + Emitter<R>),
    deck_id: &str,
    state: &PlaybackState,
) {
    if let Err(e) = manager.emit(
        "playback://update",
        serde_json::json!({ "deckId": deck_id, "state": state }),
    ) {
        log::error!(
            "Failed to emit playback state update for deck {}: {}",
            deck_id,
            e
        );
    }
}

fn emit_error_event<R: Runtime>(
    manager: &(impl Manager<R> + Emitter<R>),
    deck_id: &str,
    error_message: &str, // Keep as string for frontend simplicity
) {
    if let Err(e) = manager.emit(
        "playback://error",
        serde_json::json!({ "deckId": deck_id, "error": error_message }),
    ) {
        log::error!("Failed to emit playback error for deck {}: {}", deck_id, e);
    }
}

// --- Tauri Commands (Refactored to send messages) ---

// Helper to update logical state and emit immediately
fn update_logical_state<F>(
    state_map: &Arc<Mutex<HashMap<String, PlaybackState>>>,
    deck_id: &str,
    app_handle: &AppHandle,
    update_fn: F,
) -> Result<(), PlaybackError>
where
    F: FnOnce(&mut PlaybackState),
{
    let mut states = state_map
        .lock()
        .map_err(|_| PlaybackError::LogicalStateLockError("Mutex poisoned".to_string()))?;
    let logical_state = states.entry(deck_id.to_string()).or_default();
    let old_state_json = serde_json::to_string(logical_state).unwrap_or_default(); // Log old state
    update_fn(logical_state);
    let new_state_json = serde_json::to_string(logical_state).unwrap_or_default(); // Log new state
    log::debug!(
        "update_logical_state for '{}': Before: {} -> After: {}",
        deck_id,
        old_state_json,
        new_state_json
    );
    let state_to_emit = logical_state.clone();
    drop(states);
    emit_state_update(app_handle, deck_id, &state_to_emit);
    Ok(())
}

#[tauri::command]
pub async fn init_player(
    state: State<'_, AppState>,
    app_handle: AppHandle, // app_handle is used by update_logical_state
    deck_id: String,
) -> Result<(), String> {
    update_logical_state(&state.logical_playback_states, &deck_id, &app_handle, |_| {}).map_err(|e| e.to_string())?;
    state.audio_command_sender.send(AudioThreadCommand::InitDeck(deck_id)).await.map_err(|e| PlaybackError::from(e).to_string())
}

#[tauri::command]
pub async fn load_track(
    state: State<'_, AppState>,
    // _app_handle: AppHandle, // Not used directly here
    deck_id: String,
    path: String,
) -> Result<(), String> {
    {
        let mut states = state.logical_playback_states.lock()
            .map_err(|_| PlaybackError::LogicalStateLockError("Mutex poisoned for loading flag".to_string()).to_string())?;
        let logical_state = states.entry(deck_id.to_string()).or_default();
        logical_state.is_loading = true;
        logical_state.error = None; 
    }
    state.audio_command_sender.send(AudioThreadCommand::LoadTrack { deck_id, path }).await.map_err(|e| PlaybackError::from(e).to_string())
}

#[tauri::command]
pub async fn play_track(
    state: State<'_, AppState>,
    // _app_handle: AppHandle, // Not used
    deck_id: String,
) -> Result<(), String> {
    state.audio_command_sender.send(AudioThreadCommand::Play(deck_id)).await.map_err(|e| PlaybackError::from(e).to_string())
}

#[tauri::command]
pub async fn pause_track(
    state: State<'_, AppState>,
    // _app_handle: AppHandle, // Not used
    deck_id: String,
) -> Result<(), String> {
    state.audio_command_sender.send(AudioThreadCommand::Pause(deck_id)).await.map_err(|e| PlaybackError::from(e).to_string())
}

#[tauri::command]
pub async fn seek_track(
    state: State<'_, AppState>,
    // app_handle needed only if we were emitting from here
    deck_id: String,
    position_seconds: f64,
) -> Result<(), String> {
    log::info!("CMD Seek: Deck '{}' to {:.2}s", deck_id, position_seconds);
    // We *don't* update logical state here, audio thread will emit the new time
    state
        .audio_command_sender
        .send(AudioThreadCommand::Seek {
            deck_id,
            position_seconds,
        })
        .await
        .map_err(|e| format!("CMD Seek: Failed to send command: {}", e))
}

#[tauri::command]
pub fn get_playback_state(
    state: State<'_, AppState>,
    deck_id: String,
) -> Result<PlaybackState, String> {
    log::debug!("CMD GetState: Deck '{}'", deck_id);
    let states = state
        .logical_playback_states
        .lock()
        .map_err(|_| PlaybackError::LogicalStateLockError("Mutex poisoned for get_playback_state".to_string()).to_string())?;
    states
        .get(&deck_id)
        .cloned()
        .ok_or_else(|| PlaybackError::LogicalStateNotFound { deck_id }.to_string())
}

#[tauri::command]
pub async fn cleanup_player(state: State<'_, AppState>, deck_id: String) -> Result<(), String> {
    log::info!("CMD Cleanup: Deck '{}'", deck_id);
    // Remove logical state immediately (optional)
    // state.logical_playback_states.lock().unwrap().remove(&deck_id);

    state
        .audio_command_sender
        .send(AudioThreadCommand::CleanupDeck(deck_id))
        .await
        .map_err(|e| format!("CMD Cleanup: Failed to send command: {}", e))
}

#[tauri::command]
pub async fn set_fader_level(
    state: State<'_, AppState>,
    deck_id: String,
    level: f32, // Expect linear level 0.0 - 1.0
) -> Result<(), String> {
    // Clamp level to 0.0 - 1.0 (sink volume is linear)
    let clamped_level = level.clamp(0.0, 1.0);
    log::info!(
        "CMD SetFaderLevel: Deck '{}' to {:.2}",
        deck_id,
        clamped_level
    );
    state
        .audio_command_sender
        .send(AudioThreadCommand::SetFaderLevel {
            deck_id,
            level: clamped_level, // Send linear level
        })
        .await
        .map_err(|e| format!("CMD SetFaderLevel: Failed to send command: {}", e))
}

#[tauri::command]
pub async fn set_trim_gain(
    state: State<'_, AppState>,
    deck_id: String,
    gain_db: f32, // Expect dB value from UI
) -> Result<(), String> {
    // Clamp dB range (e.g., -12dB to +12dB)
    let clamped_db = gain_db.clamp(-12.0, 12.0);
    // Convert dB to linear gain for internal processing
    let linear_gain = 10.0f32.powf(clamped_db / 20.0);
    log::info!(
        "CMD SetTrimGain: Deck '{}' to {:.1} dB (Linear: {:.3})",
        deck_id,
        clamped_db,
        linear_gain
    );
    state
        .audio_command_sender
        .send(AudioThreadCommand::SetTrimGain {
            deck_id,
            gain: linear_gain, // Send linear gain
        })
        .await
        .map_err(|e| format!("CMD SetTrimGain: Failed to send command: {}", e))
}

#[tauri::command]
pub async fn set_eq_params(
    state: State<'_, AppState>,
    deck_id: String,
    low_gain_db: f32,
    mid_gain_db: f32,
    high_gain_db: f32,
) -> Result<(), String> {
    log::info!(
        "Tauri command `set_eq_params` invoked for deck '{}'",
        deck_id
    );

    // Adjust clamp range: Deeper cut (-26dB), less boost (+6dB)
    let clamped_low = low_gain_db.clamp(-26.0, 6.0);
    let clamped_mid = mid_gain_db.clamp(-26.0, 6.0);
    let clamped_high = high_gain_db.clamp(-26.0, 6.0);

    log::info!(
        "CMD SetEq: Deck '{}' Low: {:.1}dB, Mid: {:.1}dB, High: {:.1}dB",
        deck_id,
        clamped_low,
        clamped_mid,
        clamped_high
    );
    let params = EqParams {
        low_gain_db: clamped_low,
        mid_gain_db: clamped_mid,
        high_gain_db: clamped_high,
    };

    state
        .audio_command_sender
        .send(AudioThreadCommand::SetEq { deck_id, params })
        .await
        .map_err(|e| format!("CMD SetEq: Failed to send command: {}", e))
}

// NEW Tauri Command for setting cue point
#[tauri::command]
pub async fn set_cue_point(
    state: State<'_, AppState>,
    deck_id: String,
    position_seconds: f64,
) -> Result<(), String> {
    if position_seconds < 0.0 {
        return Err("Cue point position cannot be negative.".to_string());
    }
    log::info!("CMD SetCue: Deck '{}' to {:.3}s", deck_id, position_seconds);
    state
        .audio_command_sender
        .send(AudioThreadCommand::SetCue { deck_id, position_seconds })
        .await
        .map_err(|e| format!("CMD SetCue: Failed to send command: {}", e))
}

// --- Audio Thread Implementation (Placeholder - to be implemented) ---

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
            Duration::from_millis(config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS)
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
    if !local_states.contains_key(deck_id) {
        match Sink::try_new(audio_handle) {
            Ok(sink) => {
                let initial_eq_params = Arc::new(Mutex::new(EqParams::default()));
                let initial_trim_gain = Arc::new(Mutex::new(1.0f32)); // Default trim = 1.0 (0dB)
                local_states.insert(
                    deck_id.to_string(),
                    AudioThreadDeckState {
                        sink,
                        // Initialize audio data fields as empty/default
                        decoded_samples: Arc::new(Vec::new()),
                        sample_rate: 0.0, // Placeholder, will be set on load
                        duration: Duration::ZERO,
                        is_playing: false,
                        playback_start_time: None,
                        paused_position: None,
                        eq_params: initial_eq_params,
                        trim_gain: initial_trim_gain, // Initialize trim gain
                        cue_point: None, // Initialize cue point
                    },
                );
                log::info!("Audio Thread: Initialized sink for deck '{}'", deck_id);
                // Emit default state which now includes cue_point_seconds: null
                emit_state_update(app_handle, deck_id, &PlaybackState::default());
            }
            Err(e) => {
                let err_msg = PlaybackError::SinkCreationError{ deck_id: deck_id.to_string(), source: e }.to_string();
                log::error!("Audio Thread: {}", err_msg);
                emit_error_event(app_handle, deck_id, &err_msg);
            }
        }
    } else {
        log::warn!("Audio Thread: Deck '{}' already initialized locally.", deck_id);
    }
}

async fn audio_thread_handle_load(
    deck_id: String,
    path: String,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    log::debug!("Audio Thread: Handling LoadTrack for '{}'", deck_id);

    let (eq_params_arc, trim_gain_arc) = match local_states.get(&deck_id) {
        Some(state) => (state.eq_params.clone(), state.trim_gain.clone()),
        None => {
            let err = PlaybackError::DeckNotFound { deck_id: deck_id.to_string() };
            log::error!("Audio Thread: LoadTrack: {:?}", err); 
            emit_error_event(app_handle, &deck_id, &err.to_string());
            return;
        }
    };

    let path_clone = path.clone();
    let app_handle_clone = app_handle.clone();
    let deck_id_clone = deck_id.clone();

    let decode_handle = tokio::task::spawn_blocking(move || {
        decode_audio_for_playback(&path_clone)
    });

    match decode_handle.await {
        Ok(decode_result) => {
            if let Some(deck_state) = local_states.get_mut(&deck_id) {
                match decode_result {
                    Ok((samples, rate)) => {
                        let duration = Duration::from_secs_f64(samples.len() as f64 / rate as f64);
                        log::info!(
                            "Audio Thread: Decoded '{}'. Duration: {:?}, Rate: {}, Samples: {}",
                            path, duration, rate, samples.len()
                        );

                        // *** Store decoded data in the state ***
                        deck_state.decoded_samples = Arc::new(samples); // Store as Arc
                        deck_state.sample_rate = rate;
                        deck_state.duration = duration;
                        deck_state.cue_point = None; // Reset cue point on load

                        // Create the initial source with Trim and EQ
                        let buffer = SamplesBuffer::new(1, rate as u32, (*deck_state.decoded_samples).clone());
                        
                        // Explicitly handle Result from EqSource::new before append
                        match EqSource::new(buffer, eq_params_arc.clone(), trim_gain_arc.clone()) {
                            Ok(unwrapped_eq_source) => { // Successfully created EqSource
                                deck_state.sink.stop();
                                deck_state.sink.clear();
                                deck_state.sink.append(unwrapped_eq_source); // Append the unwrapped source
                                deck_state.sink.pause();
                                deck_state.sink.set_volume(1.0);

                                deck_state.is_playing = false;
                                deck_state.playback_start_time = None;
                                deck_state.paused_position = Some(Duration::ZERO);

                                let state_to_emit = PlaybackState {
                                    is_playing: false, is_loading: false, current_time: 0.0,
                                    duration: duration.as_secs_f64(), error: None,
                                    cue_point_seconds: None, // Emit initial null cue point
                                };
                                emit_state_update(app_handle, &deck_id, &state_to_emit);
                            }
                            Err(eq_creation_error) => {
                                let err_msg = format!("Failed to create EQ source for deck '{}': {:?}", deck_id, eq_creation_error);
                                log::error!("Audio Thread: {}", err_msg);
                                emit_error_event(app_handle, &deck_id, &err_msg);
                                let state_to_emit = PlaybackState { is_loading: false, error: Some(err_msg), ..Default::default() };
                                emit_state_update(app_handle, &deck_id, &state_to_emit);
                            }
                        }
                    }
                    Err(e_decode) => {
                        let err = PlaybackError::PlaybackDecodeError { deck_id: deck_id.to_string(), source: e_decode };
                        log::error!("Audio Thread: Decode failed: {:?}", err); // Use {:?} for custom errors
                        let err_string = err.to_string();
                        emit_error_event(app_handle, &deck_id, &err_string);
                        let state_to_emit = PlaybackState { is_loading: false, error: Some(err_string), ..Default::default() };
                        emit_state_update(app_handle, &deck_id, &state_to_emit);
                    }
                }
            } else {
                 log::error!("Audio Thread: Deck '{}' disappeared after decode attempt?!", deck_id);
            }
        }
        Err(join_error) => {
            log::error!(
                "Audio Thread: Decode task panicked for deck '{}': {}",
                deck_id_clone,
                join_error
            );
            let error_msg = format!("Audio decoding task failed: {}", join_error);
            emit_error_event(&app_handle_clone, &deck_id_clone, &error_msg);
            if let Some(deck_state) = local_states.get_mut(&deck_id_clone) {
                deck_state.decoded_samples = Arc::new(Vec::new());
                deck_state.sample_rate = 0.0;
                deck_state.duration = Duration::ZERO;
                deck_state.is_playing = false;
                deck_state.playback_start_time = None;
                deck_state.paused_position = None;
            }
            let state_to_emit = PlaybackState {
                is_loading: false,
                error: Some(error_msg),
                ..Default::default()
            };
            emit_state_update(&app_handle_clone, &deck_id_clone, &state_to_emit);
        }
    }
}

fn audio_thread_handle_play(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    log::debug!("Audio Thread: Handling Play for '{}'", deck_id);
    if let Some(deck_state) = local_states.get_mut(deck_id) {
        if deck_state.sink.empty() {
            log::warn!("Audio Thread: Play called on empty sink for '{}'", deck_id);
            emit_error_event(app_handle, deck_id, "Cannot play: No track loaded.");
        } else if !deck_state.is_playing {
            deck_state.sink.play();
            deck_state.is_playing = true;
            deck_state.playback_start_time = Some(Instant::now());
            log::info!("Audio Thread: Playback started for '{}'", deck_id);
            let state_to_emit = PlaybackState {
                is_playing: true,
                current_time: deck_state.paused_position.unwrap_or_default().as_secs_f64(),
                duration: deck_state.duration.as_secs_f64(),
                is_loading: false,
                error: None,
                cue_point_seconds: deck_state.cue_point.map(|d| d.as_secs_f64()),
            };
            emit_state_update(app_handle, deck_id, &state_to_emit);
        } else {
            log::trace!("Audio Thread: Already playing '{}'", deck_id);
        }
    }
}

fn audio_thread_handle_pause(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    log::debug!("Audio Thread: Handling Pause START for '{}'", deck_id);
    if let Some(deck_state) = local_states.get_mut(deck_id) {
        if deck_state.is_playing {
            deck_state.sink.pause();
            deck_state.is_playing = false;
            let elapsed_since_play = deck_state
                .playback_start_time
                .map_or(Duration::ZERO, |st| st.elapsed());
            let previous_pos = deck_state.paused_position.unwrap_or_default();
            let new_paused_pos = previous_pos + elapsed_since_play;
            deck_state.paused_position = Some(new_paused_pos);
            deck_state.playback_start_time = None;
            log::info!(
                "Audio Thread: Playback paused for '{}' at {:?}",
                deck_id,
                new_paused_pos
            );
            let state_to_emit = PlaybackState {
                is_playing: false,
                current_time: new_paused_pos
                    .as_secs_f64()
                    .min(deck_state.duration.as_secs_f64()),
                duration: deck_state.duration.as_secs_f64(),
                is_loading: false,
                error: None,
                cue_point_seconds: deck_state.cue_point.map(|d| d.as_secs_f64()),
            };
            emit_state_update(app_handle, deck_id, &state_to_emit);
        } else {
            log::trace!("Audio Thread: Already paused '{}'", deck_id);
        }
    }
    log::debug!("Audio Thread: Handling Pause END for '{}'", deck_id);
}

fn audio_thread_handle_seek(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    log::debug!(
        "Audio Thread: Handling Seek START for '{}' to {:.2}s",
        deck_id,
        position_seconds
    );
    if let Some(deck_state) = local_states.get_mut(deck_id) {
        // Check if track is loaded
        if deck_state.duration == Duration::ZERO || deck_state.sample_rate <= 0.0 || deck_state.decoded_samples.is_empty() {
            log::warn!(
                "Audio Thread: Cannot seek deck '{}', track not fully loaded or invalid state.",
                deck_id
            );
            emit_error_event(
                app_handle,
                deck_id,
                "Cannot seek: Track not loaded or invalid state.",
            );
            return;
        }

        // Clamp seek position and calculate sample index
        let clamped_time_secs = position_seconds.max(0.0).min(deck_state.duration.as_secs_f64());
        let target_sample_index = (clamped_time_secs * deck_state.sample_rate as f64).round() as usize;
        let seek_pos_duration = Duration::from_secs_f64(clamped_time_secs);

        log::info!(
            "Audio Thread: Seeking '{}' to {:?} (sample index {})",
            deck_id,
            seek_pos_duration,
            target_sample_index
        );

        // Rebuild the source chain
        let current_fader_level = deck_state.sink.volume(); // Preserve current fader level
        deck_state.sink.stop();
        deck_state.sink.clear();

        // Ensure index is within bounds
        let start_index = target_sample_index.min(deck_state.decoded_samples.len());

        // Create a new SamplesBuffer starting from the seek position
        // We need to clone the relevant slice of the Arc<Vec<f32>> data.
        // This involves creating a new Vec, which is less efficient than rodio's internal seek
        // but necessary for wrapped sources.
        let remaining_samples: Vec<f32> = deck_state.decoded_samples[start_index..].to_vec();
        
        if remaining_samples.is_empty() {
            log::warn!("Audio Thread: Seek position {} is at or beyond the end of the track for deck '{}'", clamped_time_secs, deck_id);
            // Treat as seeking to the end
            deck_state.paused_position = Some(deck_state.duration);
            deck_state.playback_start_time = None; // Ensure playback doesn't restart
            deck_state.is_playing = false; // Mark as not playing
        } else {
             let new_buffer = SamplesBuffer::new(1, deck_state.sample_rate as u32, remaining_samples);
             // Explicitly handle Result from EqSource::new before append
             match EqSource::new(new_buffer, deck_state.eq_params.clone(), deck_state.trim_gain.clone()) {
                 Ok(unwrapped_new_eq_source) => { // Successfully created EqSource
                    deck_state.sink.append(unwrapped_new_eq_source); // Append the unwrapped source
                    deck_state.sink.set_volume(current_fader_level);
                    deck_state.paused_position = Some(seek_pos_duration);
                    if deck_state.is_playing {
                        deck_state.playback_start_time = Some(Instant::now());
                        deck_state.sink.play();
                    } else {
                        deck_state.playback_start_time = None;
                        deck_state.sink.pause();
                    }
                 }
                 Err(eq_creation_error) => {
                    let err_msg = format!("Failed to create EQ source for seek on deck '{}': {:?}", deck_id, eq_creation_error);
                    log::error!("Audio Thread: {}", err_msg);
                    emit_error_event(app_handle, deck_id, &err_msg);
                    deck_state.is_playing = false; 
                 }
             }
        }

        // Emit the state update regardless of whether samples remained
        let state_to_emit = PlaybackState {
            is_playing: deck_state.is_playing,
            current_time: clamped_time_secs,
            duration: deck_state.duration.as_secs_f64(),
            is_loading: false,
            error: None,
            cue_point_seconds: deck_state.cue_point.map(|d| d.as_secs_f64()),
        };
        emit_state_update(app_handle, deck_id, &state_to_emit);

    } else {
         log::warn!("Audio Thread: Seek ignored for unknown deck '{}'", deck_id);
    }
    log::debug!("Audio Thread: Handling Seek END for '{}'", deck_id);
}

fn audio_thread_handle_set_fader_level(
    deck_id: &str,
    level: f32, // Linear level 0.0 - 1.0
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    log::debug!(
        "Audio Thread: Handling SetFaderLevel START for '{}' to {:.2}",
        deck_id,
        level
    );
    if let Some(deck_state) = local_states.get_mut(deck_id) {
        // Use sink's volume control for the fader
        deck_state.sink.set_volume(level);
        log::info!("Audio Thread: Fader level (sink volume) set for '{}' to {:.2}", deck_id, level);
    } else {
        log::warn!("Audio Thread: SetFaderLevel ignored for unknown deck '{}'", deck_id);
    }
    log::debug!("Audio Thread: Handling SetFaderLevel END for '{}'", deck_id);
}

fn audio_thread_handle_set_trim_gain(
    deck_id: &str,
    gain: f32, // Linear gain
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    log::debug!(
        "Audio Thread: Handling SetTrimGain START for '{}' to {:.3}",
        deck_id,
        gain
    );
    if let Some(deck_state) = local_states.get_mut(deck_id) {
        // Update the shared trim gain value
        *deck_state.trim_gain.lock().expect("Failed to lock trim gain for update") = gain;
        log::info!("Audio Thread: Trim gain updated in shared state for '{}' to {:.3}", deck_id, gain);
        // Note: EqSource will pick this up automatically in its `next` method
    } else {
        log::warn!("Audio Thread: SetTrimGain ignored for unknown deck '{}'", deck_id);
    }
    log::debug!("Audio Thread: Handling SetTrimGain END for '{}'", deck_id);
}

fn audio_thread_handle_set_eq(
    deck_id: &str,
    new_params: EqParams,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    log::debug!(
        "Audio Thread: Handling SetEq START for '{}': {:?}",
        deck_id,
        new_params
    );
    if let Some(deck_state) = local_states.get_mut(deck_id) {
        // Lock the shared parameters and update them
        let mut params_guard = deck_state
            .eq_params
            .lock()
            .expect("Failed to lock EQ params for update");
        *params_guard = new_params;
        // The EqSource instance running in the sink will pick up these changes
        // via the Arc<Mutex> the next time it checks.
        log::info!(
            "Audio Thread: EQ params updated in shared state for '{}'",
            deck_id
        );
    } else {
        log::warn!("Audio Thread: SetEq ignored for unknown deck '{}'", deck_id);
    }
    log::debug!("Audio Thread: Handling SetEq END for '{}'", deck_id);
}

fn audio_thread_handle_set_cue(
    deck_id: &str,
    position_seconds: f64,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    log::debug!("Audio Thread: Handling SetCue for '{}' to {:.3}s", deck_id, position_seconds);
    if let Some(deck_state) = local_states.get_mut(deck_id) {
        // Ensure duration is valid before setting cue relative to it
        if deck_state.duration == Duration::ZERO {
            log::warn!("Audio Thread: Cannot set cue for deck '{}', track duration is zero (not loaded?)", deck_id);
            emit_error_event(app_handle, deck_id, "Cannot set cue: Track not loaded or has zero duration.");
            return;
        }

        let clamped_time_secs = position_seconds.max(0.0).min(deck_state.duration.as_secs_f64());
        let cue_duration = Duration::from_secs_f64(clamped_time_secs);

        deck_state.cue_point = Some(cue_duration);
        log::info!("Audio Thread: Cue point set for '{}' to {:?}", deck_id, cue_duration);

        // Emit state update with the new cue point
        let current_time = get_current_playback_time_secs(deck_state); // Use helper to get current time
        let state_to_emit = PlaybackState {
            is_playing: deck_state.is_playing,
            current_time: current_time,
            duration: deck_state.duration.as_secs_f64(),
            is_loading: false,
            error: None,
            cue_point_seconds: Some(clamped_time_secs),
        };
        emit_state_update(app_handle, deck_id, &state_to_emit);
    } else {
        log::warn!("Audio Thread: SetCue ignored for unknown deck '{}'", deck_id);
    }
}

// Helper function to calculate current time (used in multiple places)
fn get_current_playback_time_secs(deck_state: &AudioThreadDeckState) -> f64 {
    if deck_state.is_playing {
        let current_elapsed = deck_state
            .playback_start_time
            .map_or(Duration::ZERO, |st| st.elapsed());
        let base_pos = deck_state.paused_position.unwrap_or_default();
        (base_pos + current_elapsed).as_secs_f64()
    } else {
        deck_state.paused_position.unwrap_or_default().as_secs_f64()
    }
    .min(deck_state.duration.as_secs_f64()) // Ensure it doesn't exceed duration
}

fn audio_thread_handle_time_update(
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle,
) {
    for (deck_id, deck_state) in local_states.iter_mut() {
        if deck_state.is_playing && !deck_state.sink.is_paused() && !deck_state.sink.empty() {
            let current_time_secs = get_current_playback_time_secs(deck_state);

            let end_buffer = Duration::from_millis(50);
            let has_ended = deck_state.duration > Duration::ZERO
                && (Duration::from_secs_f64(current_time_secs) + end_buffer >= deck_state.duration);

            let state_update = PlaybackState {
                current_time: current_time_secs,
                is_playing: !has_ended,
                duration: deck_state.duration.as_secs_f64(),
                is_loading: false,
                error: None,
                cue_point_seconds: deck_state.cue_point.map(|d| d.as_secs_f64()),
            };
            emit_state_update(app_handle, deck_id, &state_update);

            if has_ended {
                log::info!(
                    "Audio Thread: Track finished for '{}' based on time update.",
                    deck_id
                );
                deck_state.sink.pause();
                deck_state.is_playing = false;
                deck_state.playback_start_time = None;
                deck_state.paused_position = Some(deck_state.duration);
            }
        }
        // No need for an else block to emit state for paused tracks, 
        // as state is emitted when actions like pause, seek, set_cue occur.
    }
}

fn audio_thread_handle_cleanup(
    deck_id: &str,
    local_states: &mut HashMap<String, AudioThreadDeckState>,
) {
    if local_states.remove(deck_id).is_some() {
        log::info!("Audio Thread: Cleaned up '{}'", deck_id);
    }
}