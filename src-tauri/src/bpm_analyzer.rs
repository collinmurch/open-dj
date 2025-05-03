use rayon::prelude::*;
use rustfft::{FftPlanner, num_complex::Complex, num_traits::Zero};

// --- Type Alias ---
// Renamed error type for clarity
type BpmResult<T> = Result<T, String>;

// --- Private Helper Functions ---

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
        log::warn!(
            "BPM: Not enough samples ({}) for frame size ({}) to compute spectral flux.",
            samples.len(),
            frame_size
        );
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
            ac.len(),
            min_lag_frames
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
            Err(
                "Could not find a significant peak in autocorrelation for BPM estimation."
                    .to_string(),
            )
        }
    }
}

// --- Public Calculation Function ---

/// Calculates the BPM from pre-decoded mono f32 audio samples.
pub(crate) fn calculate_bpm(samples: &[f32], sample_rate: f32) -> BpmResult<f32> {
    log::debug!(
        "BPM Calc: Starting BPM calculation with {} samples at {} Hz",
        samples.len(),
        sample_rate
    );

    if samples.is_empty() {
        return Err("BPM Calc: Cannot calculate BPM from empty samples.".to_string());
    }

    // Sensible defaults - these could be parameters if needed
    let frame_size = 1024; // Larger frame for better frequency resolution
    let hop_size = frame_size / 4; // Standard overlap
    let downsample_factor = 4; // Reduce computation significantly

    // --- Preprocessing ---
    let mut processed_samples = samples.to_vec(); // Clone samples for modification
    normalize_in_place(&mut processed_samples);
    downsample_in_place(&mut processed_samples, downsample_factor);
    let effective_sample_rate = sample_rate / downsample_factor as f32;

    log::debug!(
        "BPM Calc: Downsampled to {} samples at effective {} Hz",
        processed_samples.len(),
        effective_sample_rate
    );

    if processed_samples.is_empty() {
        return Err(format!(
            "BPM Calc: Samples became empty after downsampling (factor {}). Original count: {}",
            downsample_factor,
            samples.len()
        ));
    }

    // --- Compute Spectral Flux ---
    let flux = compute_spectral_flux(&processed_samples, frame_size, hop_size);
    log::debug!("BPM Calc: Computed spectral flux ({} values)", flux.len());

    if flux.is_empty() {
        return Err("BPM Calc: Spectral flux calculation resulted in empty vector. Insufficient samples after processing?".to_string());
    }

    // --- Estimate BPM from Flux ---
    let bpm = estimate_bpm(&flux, effective_sample_rate, hop_size)?;
    log::debug!("BPM Calc: Estimated BPM: {:.2}", bpm);

    Ok(bpm)
}
