use super::{AudioDevice, AudioDeviceList, AudioDeviceType};
use crate::audio::errors::PlaybackError;
use cpal::traits::{DeviceTrait, HostTrait};

/// Cross-platform fallback device detection using cpal
#[allow(dead_code)]
pub fn detect_devices() -> Result<AudioDeviceList, PlaybackError> {
    log::info!("[cpal] Starting fallback device detection...");

    let host = cpal::default_host();
    let mut output_devices = Vec::new();
    let mut input_devices = Vec::new();

    // Get default devices
    let default_output = host.default_output_device().and_then(|d| d.name().ok());
    let default_input = host.default_input_device().and_then(|d| d.name().ok());

    log::info!("[cpal] Default output device: {:?}", default_output);
    log::info!("[cpal] Default input device: {:?}", default_input);

    // Try different host backends
    let hosts = cpal::available_hosts();
    log::info!("[cpal] Available audio hosts: {:?}", hosts);

    // First try the default host
    detect_devices_from_host(
        &host,
        &mut output_devices,
        &mut input_devices,
        &default_output,
        &default_input,
        "default",
    );

    // Try other hosts if available
    for host_id in hosts {
        match cpal::host_from_id(host_id) {
            Ok(specific_host) => {
                if std::ptr::eq(&host as *const _, &specific_host as *const _) {
                    continue; // Skip if it's the same as default host
                }
                let host_name = format!("{:?}", host_id);
                log::info!("[cpal] Trying specific host: {}", host_name);
                detect_devices_from_host(
                    &specific_host,
                    &mut output_devices,
                    &mut input_devices,
                    &default_output,
                    &default_input,
                    &host_name,
                );
            }
            Err(e) => {
                log::warn!("[cpal] Failed to get host {:?}: {}", host_id, e);
            }
        }
    }

    // Remove duplicates based on device name
    output_devices.sort_by(|a, b| a.name.cmp(&b.name));
    output_devices.dedup_by(|a, b| a.name == b.name);

    input_devices.sort_by(|a, b| a.name.cmp(&b.name));
    input_devices.dedup_by(|a, b| a.name == b.name);

    let device_list = AudioDeviceList {
        output_devices,
        input_devices,
        default_output,
        default_input,
    };

    log::info!(
        "[cpal] Device detection completed. Found {} unique output devices, {} unique input devices",
        device_list.output_devices.len(),
        device_list.input_devices.len()
    );

    Ok(device_list)
}

#[allow(dead_code)]
fn detect_devices_from_host(
    host: &cpal::Host,
    output_devices: &mut Vec<AudioDevice>,
    input_devices: &mut Vec<AudioDevice>,
    default_output: &Option<String>,
    default_input: &Option<String>,
    host_name: &str,
) {
    // Detect output devices
    match host.output_devices() {
        Ok(devices) => {
            let devices: Vec<_> = devices.collect();
            log::info!(
                "[cpal:{}] Found {} output devices to enumerate",
                host_name,
                devices.len()
            );

            for (i, device) in devices.iter().enumerate() {
                log::debug!(
                    "[cpal:{}] Processing output device {} of {}",
                    host_name,
                    i + 1,
                    devices.len()
                );

                match device.name() {
                    Ok(name) => {
                        // Check if we already have this device
                        if output_devices.iter().any(|d: &AudioDevice| d.name == name) {
                            log::debug!(
                                "[cpal:{}] Skipping duplicate output device: {}",
                                host_name,
                                name
                            );
                            continue;
                        }

                        let is_default = default_output.as_ref() == Some(&name);
                        output_devices.push(AudioDevice {
                            name: name.clone(),
                            is_default,
                            device_type: AudioDeviceType::Output,
                        });
                        log::info!(
                            "[cpal:{}] Found output device: {} {}",
                            host_name,
                            name,
                            if is_default { "(default)" } else { "" }
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "[cpal:{}] Failed to get output device name for device {}: {}",
                            host_name,
                            i,
                            e
                        );
                        continue;
                    }
                }
            }
        }
        Err(e) => {
            log::warn!(
                "[cpal:{}] Failed to enumerate output devices: {}",
                host_name,
                e
            );
        }
    }

    // Detect input devices
    match host.input_devices() {
        Ok(devices) => {
            let devices: Vec<_> = devices.collect();
            log::info!(
                "[cpal:{}] Found {} input devices to enumerate",
                host_name,
                devices.len()
            );

            for (i, device) in devices.iter().enumerate() {
                log::debug!(
                    "[cpal:{}] Processing input device {} of {}",
                    host_name,
                    i + 1,
                    devices.len()
                );
                match device.name() {
                    Ok(name) => {
                        // Check if we already have this device
                        if input_devices.iter().any(|d: &AudioDevice| d.name == name) {
                            log::debug!(
                                "[cpal:{}] Skipping duplicate input device: {}",
                                host_name,
                                name
                            );
                            continue;
                        }

                        let is_default = default_input.as_ref() == Some(&name);
                        input_devices.push(AudioDevice {
                            name: name.clone(),
                            is_default,
                            device_type: AudioDeviceType::Input,
                        });
                        log::info!(
                            "[cpal:{}] Found input device: {} {}",
                            host_name,
                            name,
                            if is_default { "(default)" } else { "" }
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "[cpal:{}] Failed to get input device name for device {}: {}",
                            host_name,
                            i,
                            e
                        );
                        continue;
                    }
                }
            }
        }
        Err(e) => {
            log::warn!(
                "[cpal:{}] Failed to enumerate input devices: {}",
                host_name,
                e
            );
        }
    }
}
