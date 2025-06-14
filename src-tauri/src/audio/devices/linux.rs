use crate::audio::errors::PlaybackError;
use super::AudioDeviceList;

/// Linux-specific audio device detection  
/// TODO: Implement Linux-specific device detection using ALSA/PulseAudio APIs
pub fn detect_devices() -> Result<AudioDeviceList, PlaybackError> {
    log::info!("[Linux] Linux-specific device detection not yet implemented, using cpal fallback");
    super::cpal_fallback::detect_devices()
}