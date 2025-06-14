use crate::audio::errors::PlaybackError;
use super::AudioDeviceList;

/// Windows-specific audio device detection
/// TODO: Implement Windows-specific device detection using WinAPI
pub fn detect_devices() -> Result<AudioDeviceList, PlaybackError> {
    log::info!("[Windows] Windows-specific device detection not yet implemented, using cpal fallback");
    super::cpal_fallback::detect_devices()
}