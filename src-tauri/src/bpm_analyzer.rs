use rayon::prelude::*;
use rustfft::{num_complex::Complex, num_traits::Zero, FftPlanner};
use std::fs::File; // Removed unused Path import
use symphonia::core::{
    audio::{SampleBuffer},
    codecs::{DecoderOptions, CODEC_TYPE_NULL},
    errors::Error as SymphoniaError, // Alias to avoid name clash
    formats::FormatOptions, // Removed unused FormatReader import
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

// --- Private Helper Functions ---

// Renamed error type for clarity
type BpmResult<T> = Result<T, String>;

fn decode_mp3(path: &str) -> BpmResult<(Vec<f32>, f32)> {
    let file =
        File::open(path).map_err(|e| format!("BPM: Failed to open file '{}': {}", path, e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let hint = Hint::new();

    let probe = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("BPM: Failed to probe format for '{}': {}", path, e))?;
    let mut format = probe.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or_else(|| format!("BPM: No suitable audio track found in '{}'", path))?;

    let codec_params = track.codec_params.clone();
    let track_id = track.id; // Store track_id before the loop
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| format!("BPM: Sample rate not found for track in '{}'", path))?
        as f32;
    let channels = track
        .codec_params
        .channels
        .ok_or_else(|| format!("BPM: Channel info not found for track in '{}'", path))?
        .count();
    // let _spec = SignalSpec::new(sample_rate as u32, track.codec_params.channels.unwrap()); // Removed unused variable

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("BPM: Failed to create decoder for '{}': {}", path, e))?;

    let mut samples = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None; // Initialize lazily

    loop {
        match format.next_packet() {
            Ok(packet) => {
                // Check if the packet belongs to the selected track
                if packet.track_id() != track_id {
                    continue;
                }

                match decoder.decode(&packet) {
                    Ok(audio_buf) => {
                        // Initialize the SampleBuffer on the first successful decode
                        if sample_buf.is_none() {
                            let spec = *audio_buf.spec();
                            let duration = audio_buf.capacity() as u64;
                            sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
                        }

                        // Copy the decoded audio samples into the SampleBuffer
                        if let Some(buf) = sample_buf.as_mut() {
                            buf.copy_interleaved_ref(audio_buf);
                            let raw_samples = buf.samples();

                            if channels > 1 {
                                // Efficiently calculate mono samples
                                samples.extend(
                                    raw_samples
                                        .chunks_exact(channels)
                                        .map(|chunk| chunk.iter().sum::<f32>() / channels as f32),
                                );
                            } else {
                                samples.extend_from_slice(raw_samples);
                            }
                        }
                    }
                    Err(SymphoniaError::DecodeError(err)) => {
                        log::warn!("BPM: Ignoring decode error in '{}': {}", path, err);
                        // Continue decoding if possible
                    }
                    Err(e) => {
                        // Return fatal decode errors
                        return Err(format!(
                            "BPM: Fatal decode error in '{}': {}",
                            path, e
                        ));
                    }
                }
            }
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                log::debug!("BPM: Reached EOF for '{}'", path);
                break; // End of stream
            }
            Err(SymphoniaError::ResetRequired) => {
                // Decoder reset is typically informational for seeking, might indicate an issue here.
                log::warn!("BPM: Decoder reset required unexpectedly for '{}'", path);
                // Consider breaking or returning an error depending on desired robustness
                break;
            }
            Err(e) => {
                // Other packet reading errors
                return Err(format!(
                    "BPM: Error reading audio packet for '{}': {}",
                    path, e
                ));
            }
        }
    }
    decoder.finalize(); // Finalize the decoder state if necessary

    Ok((samples, sample_rate))
}

fn normalize_in_place(samples: &mut [f32]) {
    // Use parallel iterator for potentially faster max finding on large samples
    let max_amplitude = samples
        .par_iter()
        .map(|&x| x.abs())
        .reduce(|| 0.0f32, f32::max);

    if max_amplitude > 1e-6 {
        // Avoid division by zero or near-zero
        // Use parallel iterator for normalization
        samples.par_iter_mut().for_each(|x| *x /= max_amplitude);
    }
}


fn downsample_in_place(samples: &mut Vec<f32>, factor: usize) {
    if factor <= 1 || samples.is_empty() {
        return; // No downsampling needed or possible
    }
    let new_len = samples.len() / factor;
    if new_len == 0 {
        samples.clear(); // Handle case where factor > len
        return;
    }
    // This is inherently sequential but fast
    for i in 0..new_len {
        samples[i] = samples[i * factor];
    }
    samples.truncate(new_len);
}


fn compute_spectral_flux(samples: &[f32], frame_size: usize, hop_size: usize) -> Vec<f32> {
    if samples.len() < frame_size {
        log::warn!("BPM: Not enough samples ({}) for frame size ({}) to compute spectral flux.", samples.len(), frame_size);
        return Vec::new(); // Return empty if not enough samples
    }

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(frame_size);
    let num_frames = (samples.len() - frame_size) / hop_size + 1;

    // Compute spectra in parallel
    let spectra: Vec<Vec<f32>> = (0..num_frames)
        .into_par_iter()
        .map(|i| {
            let start = i * hop_size;
            // Ensure we don't go out of bounds, although num_frames calculation should prevent this
            let end = (start + frame_size).min(samples.len());
            let frame = &samples[start..end];

            // Pad with zeros if the last frame is smaller than frame_size
            let mut buffer: Vec<Complex<f32>> = vec![Complex::zero(); frame_size];
            for (b, &s) in buffer.iter_mut().zip(frame.iter()) {
                *b = Complex { re: s, im: 0.0 };
            }

            fft.process(&mut buffer);

            // Take magnitude of the first half (positive frequencies)
            buffer[..frame_size / 2 + 1]
                .iter()
                .map(|c| c.norm_sqr().sqrt()) // Using norm_sqr().sqrt() is equivalent to norm()
                .collect()
        })
        .collect();

    if spectra.is_empty() {
        return Vec::new();
    }

    // Compute flux sequentially (dependency between frames)
    let mut flux = vec![0.0; num_frames]; // First frame flux is 0
    if num_frames > 1 {
        // Use parallel calculation for the flux summation within each frame difference
        flux[1..].par_iter_mut().enumerate().for_each(|(idx, f)| {
            let i = idx + 1; // Adjust index for spectra access
            *f = spectra[i]
                .iter()
                .zip(spectra[i - 1].iter())
                 // Summation of positive differences
                .map(|(&curr, &prev)| (curr - prev).max(0.0))
                .sum();
        });
    }


    // Normalize the flux: divide by the mean flux
    let flux_mean = flux.iter().sum::<f32>() / num_frames as f32;
    if flux_mean > 1e-6 {
        flux.par_iter_mut().for_each(|f| *f /= flux_mean);
    }


    flux
}   

fn fft_autocorrelation(signal: &[f32], max_lag: usize) -> BpmResult<Vec<f32>> {
     if signal.is_empty() || max_lag == 0 {
        return Ok(Vec::new());
    }

    // Ensure n is large enough for the signal and the correlation result
    let n = (signal.len() + max_lag).next_power_of_two();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    let ifft = planner.plan_fft_inverse(n);

    // Prepare buffer for FFT: signal padded with zeros
    let mut buffer: Vec<Complex<f32>> = signal
        .iter()
        .map(|&x| Complex { re: x, im: 0.0 })
        .chain(std::iter::repeat(Complex::zero()).take(n - signal.len()))
        .collect();

    // Perform forward FFT
    fft.process(&mut buffer);

    // Compute power spectrum (element-wise magnitude squared)
    // Using parallel iterator for potentially large buffers
    buffer.par_iter_mut().for_each(|c| *c = c.norm_sqr().into());
    // buffer = buffer.par_iter().map(|c| (c * c.conj()).into()).collect(); // Alternative formulation


    // Perform inverse FFT to get autocorrelation
    ifft.process(&mut buffer);

    // Extract the real part and normalize, up to max_lag
     let autocorrelation: Vec<f32> = buffer[..max_lag.min(buffer.len())]
        .par_iter()
        .map(|c| c.re / n as f32)
        .collect();

    Ok(autocorrelation)
}

fn estimate_bpm(flux: &[f32], sample_rate: f32, hop_size: usize) -> BpmResult<f32> {
    if flux.is_empty() {
         return Err("Cannot estimate BPM from empty spectral flux.".to_string());
    }
    // Typical BPM range for music
    const MIN_BPM: f32 = 60.0;
    const MAX_BPM: f32 = 200.0;

     // Calculate lag range in terms of flux frames based on BPM range
    let max_lag_frames = (60.0 * sample_rate / (MIN_BPM * hop_size as f32)).ceil() as usize;
    let min_lag_frames = (60.0 * sample_rate / (MAX_BPM * hop_size as f32)).floor() as usize;

    if min_lag_frames == 0 || max_lag_frames <= min_lag_frames {
        return Err(format!(
            "Invalid lag range calculated (min: {}, max: {}). Check sample rate ({}) and hop size ({}).",
            min_lag_frames, max_lag_frames, sample_rate, hop_size
        ));
    }

    // Ensure max_lag doesn't exceed flux length for autocorrelation
    let effective_max_lag = max_lag_frames.min(flux.len());
    if effective_max_lag <= min_lag_frames {
         return Err(format!(
            "Effective max lag ({}) is not greater than min lag ({}) after flux length check.",
            effective_max_lag, min_lag_frames
        ));
    }


    let ac = fft_autocorrelation(flux, effective_max_lag)?;
    if ac.len() <= min_lag_frames {
         return Err(format!(
            "Autocorrelation result length ({}) is not greater than min lag ({}).",
            ac.len(), min_lag_frames
        ));
    }

    // Find the peak in the autocorrelation within the valid lag range
    let peak_result = ac
        .par_iter() // Parallel search for max
        .enumerate()
        .skip(min_lag_frames) // Skip lags corresponding to > MAX_BPM
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));


     match peak_result {
        Some((peak_lag_index, _)) if peak_lag_index > 0 => {
             // Convert lag index (number of frames) back to period in seconds
            let period_secs = peak_lag_index as f32 * hop_size as f32 / sample_rate;
            if period_secs > 1e-6 {
                let bpm = 60.0 / period_secs;
                 // Clamp BPM to the expected range just in case
                Ok(bpm.clamp(MIN_BPM, MAX_BPM))
            } else {
                 Err("Calculated period is too small, cannot determine BPM.".to_string())
            }

        }
        _ => {
            // No peak found or peak is at lag 0
            Err("Could not find a significant peak in autocorrelation for BPM estimation.".to_string())
        }
    }
}


// --- Public API ---

#[derive(serde::Serialize, Debug, Clone)]
pub struct BpmAnalysisResult {
    bpm: f32,
    // Optional: Could also return onset times here if needed by frontend
    // onset_times: Vec<f32>,
}

/// Analyzes the BPM of the audio file at the given path.
#[tauri::command(async)] // Mark command as async for rayon/long tasks
pub fn analyze_bpm_for_file(path: String) -> BpmResult<BpmAnalysisResult> {
    log::info!("Starting BPM analysis for: {}", path);

    // Sensible defaults - these could be parameters if needed
    let frame_size = 1024; // Larger frame for better frequency resolution
    let hop_size = frame_size / 4; // Standard overlap
    let downsample_factor = 4; // Reduce computation significantly

    // 1. Decode (Handles file opening and track finding)
    let (mut samples, sample_rate) = decode_mp3(&path)?;
    log::debug!(
        "BPM: Decoded {} samples at {} Hz for '{}'",
        samples.len(),
        sample_rate,
        path
    );

    if samples.is_empty() {
        return Err(format!("BPM: No samples decoded from '{}'", path));
    }

    // 2. Preprocess
    normalize_in_place(&mut samples);
    downsample_in_place(&mut samples, downsample_factor);
    let effective_sample_rate = sample_rate / downsample_factor as f32;
    log::debug!(
        "BPM: Downsampled to {} samples at effective {} Hz for '{}'",
        samples.len(),
        effective_sample_rate,
        path
    );


    // 3. Compute Spectral Flux
     let flux = compute_spectral_flux(&samples, frame_size, hop_size);
     log::debug!("BPM: Computed spectral flux ({} values) for '{}'", flux.len(), path);

      if flux.is_empty() {
        return Err(format!("BPM: Spectral flux calculation resulted in empty vector for '{}'. Insufficient samples?", path));
    }

    // 4. (Optional but useful) Find Onsets - can be used for debugging or other features
    // let onset_times = find_onset_peaks(&flux, hop_size, effective_sample_rate)?;
    // log::debug!("BPM: Found {} onset times for '{}'", onset_times.len(), path);

    // 5. Estimate BPM from Flux
    let bpm = estimate_bpm(&flux, effective_sample_rate, hop_size)?;
    log::info!(
        "BPM: Estimated BPM {:.2} for '{}'",
        bpm,
        path
    );


    Ok(BpmAnalysisResult { bpm })
} 