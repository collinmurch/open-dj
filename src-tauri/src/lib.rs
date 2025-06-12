mod audio;

use audio::config::AUDIO_BUFFER_CHAN_SIZE;
use audio::playback::state::AppState;
use audio::playback::commands::AudioThreadCommand;
use tauri::Manager;
use tauri::WindowEvent;
use tokio::sync::oneshot;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let (audio_cmd_tx, audio_cmd_rx) =
        tokio::sync::mpsc::channel::<AudioThreadCommand>(AUDIO_BUFFER_CHAN_SIZE);
    let audio_cmd_tx_for_event_handler = audio_cmd_tx.clone();

    tauri::Builder::default()
        .setup(move |app| {
            let app_handle = app.handle().clone();
            app.manage(AppState::new(audio_cmd_tx.clone()));

            // Spawn the dedicated audio thread
            let app_handle_for_thread = app_handle.clone();
            std::thread::spawn(move || {
                if let Err(e) = audio::playback::run_audio_thread(app_handle_for_thread, audio_cmd_rx) {
                    log::error!("Audio thread exited with error: {}", e);
                }
            });
            Ok(())
        })
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            audio::processor::analyze_features_batch,
            audio::processor::analyze_features_batch_with_cache,
            audio::processor::get_track_volume_analysis,
            audio::processor::get_track_complete_analysis,
            audio::processor::analyze_features_and_waveforms_batch,
            audio::processor::analyze_features_and_waveforms_batch_with_cache,
            audio::cache::commands::ensure_cache_directory,
            audio::cache::commands::get_cache_stats,
            audio::cache::commands::cleanup_cache,
            audio::cache::commands::rebuild_cache_index,
            audio::cache::commands::clear_cache,
            audio::playback::commands::init_player,
            audio::playback::commands::load_track,
            audio::playback::commands::play_track,
            audio::playback::commands::pause_track,
            audio::playback::commands::seek_track,
            audio::playback::commands::set_fader_level,
            audio::playback::commands::set_trim_gain,
            audio::playback::commands::set_eq_params,
            audio::playback::commands::set_cue_point,
            audio::playback::commands::cleanup_player,
            audio::playback::commands::set_pitch_rate,
            audio::playback::commands::enable_sync,
            audio::playback::commands::disable_sync
        ])
        .on_window_event(move |window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                log::info!("Window close requested. Sending Shutdown command to audio thread.");
                // Prevent the window from closing immediately
                api.prevent_close();

                let (shutdown_tx, shutdown_rx) = oneshot::channel();

                // Use the cloned command sender for the event handler closure
                let audio_cmd_tx_clone = audio_cmd_tx_for_event_handler.clone();
                let window_clone = window.clone();

                // Send shutdown command in a separate task to avoid blocking event loop
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = audio_cmd_tx_clone
                        .send(AudioThreadCommand::Shutdown(shutdown_tx))
                        .await
                    {
                        log::error!("Failed to send Shutdown command to audio thread: {}", e);
                        if let Err(close_err) = window_clone.close() {
                            log::error!("Failed to close window after send error: {}", close_err);
                        }
                        return;
                    }

                    // Wait for the audio thread to signal completion
                    log::info!("Waiting for audio thread shutdown confirmation...");
                    match shutdown_rx.await {
                        Ok(_) => log::info!("Audio thread confirmed shutdown."),
                        Err(e) => log::error!(
                            "Failed to receive shutdown confirmation from audio thread: {}",
                            e
                        ),
                    }

                    log::info!("Proceeding with window close after sending Shutdown command.");
                    if let Err(e) = window_clone.close() {
                        log::error!("Failed to close window: {}", e);
                    }
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
