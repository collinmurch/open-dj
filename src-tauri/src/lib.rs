mod audio_analysis;
mod audio_effects;
mod audio_playback;
mod audio_processor;
mod bpm_analyzer;
mod config;
mod errors;
mod playback_types;

use audio_playback::AppState;
use playback_types::AudioThreadCommand;
use tauri::WindowEvent; // Removed Manager, AppHandle, Emitter, Runtime
use tokio::sync::oneshot;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logging
    // Consider using a more robust logger like tracing or fern
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Create the channel for audio commands
    let (audio_cmd_tx, audio_cmd_rx) = tokio::sync::mpsc::channel::<AudioThreadCommand>(32); // 32 is buffer size

    tauri::Builder::default()
        // Manage the sender end of the channel and the logical states
        .manage(AppState::new(audio_cmd_tx.clone())) // Clone sender for state
        .setup(|app| {
            // Spawn the dedicated audio thread
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                audio_playback::run_audio_thread(app_handle, audio_cmd_rx);
            });
            Ok(())
        })
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            audio_processor::analyze_features_batch,
            audio_playback::init_player,
            audio_playback::load_track,
            audio_playback::play_track,
            audio_playback::pause_track,
            audio_playback::seek_track,
            audio_playback::get_playback_state,
            audio_playback::set_fader_level,
            audio_playback::set_trim_gain,
            audio_playback::cleanup_player,
            audio_playback::set_eq_params
        ])
        .on_window_event(move |window, event| {
            // Send shutdown command only once when close is requested
            if let WindowEvent::CloseRequested { api, .. } = event {
                log::info!("Window close requested. Sending Shutdown command to audio thread.");
                // Prevent the window from closing immediately
                api.prevent_close();

                // Create the shutdown signalling channel
                let (shutdown_tx, shutdown_rx) = oneshot::channel();

                // Clone the command sender again for the event handler closure
                let audio_cmd_tx_clone = audio_cmd_tx.clone();
                let window_clone = window.clone();

                // Send shutdown command in a separate task to avoid blocking event loop
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = audio_cmd_tx_clone
                        .send(AudioThreadCommand::Shutdown(shutdown_tx))
                        .await
                    {
                        log::error!("Failed to send Shutdown command to audio thread: {}", e);
                        // If sending fails, we can probably just close the window
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

                    // Optionally wait a short moment for thread to potentially process (Probably not needed now)
                    // tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    log::info!("Proceeding with window close after sending Shutdown command.");
                    // Now allow the window to close
                    if let Err(e) = window_clone.close() {
                        log::error!("Failed to close window: {}", e);
                    }
                });
            }
        })
        // TODO: Add graceful shutdown for audio thread - DONE
        // .on_window_event(|window, event| match event { ... })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
