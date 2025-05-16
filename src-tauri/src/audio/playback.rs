use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tokio::sync::mpsc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::audio::config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS;
use crate::audio::playback::commands::AudioThreadCommand;
use crate::audio::errors::PlaybackError;

pub mod state;
use state::AudioThreadDeckState;
pub mod commands;
mod events;
mod handlers;
pub mod sync;
pub mod time;

pub fn run_audio_thread<R: Runtime>(app_handle: AppHandle<R>, mut receiver: mpsc::Receiver<AudioThreadCommand>) -> Result<(), PlaybackError> {
    log::info!("Audio Thread: Starting...");

    let cpal_host: cpal::Host = cpal::default_host();

    let cpal_device: cpal::Device;
    match cpal_host.default_output_device() {
        Some(d) => {
            cpal_device = d;
        }
        None => {
            log::error!("Audio Thread: Failed to get default CPAL output device (it was None).");
            return Err(PlaybackError::CpalNoDefaultOutputDevice("No default CPAL output device found (Option was None)".to_string()));
        }
    }
    
    match cpal_device.name() {
        Ok(name) => log::info!("Audio Thread: Using CPAL output device: {}", name),
        Err(e) => log::warn!("Audio Thread: Could not get CPAL output device name: {}",e),
    };
    

    let mut local_deck_states: HashMap<String, AudioThreadDeckState> = HashMap::new();

    log::info!("Audio Thread: Building Tokio current_thread runtime...");
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            log::error!("Audio Thread: Failed to build Tokio runtime: {}", e);
            return Err(PlaybackError::OutputStreamInitError(format!("Failed to build Tokio runtime: {}", e)));
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
                                    handlers::audio_thread_handle_init(&deck_id, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::LoadTrack { deck_id, path, original_bpm, first_beat_sec } => {
                                    handlers::audio_thread_handle_load(deck_id, path, original_bpm, first_beat_sec, &mut local_deck_states, &cpal_device, &app_handle).await;
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
                                    for (_deck_id, mut deck_state) in local_deck_states.drain() { 
                                        if let Some(stream) = deck_state.cpal_stream.take() {
                                            // Stream stops and resources are released when it's dropped.
                                            drop(stream); 
                                            // log::trace!("Dropped CPAL stream for deck '{}' during shutdown.", _deck_id); // Optional trace
                                        }
                                    }
                                    should_shutdown = true;
                                    if shutdown_complete_tx.send(()).is_err() {
                                         log::error!("Audio Thread: Failed to send shutdown completion signal.");
                                    }
                                }
                                AudioThreadCommand::SetPitchRate { deck_id, rate, is_manual_adjustment } => {
                                    handlers::audio_thread_handle_set_pitch_rate(&deck_id, rate, is_manual_adjustment, &mut local_deck_states, &app_handle);
                                }
                                AudioThreadCommand::EnableSync { slave_deck_id, master_deck_id } => {
                                    sync::audio_thread_handle_enable_sync_async(&slave_deck_id, &master_deck_id, &mut local_deck_states, &app_handle).await;
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
    Ok(())
}

// --- Private Handler Functions for Audio Thread Commands ---
