use super::*;
use crate::audio::devices::store::AudioDeviceStore;
use std::collections::VecDeque;
use std::sync::atomic::AtomicU64;

#[cfg(target_os = "macos")]
use coreaudio::audio_unit::{
    AudioUnit, Scope, Element, SampleFormat, StreamFormat,
    audio_format::LinearPcmFlags,
    macos_helpers::{get_audio_device_ids_for_scope, get_device_name, audio_unit_from_device_id},
    render_callback::{self, data},
};

#[cfg(target_os = "macos")]
use coreaudio::sys::kAudioUnitProperty_StreamFormat;

const SAMPLE_RATE: f64 = 44100.0;
const BUFFER_SIZE: usize = 8192; // Larger buffer for stability
const TARGET_BUFFER_SIZE: usize = 2048; // Target buffer level to maintain

/// Manages cue output using CoreAudio on macOS
pub struct CueOutputManager {
    #[cfg(target_os = "macos")]
    audio_unit: Option<AudioUnit>,
    is_active: Arc<AtomicBool>,
    device_name: Arc<Mutex<Option<String>>>,
    // Audio buffer for passing samples from the selected deck's callback to cue output
    audio_buffer_left: Arc<Mutex<VecDeque<f32>>>,
    audio_buffer_right: Arc<Mutex<VecDeque<f32>>>,
    // Track the sample rate of the current track
    current_sample_rate: Arc<Mutex<Option<f64>>>,
    // Track which deck is currently outputting to cue (A, B, or None)
    selected_deck: Arc<Mutex<Option<String>>>,
}

impl CueOutputManager {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            audio_unit: None,
            is_active: Arc::new(AtomicBool::new(false)),
            device_name: Arc::new(Mutex::new(None)),
            audio_buffer_left: Arc::new(Mutex::new(VecDeque::new())),
            audio_buffer_right: Arc::new(Mutex::new(VecDeque::new())),
            current_sample_rate: Arc::new(Mutex::new(None)),
            selected_deck: Arc::new(Mutex::new(None)),
        }
    }

    /// Add a sample to the cue output buffer (called from track B's audio callback)
    #[inline]
    pub fn push_sample(&self, sample: f32) {
        // Fast early exit if not active
        if !self.is_active.load(Ordering::Relaxed) {
            return;
        }

        // Try to get both locks in one attempt to reduce contention
        if let (Ok(mut left_buf), Ok(mut right_buf)) = (
            self.audio_buffer_left.try_lock(),
            self.audio_buffer_right.try_lock()
        ) {
            // Efficient buffer management - only check size occasionally
            let left_len = left_buf.len();
            if left_len > BUFFER_SIZE {
                // Drain excess samples efficiently
                let excess = left_len - TARGET_BUFFER_SIZE;
                left_buf.drain(0..excess);
                right_buf.drain(0..excess);
            }
            
            // Push new samples
            left_buf.push_back(sample);
            right_buf.push_back(sample);
        }
        // If we can't get locks, just drop this sample - audio will continue smoothly
    }

    /// Set the sample rate (should be called when deck B loads a new track)
    pub fn set_sample_rate(&mut self, sample_rate: f64) -> Result<(), PlaybackError> {
        let mut sample_rate_guard = self.current_sample_rate.lock().map_err(|_| {
            PlaybackError::LogicalStateLockError("Failed to lock current_sample_rate".to_string())
        })?;
        *sample_rate_guard = Some(sample_rate);
        log::info!("[CueOutput] Sample rate updated to {} Hz", sample_rate);
        Ok(())
    }

    /// Set which deck outputs to cue (A, B, or None)
    pub fn set_selected_deck(&mut self, deck_id: Option<String>) -> Result<(), PlaybackError> {
        let mut selected_deck_guard = self.selected_deck.lock().map_err(|_| {
            PlaybackError::LogicalStateLockError("Failed to lock selected_deck".to_string())
        })?;
        *selected_deck_guard = deck_id.clone();
        log::info!("[CueOutput] Selected deck updated to {:?}", deck_id);
        Ok(())
    }

    /// Updates the selected cue output device
    pub fn set_device(&mut self, device_name: Option<String>) -> Result<(), PlaybackError> {
        {
            let mut current_device = self.device_name.lock().map_err(|_| {
                PlaybackError::LogicalStateLockError("Failed to lock device_name".to_string())
            })?;
            
            *current_device = device_name.clone();
        } // Lock is dropped here
        
        if let Some(ref name) = device_name {
            log::info!("[CueOutput] Device set to: {}", name);
            self.setup_audio_unit(Some(name.clone()))?;
            
            // Clear buffers and activate cue output immediately
            if let (Ok(mut left_buf), Ok(mut right_buf)) = (
                self.audio_buffer_left.lock(),
                self.audio_buffer_right.lock()
            ) {
                left_buf.clear();
                right_buf.clear();
                // Pre-fill with a small amount of silence to prevent initial crackling
                for _ in 0..512 {
                    left_buf.push_back(0.0);
                    right_buf.push_back(0.0);
                }
                log::info!("[CueOutput] Cleared and pre-filled buffers with 512 silent samples");
            }
            
            self.is_active.store(true, Ordering::Relaxed);
            
            if let Some(ref mut audio_unit) = self.audio_unit {
                audio_unit.start().map_err(|e| {
                    PlaybackError::OutputStreamInitError(format!("Failed to start CoreAudio unit: {}", e))
                })?;
                log::info!("[CueOutput] Started cue output immediately for device: {}", name);
            }
        } else {
            log::info!("[CueOutput] Device cleared, stopping cue output");
            self.stop_audio_unit()?;
        }
        
        Ok(())
    }

    /// Starts cue output for deck B with the given audio samples
    pub fn start_cue_output(
        &mut self,
        deck_b_state: &AudioThreadDeckState,
    ) -> Result<(), PlaybackError> {
        #[cfg(target_os = "macos")]
        {
            let device_name = {
                let device_guard = self.device_name.lock().map_err(|_| {
                    PlaybackError::LogicalStateLockError("Failed to lock device_name".to_string())
                })?;
                device_guard.clone()
            };

            if device_name.is_none() {
                log::info!("[CueOutput] No cue device selected, skipping start");
                return Ok(());
            }

            // Set the sample rate from deck B's current configuration
            let track_sample_rate = deck_b_state.sample_rate as f64;
            {
                let mut sample_rate_guard = self.current_sample_rate.lock().map_err(|_| {
                    PlaybackError::LogicalStateLockError("Failed to lock current_sample_rate".to_string())
                })?;
                *sample_rate_guard = Some(track_sample_rate);
                log::info!("[CueOutput] Set sample rate to {} Hz from deck B", track_sample_rate);
            }

            // If audio unit is already set up, need to recreate it with the new sample rate
            if self.audio_unit.is_some() {
                log::info!("[CueOutput] Recreating audio unit with new sample rate");
                self.set_device(device_name)?;
            } else {
                log::info!("[CueOutput] Audio unit not setup, calling set_device to initialize");
                self.set_device(device_name)?;
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            log::warn!("[CueOutput] Cue output only supported on macOS");
        }

        Ok(())
    }

    /// Stops cue output
    pub fn stop_cue_output(&mut self) -> Result<(), PlaybackError> {
        self.is_active.store(false, Ordering::Relaxed);
        self.stop_audio_unit()
    }

    #[cfg(target_os = "macos")]
    fn setup_audio_unit(&mut self, device_name: Option<String>) -> Result<(), PlaybackError> {
        // Stop existing audio unit if any
        self.stop_audio_unit()?;

        let device_name = match device_name {
            Some(name) => name,
            None => return Ok(()), // No device to set up
        };

        log::info!("[CueOutput] Setting up CoreAudio unit for device: {}", device_name);

        // Find the CoreAudio device ID
        let device_id = self.find_coreaudio_device_id(&device_name)?;

        // Create audio unit from device ID
        let mut output_audio_unit = audio_unit_from_device_id(device_id, false)
            .map_err(|e| PlaybackError::OutputStreamInitError(format!("Failed to create audio unit: {}", e)))?;

        // Use the current track's sample rate, fall back to 44.1kHz
        let device_sample_rate = {
            let sample_rate_guard = self.current_sample_rate.lock().map_err(|_| {
                PlaybackError::LogicalStateLockError("Failed to lock current_sample_rate".to_string())
            })?;
            sample_rate_guard.unwrap_or(SAMPLE_RATE)
        };

        log::info!("[CueOutput] Using sample rate: {} Hz", device_sample_rate);

        // Set up stream format
        let out_stream_format = StreamFormat {
            sample_rate: device_sample_rate,
            sample_format: SampleFormat::F32,
            flags: LinearPcmFlags::IS_FLOAT | LinearPcmFlags::IS_PACKED | LinearPcmFlags::IS_NON_INTERLEAVED,
            channels: 2, // Stereo output
        };

        log::info!("[CueOutput] Setting stream format: {:#?}", &out_stream_format);

        let asbd = out_stream_format.to_asbd();
        output_audio_unit.set_property(kAudioUnitProperty_StreamFormat, Scope::Input, Element::Output, Some(&asbd))
            .map_err(|e| PlaybackError::OutputStreamInitError(format!("Failed to set stream format: {}", e)))?;

        // Clone buffers for the render callback
        let consumer_left = self.audio_buffer_left.clone();
        let consumer_right = self.audio_buffer_right.clone();
        let is_active = self.is_active.clone();

        // Set up render callback
        type Args = render_callback::Args<data::NonInterleaved<f32>>;
        output_audio_unit.set_render_callback(move |args: Args| {
            let render_callback::Args { num_frames, mut data, .. } = args;

            if !is_active.load(Ordering::Relaxed) {
                // Fill with silence if not active
                for channel in data.channels_mut() {
                    for sample in channel.iter_mut().take(num_frames) {
                        *sample = 0.0;
                    }
                }
                return Ok(());
            }

            // Get buffers (non-blocking to avoid audio dropouts)
            let (mut left_buffer, mut right_buffer) = match (consumer_left.try_lock(), consumer_right.try_lock()) {
                (Ok(left), Ok(right)) => (left, right),
                _ => {
                    // If we can't get the lock, fill with silence
                    for channel in data.channels_mut() {
                        for sample in channel.iter_mut().take(num_frames) {
                            *sample = 0.0;
                        }
                    }
                    return Ok(());
                }
            };

            // Check buffer status and provide detailed logging
            let available_samples = left_buffer.len().min(right_buffer.len());
            
            // Minimal callback logging
            static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
            let callback_num = CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);

            // Wait for buffer to fill to target level before starting playback
            if available_samples < TARGET_BUFFER_SIZE {
                if callback_num % 10000 == 0 { // Log every ~5 seconds
                    log::trace!("[CueOutput] Buffering: {}/{}", available_samples, TARGET_BUFFER_SIZE);
                }
                for channel in data.channels_mut() {
                    for sample in channel.iter_mut().take(num_frames) {
                        *sample = 0.0;
                    }
                }
                return Ok(());
            }

            // Ensure we don't consume more samples than we have
            let samples_to_consume = num_frames.min(available_samples);
            
            // Optimized sample consumption - process in chunks
            let mut channels: Vec<_> = data.channels_mut().collect();
            
            // Fill the first samples_to_consume frames with buffer data
            for i in 0..samples_to_consume {
                let sample = left_buffer.pop_front().unwrap_or(0.0);
                right_buffer.pop_front(); // Keep buffers in sync
                
                // Write to both channels (mono -> stereo)
                if let Some(left_ch) = channels.get_mut(0) {
                    left_ch[i] = sample;
                }
                if let Some(right_ch) = channels.get_mut(1) {
                    right_ch[i] = sample;
                }
            }
            
            // Fill remaining frames with silence if needed
            if samples_to_consume < num_frames {
                for ch in &mut channels {
                    for i in samples_to_consume..num_frames {
                        ch[i] = 0.0;
                    }
                }
            }

            // Minimal buffer status logging
            if callback_num % 50000 == 0 { // Log every ~25 seconds
                let remaining_samples = left_buffer.len().min(right_buffer.len());
                log::trace!("[CueOutput] Buffer health: {} remaining", remaining_samples);
            }

            Ok(())
        }).map_err(|e| PlaybackError::OutputStreamInitError(format!("Failed to set render callback: {}", e)))?;

        // Initialize the audio unit
        output_audio_unit.initialize()
            .map_err(|e| PlaybackError::OutputStreamInitError(format!("Failed to initialize audio unit: {}", e)))?;

        // Start the audio unit
        output_audio_unit.start()
            .map_err(|e| PlaybackError::OutputStreamInitError(format!("Failed to start audio unit: {}", e)))?;

        self.audio_unit = Some(output_audio_unit);
        log::info!("[CueOutput] CoreAudio unit started successfully for device: {}", device_name);

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn stop_audio_unit(&mut self) -> Result<(), PlaybackError> {
        if let Some(ref mut audio_unit) = self.audio_unit {
            audio_unit.stop().map_err(|e| {
                PlaybackError::OutputStreamInitError(format!("Failed to stop CoreAudio unit: {}", e))
            })?;
            audio_unit.uninitialize().map_err(|e| {
                PlaybackError::OutputStreamInitError(format!("Failed to uninitialize CoreAudio unit: {}", e))
            })?;
            log::info!("[CueOutput] Stopped and uninitialized CoreAudio unit");
        }
        
        // Clear audio buffers
        if let Ok(mut left_buf) = self.audio_buffer_left.lock() {
            left_buf.clear();
        }
        if let Ok(mut right_buf) = self.audio_buffer_right.lock() {
            right_buf.clear();
        }
        
        self.audio_unit = None;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn find_coreaudio_device_id(&self, device_name: &str) -> Result<u32, PlaybackError> {
        // Get all output devices
        let output_ids = get_audio_device_ids_for_scope(Scope::Output)
            .map_err(|e| {
                PlaybackError::CpalNoDefaultOutputDevice(format!("Failed to get output device IDs: {}", e))
            })?;

        // Find device by name
        for device_id in output_ids {
            if let Ok(name) = get_device_name(device_id) {
                if name == device_name {
                    log::info!("[CueOutput] Found CoreAudio device ID {} for '{}'", device_id, device_name);
                    return Ok(device_id);
                }
            }
        }

        Err(PlaybackError::CpalNoDefaultOutputDevice(format!(
            "CoreAudio device '{}' not found", device_name
        )))
    }

    #[cfg(not(target_os = "macos"))]
    fn setup_audio_unit(&mut self, _device_name: Option<String>) -> Result<(), PlaybackError> {
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn stop_audio_unit(&mut self) -> Result<(), PlaybackError> {
        Ok(())
    }
}

/// Global cue output manager instance
use std::sync::LazyLock;
static CUE_OUTPUT_MANAGER: LazyLock<Arc<Mutex<Option<CueOutputManager>>>> = 
    LazyLock::new(|| Arc::new(Mutex::new(None)));

/// Initialize the cue output manager
pub fn init_cue_output_manager() -> Result<(), PlaybackError> {
    let mut manager = CUE_OUTPUT_MANAGER.lock().map_err(|_| {
        PlaybackError::LogicalStateLockError("Failed to lock cue output manager".to_string())
    })?;
    
    *manager = Some(CueOutputManager::new());
    log::info!("[CueOutput] Cue output manager initialized");
    Ok(())
}

/// Update the cue output device selection
pub fn update_cue_device(device_store: &AudioDeviceStore) -> Result<(), PlaybackError> {
    let device_name = device_store.get_cue_output_device()?;
    
    let mut manager = CUE_OUTPUT_MANAGER.lock().map_err(|_| {
        PlaybackError::LogicalStateLockError("Failed to lock cue output manager".to_string())
    })?;
    
    if let Some(ref mut manager) = manager.as_mut() {
        manager.set_device(device_name)?;
    }
    
    Ok(())
}

/// Start cue output for deck B (called when deck B is playing and cue is enabled)
pub fn start_deck_b_cue_output(
    local_states: &HashMap<String, AudioThreadDeckState>
) -> Result<(), PlaybackError> {
    log::info!("[CueOutput] start_deck_b_cue_output called");

    let deck_b_state = local_states.get("B").ok_or_else(|| {
        log::warn!("[CueOutput] Deck B not found in local_states");
        PlaybackError::DeckNotFound {
            deck_id: "B".to_string(),
        }
    })?;

    let is_playing = deck_b_state.is_playing.load(Ordering::Relaxed);
    log::info!("[CueOutput] Deck B playing status: {}", is_playing);

    if !is_playing {
        log::info!("[CueOutput] Deck B is not playing, skipping cue output start");
        return Ok(());
    }

    let mut manager = CUE_OUTPUT_MANAGER.lock().map_err(|_| {
        PlaybackError::LogicalStateLockError("Failed to lock cue output manager".to_string())
    })?;
    
    match manager.as_mut() {
        Some(ref mut manager) => {
            log::info!("[CueOutput] Starting cue output for deck B");
            manager.start_cue_output(deck_b_state)?;
        },
        None => {
            log::warn!("[CueOutput] Cue output manager is None, cannot start");
        }
    }
    
    Ok(())
}

/// Stop cue output
pub fn stop_cue_output() -> Result<(), PlaybackError> {
    let mut manager = CUE_OUTPUT_MANAGER.lock().map_err(|_| {
        PlaybackError::LogicalStateLockError("Failed to lock cue output manager".to_string())
    })?;
    
    if let Some(ref mut manager) = manager.as_mut() {
        manager.stop_cue_output()?;
    }
    
    Ok(())
}

/// Set the sample rate for cue output (called when deck B loads a track)
pub fn set_cue_sample_rate(sample_rate: f64) -> Result<(), PlaybackError> {
    let mut manager = CUE_OUTPUT_MANAGER.lock().map_err(|_| {
        PlaybackError::LogicalStateLockError("Failed to lock cue output manager".to_string())
    })?;
    
    if let Some(ref mut manager) = manager.as_mut() {
        manager.set_sample_rate(sample_rate)?;
    }
    
    Ok(())
}

/// Push a sample to the cue output buffer (called from track B's audio callback)
/// Optimized for minimal overhead in audio callback
#[inline]
pub fn push_cue_sample(sample: f32) {
    // Fast path: try to get manager without blocking
    if let Ok(manager) = CUE_OUTPUT_MANAGER.try_lock() {
        if let Some(ref manager) = manager.as_ref() {
            manager.push_sample(sample);
        }
    }
    // If we can't get the lock, drop the sample to avoid blocking the audio thread
}

/// Set which deck should output to cue (A, B, or None)
pub fn set_cue_deck(deck_id: Option<String>) -> Result<(), PlaybackError> {
    let mut manager = CUE_OUTPUT_MANAGER.lock().map_err(|_| {
        PlaybackError::LogicalStateLockError("Failed to lock cue output manager".to_string())
    })?;
    
    if let Some(ref mut manager) = manager.as_mut() {
        manager.set_selected_deck(deck_id)?;
    }
    
    Ok(())
}

/// Check if a specific deck should output to cue (called from audio callbacks)
/// Optimized for minimal overhead in audio callback
#[inline]
pub fn should_deck_output_to_cue(deck_id: &str) -> bool {
    // Fast path: try to get manager without blocking
    if let Ok(manager) = CUE_OUTPUT_MANAGER.try_lock() {
        if let Some(ref manager) = manager.as_ref() {
            if let Ok(selected_deck) = manager.selected_deck.try_lock() {
                return selected_deck.as_ref() == Some(&deck_id.to_string());
            }
        }
    }
    // If we can't get the lock, default to false to avoid blocking the audio thread
    false
}

