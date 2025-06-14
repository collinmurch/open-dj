use tauri::State;
use super::store::{AudioDeviceStore, AudioDeviceState};
use crate::audio::playback::handlers::cue_output;

#[tauri::command]
pub async fn get_audio_devices(
    device_store: State<'_, AudioDeviceStore>,
) -> Result<AudioDeviceState, String> {
    let mut state = device_store
        .get_state()
        .map_err(|e| format!("Failed to get audio device state: {}", e))?;
    
    log::info!("get_audio_devices - returning {} output devices (filtered out {} input devices)", 
        state.devices.output_devices.len(), 
        state.devices.input_devices.len());
    
    for (i, device) in state.devices.output_devices.iter().enumerate() {
        log::info!("  Output device {}: {}", i, device.name);
    }
    
    // Clear input devices since we only need output devices for cue output
    state.devices.input_devices.clear();
    state.devices.default_input = None;
    
    Ok(state)
}


#[tauri::command]
pub async fn set_cue_output_device(
    device_store: State<'_, AudioDeviceStore>,
    device_name: Option<String>,
) -> Result<(), String> {
    device_store
        .set_cue_output(device_name)
        .map_err(|e| format!("Failed to set cue output device: {}", e))?;
    
    // Notify the cue output manager about the device change
    cue_output::update_cue_device(&device_store)
        .map_err(|e| format!("Failed to update cue output manager: {}", e))?;
    
    Ok(())
}

#[tauri::command]
pub async fn refresh_audio_devices(
    device_store: State<'_, AudioDeviceStore>,
) -> Result<AudioDeviceState, String> {
    device_store
        .refresh_devices()
        .map_err(|e| format!("Failed to refresh audio devices: {}", e))?;
    
    let mut state = device_store
        .get_state()
        .map_err(|e| format!("Failed to get audio device state after refresh: {}", e))?;
    
    // Clear input devices since we only need output devices for cue output
    state.devices.input_devices.clear();
    state.devices.default_input = None;
    
    Ok(state)
}

#[tauri::command]
pub async fn set_cue_deck(
    deck_id: Option<String>,
) -> Result<(), String> {
    // Update the cue deck selection
    cue_output::set_cue_deck(deck_id)
        .map_err(|e| format!("Failed to set cue deck: {}", e))?;
    
    Ok(())
}