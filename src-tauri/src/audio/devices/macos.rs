use crate::audio::errors::PlaybackError;
use super::{AudioDevice, AudioDeviceList, AudioDeviceType};

#[cfg(target_os = "macos")]
use coreaudio::audio_unit::{
    macos_helpers::{get_audio_device_ids_for_scope, get_device_name, get_default_device_id},
    Scope,
};

#[cfg(target_os = "macos")]
fn device_has_output_streams(device_id: u32) -> bool {
    use coreaudio::sys::{
        AudioObjectPropertyAddress,
        AudioObjectGetPropertyDataSize, AudioObjectGetPropertyData,
        kAudioObjectPropertyElementMain, kAudioDevicePropertyStreamConfiguration,
        kAudioObjectPropertyScopeOutput, kAudioObjectPropertyScopeInput,
        AudioBufferList, OSStatus
    };
    use std::mem;
    use std::ptr;

    let device_name = get_device_name(device_id).unwrap_or_else(|_| format!("Device {}", device_id));
    
    // Check if device has output streams using CoreAudio property API
    let output_property = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreamConfiguration,
        mScope: kAudioObjectPropertyScopeOutput,
        mElement: kAudioObjectPropertyElementMain,
    };

    let input_property = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreamConfiguration,
        mScope: kAudioObjectPropertyScopeInput,
        mElement: kAudioObjectPropertyElementMain,
    };

    // Get output stream configuration
    let mut output_size: u32 = 0;
    let output_size_result: OSStatus = unsafe {
        AudioObjectGetPropertyDataSize(
            device_id,
            &output_property,
            0,
            ptr::null(),
            &mut output_size,
        )
    };

    // Get input stream configuration
    let mut input_size: u32 = 0;
    let input_size_result: OSStatus = unsafe {
        AudioObjectGetPropertyDataSize(
            device_id,
            &input_property,
            0,
            ptr::null(),
            &mut input_size,
        )
    };

    let has_output = if output_size_result == 0 && output_size > 0 {
        // Try to get the actual stream configuration
        let buffer_list_size = output_size as usize;
        let mut buffer: Vec<u8> = vec![0; buffer_list_size];
        let mut actual_size = output_size;
        
        let result: OSStatus = unsafe {
            AudioObjectGetPropertyData(
                device_id,
                &output_property,
                0,
                ptr::null(),
                &mut actual_size,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
            )
        };

        if result == 0 && actual_size >= mem::size_of::<AudioBufferList>() as u32 {
            let buffer_list = unsafe { &*(buffer.as_ptr() as *const AudioBufferList) };
            let output_channels = buffer_list.mNumberBuffers;
            log::info!("[macOS] Device '{}': {} output buffers", device_name, output_channels);
            output_channels > 0
        } else {
            log::warn!("[macOS] Device '{}': Failed to get output stream config (result: {})", device_name, result);
            false
        }
    } else {
        log::info!("[macOS] Device '{}': No output streams (size result: {}, size: {})", device_name, output_size_result, output_size);
        false
    };

    let has_input = if input_size_result == 0 && input_size > 0 {
        let buffer_list_size = input_size as usize;
        let mut buffer: Vec<u8> = vec![0; buffer_list_size];
        let mut actual_size = input_size;
        
        let result: OSStatus = unsafe {
            AudioObjectGetPropertyData(
                device_id,
                &input_property,
                0,
                ptr::null(),
                &mut actual_size,
                buffer.as_mut_ptr() as *mut std::ffi::c_void,
            )
        };

        if result == 0 && actual_size >= mem::size_of::<AudioBufferList>() as u32 {
            let buffer_list = unsafe { &*(buffer.as_ptr() as *const AudioBufferList) };
            let input_channels = buffer_list.mNumberBuffers;
            log::info!("[macOS] Device '{}': {} input buffers", device_name, input_channels);
            input_channels > 0
        } else {
            log::warn!("[macOS] Device '{}': Failed to get input stream config (result: {})", device_name, result);
            false
        }
    } else {
        log::info!("[macOS] Device '{}': No input streams (size result: {}, size: {})", device_name, input_size_result, input_size);
        false
    };

    log::info!("[macOS] Device '{}': has_output={}, has_input={}", device_name, has_output, has_input);
    
    // For DJ purposes, we want devices that have output capabilities
    // Skip devices that are input-only
    has_output
}

/// macOS-specific audio device detection using CoreAudio
pub fn detect_devices() -> Result<AudioDeviceList, PlaybackError> {
    log::info!("[macOS] Starting CoreAudio device detection...");

    #[cfg(target_os = "macos")]
    {
        detect_via_coreaudio()
    }

    #[cfg(not(target_os = "macos"))]
    {
        // Fallback for non-macOS platforms (shouldn't happen)
        super::cpal_fallback::detect_devices()
    }
}

#[cfg(target_os = "macos")]
fn detect_via_coreaudio() -> Result<AudioDeviceList, PlaybackError> {
    let mut output_devices = Vec::new();
    let mut input_devices = Vec::new();
    
    // Get default devices
    let default_output_id = get_default_device_id(false); // false = output
    let default_input_id = get_default_device_id(true);   // true = input
    
    let default_output_name = default_output_id
        .and_then(|id| get_device_name(id).ok());
    let default_input_name = default_input_id
        .and_then(|id| get_device_name(id).ok());
        
    log::info!("[macOS] Default output device: {:?}", default_output_name);
    log::info!("[macOS] Default input device: {:?}", default_input_name);

    // Get all devices from both scopes and log details
    log::info!("[macOS] === DEBUGGING DEVICE DETECTION ===");
    
    // Get devices marked as "output" by CoreAudio
    match get_audio_device_ids_for_scope(Scope::Output) {
        Ok(output_ids) => {
            log::info!("[macOS] CoreAudio reports {} devices with output scope", output_ids.len());
            
            for device_id in output_ids {
                match get_device_name(device_id) {
                    Ok(name) => {
                        log::info!("[macOS] Output scope device ID {}: '{}'", device_id, name);
                        
                        // Skip aggregate and multi-output devices as they are system-managed
                        if name.contains("Aggregate") || name.contains("Multi-Output") {
                            log::debug!("[macOS] Skipping system device: {}", name);
                            continue;
                        }
                        
                        // Check if this device actually has output capabilities
                        if !device_has_output_streams(device_id) {
                            log::info!("[macOS] Skipping input-only device: {}", name);
                            continue;
                        }
                        
                        let is_default = default_output_id.map_or(false, |id| id == device_id);
                        
                        output_devices.push(AudioDevice {
                            name: name.clone(),
                            is_default,
                            device_type: AudioDeviceType::Output,
                        });
                        
                        log::info!("[macOS] Added output device: {} {}", 
                            name, if is_default { "(default)" } else { "" });
                    }
                    Err(e) => {
                        log::warn!("[macOS] Failed to get name for output device {}: {}", device_id, e);
                    }
                }
            }
        }
        Err(e) => {
            log::error!("[macOS] Failed to get output device IDs: {}", e);
            return Err(PlaybackError::CpalNoDefaultOutputDevice(
                format!("Failed to enumerate output devices: {}", e)
            ));
        }
    }

    // Get all input devices for comparison
    match get_audio_device_ids_for_scope(Scope::Input) {
        Ok(input_ids) => {
            log::info!("[macOS] CoreAudio reports {} devices with input scope", input_ids.len());
            
            for device_id in input_ids {
                match get_device_name(device_id) {
                    Ok(name) => {
                        log::info!("[macOS] Input scope device ID {}: '{}'", device_id, name);
                        
                        // Skip aggregate devices
                        if name.contains("Aggregate") {
                            log::debug!("[macOS] Skipping system device: {}", name);
                            continue;
                        }
                        
                        let is_default = default_input_id.map_or(false, |id| id == device_id);
                        
                        input_devices.push(AudioDevice {
                            name: name.clone(),
                            is_default,
                            device_type: AudioDeviceType::Input,
                        });
                        
                        log::info!("[macOS] Added input device: {} {}", 
                            name, if is_default { "(default)" } else { "" });
                    }
                    Err(e) => {
                        log::warn!("[macOS] Failed to get name for input device {}: {}", device_id, e);
                    }
                }
            }
        }
        Err(e) => {
            log::error!("[macOS] Failed to get input device IDs: {}", e);
            return Err(PlaybackError::CpalNoDefaultOutputDevice(
                format!("Failed to enumerate input devices: {}", e)
            ));
        }
    }

    log::info!("[macOS] CoreAudio detection completed: {} output, {} input devices", 
        output_devices.len(), input_devices.len());

    Ok(AudioDeviceList {
        output_devices,
        input_devices,
        default_output: default_output_name,
        default_input: default_input_name,
    })
}