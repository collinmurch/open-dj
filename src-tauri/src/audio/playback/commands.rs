use tokio::sync::oneshot;
use crate::audio::types::EqParams; // EqParams is still in audio::types
use super::state::AppState;      // AppState is in the parent's state module
use crate::audio::devices::store::AudioDeviceStore;
use tauri::State;

// --- Audio Thread Commands ---
#[derive(Debug)]
pub enum AudioThreadCommand {
    InitDeck(String),
    LoadTrack {
        deck_id: String,
        path: String,
        original_bpm: Option<f32>,
        first_beat_sec: Option<f32>,
        output_device_name: Option<String>,
    },
    Play(String),
    Pause(String),
    Seek {
        deck_id: String,
        position_seconds: f64,
    },
    SetFaderLevel {
        deck_id: String,
        level: f32,
    },
    SetTrimGain {
        deck_id: String,
        gain: f32, // This is already linear gain in the command struct
    },
    SetEq {
        deck_id: String,
        params: EqParams,
    },
    SetCue {
        deck_id: String,
        position_seconds: f64,
    },
    SetPitchRate {
        deck_id: String,
        rate: f32,
        is_manual_adjustment: bool,
    },
    EnableSync {
        slave_deck_id: String,
        master_deck_id: String,
    },
    DisableSync {
        deck_id: String,
    },
    CleanupDeck(String),
    Shutdown(oneshot::Sender<()>),
}

// --- Tauri Commands ---

#[tauri::command]
pub async fn init_player(deck_id: String, app_state: State<'_, AppState>) -> Result<(), String> {
    log::info!("CMD: Init player for deck: {}", deck_id);
    app_state
        .get_command_sender()
        .send(AudioThreadCommand::InitDeck(deck_id.clone()))
        .await
        .map_err(|e| {
            log::error!("Failed to send InitDeck command for {}: {}", deck_id, e);
            e.to_string()
        })?;

    Ok(())
}

#[tauri::command]
pub async fn load_track(
    deck_id: String,
    path: String,
    original_bpm: Option<f32>,
    first_beat_sec: Option<f32>,
    app_state: State<'_, AppState>,
    _device_store: State<'_, AudioDeviceStore>,
) -> Result<(), String> {
    log::info!(
        "CMD: Load track '{}' for deck: {}. BPM: {:?}, First Beat: {:?}",
        path,
        deck_id,
        original_bpm,
        first_beat_sec
    );

    // Master output always uses the default device
    let output_device_name = if deck_id == "A" || deck_id == "B" {
        // Master output will always use the default system output device
        log::info!("CMD: Using default system output device for deck {}", deck_id);
        None // None means use default device
    } else {
        None
    };

    app_state
        .get_command_sender()
        .send(AudioThreadCommand::LoadTrack {
            deck_id,
            path,
            original_bpm,
            first_beat_sec,
            output_device_name,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn play_track(deck_id: String, app_state: State<'_, AppState>) -> Result<(), String> {
    log::info!("CMD: Play track for deck: {}", deck_id);
    app_state
        .get_command_sender()
        .send(AudioThreadCommand::Play(deck_id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pause_track(deck_id: String, app_state: State<'_, AppState>) -> Result<(), String> {
    log::info!("CMD: Pause track for deck: {}", deck_id);
    app_state
        .get_command_sender()
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
        .get_command_sender()
        .send(AudioThreadCommand::Seek {
            deck_id,
            position_seconds,
        })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_fader_level(
    deck_id: String,
    level: f32,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    log::debug!("CMD: Set fader level for deck {}: {}", deck_id, level);
    app_state
        .get_command_sender()
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
    let linear_gain = if gain_db <= -96.0 {
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
        .get_command_sender()
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
        .get_command_sender()
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
        .get_command_sender()
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
    app_state
        .get_command_sender()
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
        .get_command_sender()
        .send(AudioThreadCommand::SetPitchRate { deck_id, rate, is_manual_adjustment: true })
        .await
        .map_err(|e| e.to_string())
}

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
        .get_command_sender()
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
        .get_command_sender()
        .send(AudioThreadCommand::DisableSync { deck_id })
        .await
        .map_err(|e| e.to_string())
} 