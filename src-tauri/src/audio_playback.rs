// src-tauri/src/audio_playback.rs
use std::collections::HashMap;
use std::fs::File;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{OutputStream, Sink, buffer::SamplesBuffer};
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

// --- Audio Thread Communication ---

#[derive(Debug)]
pub enum AudioThreadCommand {
    InitDeck(String), // deck_id
    LoadTrack {
        deck_id: String,
        path: String,
    },
    Play(String),  // deck_id
    Pause(String), // deck_id
    Seek {
        deck_id: String,
        position_seconds: f64,
    },
    CleanupDeck(String), // deck_id
    Shutdown,
}

// --- State Management ---

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    is_playing: bool,
    is_loading: bool,
    current_time: f64,
    duration: f64,
    error: Option<String>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        PlaybackState {
            is_playing: false,
            is_loading: false,
            current_time: 0.0,
            duration: 0.0,
            error: None,
        }
    }
}

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
    sink: Sink,
    playback_start_time: Option<Instant>,
    paused_position: Option<Duration>,
    duration: Duration,
    is_playing: bool, // Track actual sink state
}

// --- Decoding Logic (Moved from audio_processor.rs) ---

/// Decodes an audio file to mono f32 samples for playback.
/// Made public within the crate (`pub(crate)`) for use by audio_processor.
pub(crate) fn decode_audio_for_playback(path: &str) -> Result<(Vec<f32>, f32), String> {
    // This implementation is largely the same as the original one in audio_processor,
    // with minor adjustments to error messages for clarity.
    let file = File::open(path)
        .map_err(|e| format!("Playback Decode: Failed to open file '{}': {}", path, e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let hint = Hint::new();

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Playback Decode: Failed probe format for '{}': {}", path, e))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| format!("Playback Decode: No suitable audio track in '{}'", path))?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| format!("Playback Decode: Sample rate missing in '{}'", path))?
        as f32;
    let channels = track
        .codec_params
        .channels
        .ok_or_else(|| format!("Playback Decode: Channel info missing in '{}'", path))?
        .count();
    let codec_params = track.codec_params.clone();

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| {
            format!(
                "Playback Decode: Failed to create decoder for '{}': {}",
                path, e
            )
        })?;

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
                    Err(SymphoniaError::DecodeError(err)) => {
                        log::warn!(
                            "Playback Decode: Ignoring decode error in '{}': {}",
                            path,
                            err
                        );
                    }
                    Err(e) => {
                        return Err(format!(
                            "Playback Decode: Fatal decode error in '{}': {}",
                            path, e
                        ));
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
                return Err(format!(
                    "Playback Decode: Error reading packet for '{}': {}",
                    path, e
                ));
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
        return Err(format!(
            "Playback Decode: No samples decoded from '{}'",
            path
        ));
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

fn emit_error<R: Runtime>(
    manager: &(impl Manager<R> + Emitter<R>),
    deck_id: &str,
    error_message: &str,
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
) -> Result<(), String>
where
    F: FnOnce(&mut PlaybackState),
{
    let mut states = state_map
        .lock()
        .map_err(|_| "Failed to lock logical state".to_string())?;
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
    app_handle: AppHandle, // Can get app_handle directly
    deck_id: String,
) -> Result<(), String> {
    log::info!("CMD Init: Deck '{}'", deck_id);
    // Ensure initial logical state exists
    update_logical_state(
        &state.logical_playback_states,
        &deck_id,
        &app_handle,
        |_| {},
    )?;

    state
        .audio_command_sender
        .send(AudioThreadCommand::InitDeck(deck_id))
        .await
        .map_err(|e| format!("CMD Init: Failed to send command: {}", e))
}

#[tauri::command]
pub async fn load_track(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    deck_id: String,
    path: String,
) -> Result<(), String> {
    log::info!("CMD Load: Deck '{}', Path '{}'", deck_id, path);
    // Update logical state to loading immediately
    update_logical_state(&state.logical_playback_states, &deck_id, &app_handle, |s| {
        s.is_loading = true;
        s.is_playing = false;
        s.error = None;
        s.current_time = 0.0;
        s.duration = 0.0;
    })?;

    state
        .audio_command_sender
        .send(AudioThreadCommand::LoadTrack { deck_id, path })
        .await
        .map_err(|e| format!("CMD Load: Failed to send command: {}", e))
}

#[tauri::command]
pub async fn play_track(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    deck_id: String,
) -> Result<(), String> {
    log::info!("CMD Play: Deck '{}'", deck_id);
    // Optimistically update logical state (audio thread confirms)
    update_logical_state(&state.logical_playback_states, &deck_id, &app_handle, |s| {
        s.is_playing = true;
        s.error = None;
    })?;

    state
        .audio_command_sender
        .send(AudioThreadCommand::Play(deck_id))
        .await
        .map_err(|e| format!("CMD Play: Failed to send command: {}", e))
}

#[tauri::command]
pub async fn pause_track(
    state: State<'_, AppState>,
    _app_handle: AppHandle, // No longer needed for optimistic update
    deck_id: String,
) -> Result<(), String> {
    log::info!("CMD Pause: Deck '{}'", deck_id);
    // REMOVED Optimistic update: Rely solely on audio thread event
    // update_logical_state(&state.logical_playback_states, &deck_id, &app_handle, |s| {
    //     s.is_playing = false;
    // })?;

    state
        .audio_command_sender
        .send(AudioThreadCommand::Pause(deck_id))
        .await
        .map_err(|e| format!("CMD Pause: Failed to send command: {}", e))
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
        .map_err(|_| "CMD GetState: Failed to lock logical state".to_string())?;
    states
        .get(&deck_id)
        .cloned()
        .ok_or_else(|| format!("CMD GetState: Logical state not found for '{}'", deck_id))
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
            // We could potentially emit a global error event here
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
        let mut time_update_interval = tokio::time::interval(Duration::from_millis(100));

        while !should_shutdown {
            tokio::select! {
                maybe_command = receiver.recv() => {
                    match maybe_command {
                        Some(command) => {
                            log::debug!("Audio Thread Received: {:?}", command);
                            match command {
                                AudioThreadCommand::InitDeck(deck_id) => {
                                    if !local_deck_states.contains_key(&deck_id) {
                                        match Sink::try_new(&handle) {
                                            Ok(sink) => {
                                                local_deck_states.insert(deck_id.clone(), AudioThreadDeckState {
                                                    sink,
                                                    duration: Duration::ZERO,
                                                    is_playing: false,
                                                    playback_start_time: None,
                                                    paused_position: None,
                                                });
                                                log::info!("Audio Thread: Initialized sink for deck '{}'", deck_id);
                                            }
                                            Err(e) => {
                                                log::error!("Audio Thread: Failed to create sink for deck '{}': {}", deck_id, e);
                                                emit_error(&app_handle, &deck_id, &format!("Failed to create sink: {}", e));
                                            }
                                        }
                                    } else {
                                        log::warn!("Audio Thread: Deck '{}' already initialized locally.", deck_id);
                                    }
                                }
                                AudioThreadCommand::LoadTrack { deck_id, path } => {
                                    log::debug!("Audio Thread: Handling LoadTrack for '{}'", deck_id);

                                    // Clone path *before* the closure for use in logging later
                                    let path_for_log = path.clone();
                                    let path_for_decode = path; // Ownership moves to spawn_blocking

                                    // Use spawn_blocking for synchronous, CPU-bound work
                                    let decode_handle = tokio::task::spawn_blocking(move || {
                                        // path_for_decode (original path) is moved here
                                        decode_audio_for_playback(&path_for_decode)
                                    });

                                    // Await the result from the blocking thread
                                    match decode_handle.await {
                                        Ok(decode_result) => {
                                            // Now process the inner Result from decode_audio_for_playback
                                            match local_deck_states.get_mut(&deck_id) {
                                                Some(deck_state) => {
                                                    match decode_result {
                                                        Ok((samples, sample_rate)) => {
                                                            let duration = Duration::from_secs_f64(samples.len() as f64 / sample_rate as f64);
                                                            // Use the cloned path here for logging
                                                            log::info!("Audio Thread: Decoded '{}'. Duration: {:?}, Rate: {}", path_for_log, duration, sample_rate);

                                                            deck_state.sink.stop();
                                                            deck_state.sink.clear();
                                                            let source = SamplesBuffer::new(1, sample_rate as u32, samples);
                                                            deck_state.sink.append(source);
                                                            deck_state.sink.pause();

                                                            deck_state.duration = duration;
                                                            deck_state.is_playing = false;
                                                            deck_state.playback_start_time = None;
                                                            deck_state.paused_position = Some(Duration::ZERO);

                                                            let state_to_emit = PlaybackState {
                                                                is_playing: false,
                                                                is_loading: false,
                                                                current_time: 0.0,
                                                                duration: duration.as_secs_f64(),
                                                                error: None,
                                                            };
                                                            emit_state_update(&app_handle, &deck_id, &state_to_emit);
                                                        }
                                                        Err(e) => {
                                                            log::error!("Audio Thread: Decode failed for deck '{}': {}", deck_id, e);
                                                            emit_error(&app_handle, &deck_id, &e);
                                                             let state_to_emit = PlaybackState {
                                                                 is_loading: false,
                                                                 error: Some(e),
                                                                 ..Default::default()
                                                             };
                                                             emit_state_update(&app_handle, &deck_id, &state_to_emit);
                                                        }
                                                    }
                                                }
                                                None => {
                                                    log::error!("Audio Thread: Deck '{}' not found after decode completed.", deck_id);
                                                }
                                            }
                                        }
                                        Err(join_error) => {
                                            // Handle error if the blocking task itself panicked
                                            log::error!("Audio Thread: Decode task panicked for deck '{}': {}", deck_id, join_error);
                                            let error_msg = format!("Audio decoding task failed: {}", join_error);
                                            emit_error(&app_handle, &deck_id, &error_msg);
                                            let state_to_emit = PlaybackState {
                                                is_loading: false,
                                                error: Some(error_msg),
                                                ..Default::default()
                                            };
                                            emit_state_update(&app_handle, &deck_id, &state_to_emit);
                                        }
                                    }
                                }
                                AudioThreadCommand::Play(deck_id) => {
                                    log::debug!("Audio Thread: Handling Play for '{}'", deck_id);
                                    if let Some(deck_state) = local_deck_states.get_mut(&deck_id) {
                                        if deck_state.sink.empty() {
                                            log::warn!("Audio Thread: Play called on empty sink for '{}'", deck_id);
                                            emit_error(&app_handle, &deck_id, "Cannot play: No track loaded.");
                                        } else if !deck_state.is_playing {
                                            deck_state.sink.play();
                                            deck_state.is_playing = true;
                                            deck_state.playback_start_time = Some(Instant::now());
                                            log::info!("Audio Thread: Playback started for '{}'", deck_id);
                                            // Emit state update
                                            let state_to_emit = PlaybackState {
                                                is_playing: true,
                                                current_time: deck_state.paused_position.unwrap_or_default().as_secs_f64(),
                                                duration: deck_state.duration.as_secs_f64(),
                                                is_loading: false,
                                                error: None,
                                            };
                                            emit_state_update(&app_handle, &deck_id, &state_to_emit);
                                        } else {
                                            log::trace!("Audio Thread: Already playing '{}'", deck_id);
                                        }
                                    }
                                }
                                AudioThreadCommand::Pause(deck_id) => {
                                    log::debug!("Audio Thread: Handling Pause START for '{}'", deck_id);
                                    if let Some(deck_state) = local_deck_states.get_mut(&deck_id) {
                                        if deck_state.is_playing {
                                            deck_state.sink.pause();
                                            deck_state.is_playing = false;
                                            // Calculate and store paused position
                                            let elapsed_since_play = deck_state.playback_start_time.map_or(Duration::ZERO, |st| st.elapsed());
                                            let previous_pos = deck_state.paused_position.unwrap_or_default();
                                            let new_paused_pos = previous_pos + elapsed_since_play;
                                            deck_state.paused_position = Some(new_paused_pos);
                                            deck_state.playback_start_time = None;
                                            log::info!("Audio Thread: Playback paused for '{}' at {:?}", deck_id, new_paused_pos);

                                            // Emit state update
                                            let state_to_emit = PlaybackState {
                                                is_playing: false,
                                                current_time: new_paused_pos.as_secs_f64().min(deck_state.duration.as_secs_f64()),
                                                duration: deck_state.duration.as_secs_f64(),
                                                is_loading: false,
                                                error: None,
                                            };
                                            emit_state_update(&app_handle, &deck_id, &state_to_emit);
                                        } else {
                                            log::trace!("Audio Thread: Already paused '{}'", deck_id);
                                        }
                                    }
                                    log::debug!("Audio Thread: Handling Pause END for '{}'", deck_id);
                                }
                                AudioThreadCommand::Seek { deck_id, position_seconds } => {
                                    log::debug!("Audio Thread: Handling Seek START for '{}' to {:.2}s", deck_id, position_seconds);
                                    if let Some(deck_state) = local_deck_states.get_mut(&deck_id) {
                                        if deck_state.duration == Duration::ZERO {
                                            log::warn!("Audio Thread: Cannot seek deck '{}', duration unknown.", deck_id);
                                            emit_error(&app_handle, &deck_id, "Cannot seek: Track duration not known.");
                                            continue; // Use continue within the select! loop arm
                                        }
                                        let target_duration = Duration::from_secs_f64(position_seconds.max(0.0));

                                        log::debug!("Audio Thread: Calling sink.try_seek({:?}) for '{}'", target_duration, deck_id);
                                        match deck_state.sink.try_seek(target_duration) {
                                            Ok(_) => {
                                                let clamped_time_secs = position_seconds.max(0.0).min(deck_state.duration.as_secs_f64());
                                                let seek_pos_duration = Duration::from_secs_f64(clamped_time_secs);
                                                log::info!("Audio Thread: Seek successful for '{}' to {:?}", deck_id, seek_pos_duration);

                                                // Update timing state
                                                if deck_state.is_playing {
                                                    deck_state.playback_start_time = Some(Instant::now());
                                                    deck_state.paused_position = Some(seek_pos_duration);
                                                } else {
                                                    deck_state.playback_start_time = None;
                                                    deck_state.paused_position = Some(seek_pos_duration);
                                                }

                                                // Emit state update
                                                let state_to_emit = PlaybackState {
                                                    is_playing: deck_state.is_playing,
                                                    current_time: clamped_time_secs,
                                                    duration: deck_state.duration.as_secs_f64(),
                                                    is_loading: false,
                                                    error: None,
                                                };
                                                emit_state_update(&app_handle, &deck_id, &state_to_emit);
                                                log::debug!("Audio Thread: Emitted seek state update for '{}'", deck_id);
                                            }
                                            Err(e) => {
                                                log::error!("Audio Thread: Seek failed for deck '{}': {:?}", deck_id, e);
                                                emit_error(&app_handle, &deck_id, &format!("Seek failed: {:?}", e));
                                            }
                                        }
                                    }
                                    log::debug!("Audio Thread: Handling Seek END for '{}'", deck_id);
                                }
                                AudioThreadCommand::CleanupDeck(deck_id) => {
                                    if local_deck_states.remove(&deck_id).is_some() {
                                        log::info!("Audio Thread: Cleaned up '{}'", deck_id);
                                    }
                                }
                                AudioThreadCommand::Shutdown => {
                                    log::info!("Audio Thread: Shutdown received. Cleaning up decks.");
                                    local_deck_states.clear(); // Stop all sinks implicitly
                                    receiver.close();
                                    should_shutdown = true;
                                }
                            }
                        }
                        None => {
                           log::info!("Audio Thread: Command channel closed. Exiting loop.");
                           should_shutdown = true; // Exit if channel closes
                        }
                    }
                }

                // Handle periodic time updates for playing decks
                _ = time_update_interval.tick(), if !should_shutdown => {
                    // Iterate through decks and update/emit time for playing ones
                    for (deck_id, deck_state) in local_deck_states.iter_mut() {
                         if deck_state.is_playing && !deck_state.sink.is_paused() && !deck_state.sink.empty() {
                             let current_elapsed = deck_state.playback_start_time.map_or(Duration::ZERO, |st| st.elapsed());
                             let base_pos = deck_state.paused_position.unwrap_or_default();
                             let total_pos = base_pos + current_elapsed;
                             let current_time_secs = total_pos.as_secs_f64().min(deck_state.duration.as_secs_f64());

                             // Construct PlaybackState to emit
                             let state_update = PlaybackState {
                                 current_time: current_time_secs,
                                 is_playing: deck_state.is_playing,
                                 duration: deck_state.duration.as_secs_f64(),
                                 is_loading: false,
                                 error: None,
                             };
                             // Emit only if time changed significantly to reduce traffic
                             // (Need to fetch logical state or store previous emitted time)
                             // For now, emit unconditionally during testing.
                             emit_state_update(&app_handle, deck_id, &state_update);

                             // Check if track finished
                             if current_time_secs >= deck_state.duration.as_secs_f64() && deck_state.duration > Duration::ZERO {
                                 log::info!("Audio Thread: Track finished for deck '{}'", deck_id);
                                 deck_state.sink.pause(); // Pause sink technically
                                 deck_state.is_playing = false;
                                 deck_state.playback_start_time = None;
                                 deck_state.paused_position = Some(deck_state.duration);
                                  // Emit final paused state
                                  let final_state = PlaybackState {
                                      is_playing: false,
                                      current_time: deck_state.duration.as_secs_f64(),
                                      duration: deck_state.duration.as_secs_f64(),
                                      is_loading: false,
                                      error: None,
                                  };
                                  emit_state_update(&app_handle, deck_id, &final_state);
                             }
                         }
                     }
                }
            }
            // Loop continues until should_shutdown is true
        }
        log::info!("Audio thread loop finished.");
    });
    log::info!("Audio thread has stopped.");
}

// --- TODO ---
// - Implement actual Load/Play/Pause/Seek logic within the audio thread loop.
// - Ensure correct state synchronization between logical state and audio thread state.
// - Handle errors gracefully within the audio thread (e.g., decode errors) and emit them.
// - Add volume control command/logic.
// - Implement robust shutdown handling (e.g., send Shutdown command on app close).
