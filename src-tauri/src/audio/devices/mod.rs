use serde::{Deserialize, Serialize};
use crate::audio::errors::PlaybackError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
    pub device_type: AudioDeviceType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioDeviceType {
    Input,
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceList {
    pub output_devices: Vec<AudioDevice>,
    pub input_devices: Vec<AudioDevice>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
}

// Platform-specific modules
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "linux")]
pub mod linux;

// Cross-platform fallback using cpal
pub mod cpal_fallback;

// Audio device store for managing selection state
pub mod store;

// Tauri commands for device management
pub mod commands;

/// Detects all available audio devices on the system using platform-specific methods
pub fn detect_audio_devices() -> Result<AudioDeviceList, PlaybackError> {
    log::info!("Detecting audio devices...");

    #[cfg(target_os = "macos")]
    {
        macos::detect_devices()
    }

    #[cfg(target_os = "windows")]
    {
        windows::detect_devices()
    }

    #[cfg(target_os = "linux")]
    {
        linux::detect_devices()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        cpal_fallback::detect_devices()
    }
}

/// Logs all detected audio devices to console
pub fn log_audio_devices() {
    match detect_audio_devices() {
        Ok(devices) => {
            log::info!("=== AUDIO DEVICES DETECTED ===");
            
            if let Some(default_output) = &devices.default_output {
                log::info!("Default output device: {}", default_output);
            }
            
            if let Some(default_input) = &devices.default_input {
                log::info!("Default input device: {}", default_input);
            }
            
            log::info!("Output devices ({}): ", devices.output_devices.len());
            for (i, device) in devices.output_devices.iter().enumerate() {
                log::info!("  {}. {} {}", i + 1, device.name, if device.is_default { "(default)" } else { "" });
            }
            
            log::info!("Input devices ({}): ", devices.input_devices.len());
            for (i, device) in devices.input_devices.iter().enumerate() {
                log::info!("  {}. {} {}", i + 1, device.name, if device.is_default { "(default)" } else { "" });
            }
            
            log::info!("==============================");
        }
        Err(e) => {
            log::error!("Failed to detect audio devices: {}", e);
        }
    }
}

/// Finds a CPAL output device by name, returns None if device name is None (use default)
/// Tries exact match first, then partial matching for Core Audio detected devices
pub fn find_cpal_output_device(device_name: Option<&str>) -> Result<Option<cpal::Device>, PlaybackError> {
    use cpal::traits::{HostTrait, DeviceTrait};
    
    if device_name.is_none() {
        // Use default device
        return Ok(None);
    }
    
    let device_name = device_name.unwrap();
    let host = cpal::default_host();
    
    match host.output_devices() {
        Ok(devices) => {
            let device_list: Vec<_> = devices.collect();
            
            // First try exact name match
            for device in &device_list {
                if let Ok(name) = device.name() {
                    if name == device_name {
                        log::info!("Found CPAL output device (exact match): {}", device_name);
                        return Ok(Some(device.clone()));
                    }
                }
            }
            
            // Then try partial matching (Core Audio names might be different in CPAL)
            for device in &device_list {
                if let Ok(name) = device.name() {
                    // Try bidirectional partial matching
                    if name.contains(device_name) || device_name.contains(&name) {
                        log::info!("Found CPAL output device (partial match): '{}' for requested '{}'", name, device_name);
                        return Ok(Some(device.clone()));
                    }
                }
            }
            
            // Log all available devices for debugging
            log::warn!("CPAL output device '{}' not found. Available devices:", device_name);
            for (i, device) in device_list.iter().enumerate() {
                if let Ok(name) = device.name() {
                    log::warn!("  {}: {}", i, name);
                }
            }
            
            log::warn!("Will use default device instead");
            Ok(None)
        }
        Err(e) => {
            log::error!("Failed to enumerate CPAL output devices: {}", e);
            Err(PlaybackError::CpalNoDefaultOutputDevice(format!("Failed to enumerate output devices: {}", e)))
        }
    }
}