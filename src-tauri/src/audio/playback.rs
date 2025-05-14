use rodio::OutputStream;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tokio::sync::mpsc;

use crate::audio::config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS;
use crate::audio::playback::commands::AudioThreadCommand;

pub mod state;
use state::AudioThreadDeckState;
pub mod commands;
mod events;
mod handlers;
pub mod sync;
pub mod time;

// --- PLL Constants --- // MOVED to sync.rs
// const PLL_KP: f32 = 0.0075;
// const MAX_PLL_PITCH_ADJUSTMENT: f32 = 0.01;

// --- Event Emitter Helpers ---

// --- Audio Thread Implementation ---

pub fn run_audio_thread<R: Runtime>(app_handle: AppHandle<R>, mut receiver: mpsc::Receiver<AudioThreadCommand>) {
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
            Duration::from_millis(AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS)
        );

        while !should_shutdown {
            tokio::select! {
                maybe_command = receiver.recv() => {
                    match maybe_command {
                        Some(command) => {
                            log::debug!("Audio Thread Received: {:?}", command);
                            match command {
                                AudioThreadCommand::InitDeck(deck_id) => {
                                    handlers::audio_thread_handle_init(&deck_id, &mut local_deck_states, &handle, &app_handle);
                                }
                                AudioThreadCommand::LoadTrack { deck_id, path, original_bpm, first_beat_sec } => {
                                    handlers::audio_thread_handle_load(deck_id, path, original_bpm, first_beat_sec, &mut local_deck_states, &app_handle).await;
                                }
                                AudioThreadCommand::Play(deck_id) => {
                                    handlers::audio_thread_handle_play(&deck_id, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::Pause(deck_id) => {
                                    handlers::audio_thread_handle_pause(&deck_id, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::Seek { deck_id, position_seconds } => {
                                    handlers::audio_thread_handle_seek(&deck_id, position_seconds, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::SetFaderLevel { deck_id, level } => {
                                    handlers::audio_thread_handle_set_fader_level(&deck_id, level, &mut local_deck_states);
                                }
                                AudioThreadCommand::SetTrimGain { deck_id, gain } => {
                                    handlers::audio_thread_handle_set_trim_gain(&deck_id, gain, &mut local_deck_states);
                                }
                                AudioThreadCommand::SetEq { deck_id, params } => {
                                    handlers::audio_thread_handle_set_eq(&deck_id, params, &mut local_deck_states);
                                }
                                AudioThreadCommand::SetCue { deck_id, position_seconds } => {
                                    handlers::audio_thread_handle_set_cue(&deck_id, position_seconds, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::CleanupDeck(deck_id) => {
                                    handlers::audio_thread_handle_cleanup(&deck_id, &mut local_deck_states);
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
                                    handlers::audio_thread_handle_set_pitch_rate(&deck_id, rate, is_manual_adjustment, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::EnableSync { slave_deck_id, master_deck_id } => {
                                    sync::audio_thread_handle_enable_sync(&slave_deck_id, &master_deck_id, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::DisableSync { deck_id } => {
                                    sync::audio_thread_handle_disable_sync(&deck_id, &mut local_deck_states, &app_handle);
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
                    time::process_time_slice_updates(&mut local_deck_states, &app_handle);
                }
            }
        }
        log::info!("Audio thread loop finished.");
    });
    log::info!("Audio thread has stopped.");
}

// --- Private Handler Functions for Audio Thread Commands ---
