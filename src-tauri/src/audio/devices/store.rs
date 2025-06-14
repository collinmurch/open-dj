use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};
use super::{AudioDeviceList, detect_audio_devices};
use crate::audio::errors::PlaybackError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceSelection {
    pub cue_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceState {
    pub devices: AudioDeviceList,
    pub selection: AudioDeviceSelection,
}

pub struct AudioDeviceStore {
    state: Arc<Mutex<AudioDeviceState>>,
}

impl AudioDeviceStore {
    pub fn new() -> Result<Self, PlaybackError> {
        let devices = detect_audio_devices()?;
        
        let state = AudioDeviceState {
            devices,
            selection: AudioDeviceSelection {
                cue_output: None,
            },
        };
        
        Ok(AudioDeviceStore {
            state: Arc::new(Mutex::new(state)),
        })
    }
    
    pub fn get_state(&self) -> Result<AudioDeviceState, PlaybackError> {
        let state = self.state.lock().map_err(|e| {
            PlaybackError::CpalNoDefaultOutputDevice(format!("Failed to lock device store: {}", e))
        })?;
        Ok(state.clone())
    }
    
    
    pub fn set_cue_output(&self, device_name: Option<String>) -> Result<(), PlaybackError> {
        let mut state = self.state.lock().map_err(|e| {
            PlaybackError::CpalNoDefaultOutputDevice(format!("Failed to lock device store: {}", e))
        })?;
        
        // Validate device exists if provided
        if let Some(ref name) = device_name {
            if !state.devices.output_devices.iter().any(|d| d.name == *name) {
                return Err(PlaybackError::CpalNoDefaultOutputDevice(
                    format!("Cue output device '{}' not found", name)
                ));
            }
        }
        
        state.selection.cue_output = device_name;
        log::info!("Cue output device set to: {:?}", state.selection.cue_output);
        Ok(())
    }
    
    pub fn refresh_devices(&self) -> Result<(), PlaybackError> {
        let mut state = self.state.lock().map_err(|e| {
            PlaybackError::CpalNoDefaultOutputDevice(format!("Failed to lock device store: {}", e))
        })?;
        
        let new_devices = detect_audio_devices()?;
        
        // Check if currently selected devices still exist
        if let Some(ref cue) = state.selection.cue_output {
            if !new_devices.output_devices.iter().any(|d| d.name == *cue) {
                log::warn!("Cue output device '{}' no longer available, clearing selection", cue);
                state.selection.cue_output = None;
            }
        }
        
        state.devices = new_devices;
        log::info!("Audio devices refreshed");
        Ok(())
    }
    
    
    #[allow(dead_code)] // Ready for future cue routing implementation
    pub fn get_cue_output_device(&self) -> Result<Option<String>, PlaybackError> {
        let state = self.state.lock().map_err(|e| {
            PlaybackError::CpalNoDefaultOutputDevice(format!("Failed to lock device store: {}", e))
        })?;
        Ok(state.selection.cue_output.clone())
    }
}