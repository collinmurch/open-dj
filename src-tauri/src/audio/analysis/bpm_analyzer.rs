use crate::audio::config;
use crate::audio::errors::BpmError;
use rayon::prelude::*;
use rustfft::{FftPlanner, num_complex::Complex, num_traits::Zero};
use std::sync::Arc;

// --- Private Helper Functions ---

fn normalize_in_place(samples: &mut [f32]) {
    let max_amplitude = samples
        .par_iter()
        .map(|&x| x.abs())
        .reduce(|| 0.0f32, f32::max);

    // Avoid division by zero or near-zero
    if max_amplitude > 1e-6 {
        let inv_max = 1.0 / max_amplitude;
        samples.par_iter_mut().for_each(|x| *x *= inv_max);
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
        return Vec::new();
    }

    let mut planner = FftPlanner::new();
    let fft = Arc::new(planner.plan_fft_forward(frame_size));
    let num_frames = (samples.len() - frame_size) / hop_size + 1;

    // Precompute Hann window once
    let hann_window: Arc<Vec<f32>> = Arc::new((0..frame_size)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (frame_size - 1) as f32).cos()))
        .collect());

    // Pre-allocate spectrum storage to avoid repeated allocations
    let spectrum_bins = frame_size / 2 + 1;
    
    // Compute spectra in parallel with optimized memory allocation
    let spectra: Vec<Vec<f32>> = (0..num_frames)
        .into_par_iter()
        .map(|i| {
            let start = i * hop_size;
            let end = (start + frame_size).min(samples.len());
            let frame = &samples[start..end];

            // Pre-allocate buffer once per thread
            let mut buffer: Vec<Complex<f32>> = Vec::with_capacity(frame_size);
            buffer.resize(frame_size, Complex::zero());
            
            // Apply window and copy to buffer in single pass
            for (j, (&s, &w)) in frame.iter().zip(hann_window.iter()).enumerate() {
                buffer[j] = Complex { re: s * w, im: 0.0 };
            }
            // Zero-pad remaining if needed (frame < frame_size)
            for j in frame.len()..frame_size {
                buffer[j] = Complex::zero();
            }

            fft.process(&mut buffer);

            // Extract magnitude spectrum with pre-allocated capacity
            let mut spectrum = Vec::with_capacity(spectrum_bins);
            for c in &buffer[..spectrum_bins] {
                spectrum.push(c.norm());
            }
            spectrum
        })
        .collect();

    if spectra.is_empty() {
        return Vec::new();
    }

    // Compute flux differences in parallel where possible
    let mut flux = vec![0.0; num_frames];
    if num_frames > 1 {
        // Parallel flux computation with optimized difference calculation
        flux[1..].par_iter_mut().enumerate().for_each(|(idx, f)| {
            let i = idx + 1;
            *f = spectra[i]
                .par_iter()
                .zip(spectra[i - 1].par_iter())
                .map(|(&curr, &prev)| (curr - prev).max(0.0))
                .sum();
        });
    }

    // Fast normalization with parallel sum
    let flux_sum = flux.par_iter().sum::<f32>();
    if flux_sum > 1e-6 {
        let flux_mean = flux_sum / num_frames as f32;
        flux.par_iter_mut().for_each(|f| *f /= flux_mean);
    }

    flux
}

fn fft_autocorrelation(signal: &[f32], max_lag: usize) -> Result<Vec<f32>, BpmError> {
    if signal.is_empty() || max_lag == 0 {
        return Ok(Vec::new());
    }

    // Optimize FFT size for better performance
    let n = (signal.len() + max_lag).next_power_of_two();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    let ifft = planner.plan_fft_inverse(n);

    // Pre-allocate buffer with exact capacity to avoid reallocations
    let mut buffer: Vec<Complex<f32>> = Vec::with_capacity(n);
    
    // Fast buffer initialization
    buffer.extend(signal.iter().map(|&x| Complex { re: x, im: 0.0 }));
    buffer.resize(n, Complex::zero());

    // Perform forward FFT
    fft.process(&mut buffer);

    // Compute power spectrum in-place for memory efficiency
    buffer.par_iter_mut().for_each(|c| {
        let mag_sqr = c.norm_sqr();
        *c = Complex { re: mag_sqr, im: 0.0 };
    });

    // Perform inverse FFT to get autocorrelation
    ifft.process(&mut buffer);

    // Extract and normalize in single pass with pre-allocated capacity
    let result_len = max_lag.min(buffer.len());
    let mut autocorrelation = Vec::with_capacity(result_len);
    let normalization_factor = 1.0 / n as f32;
    
    for i in 0..result_len {
        autocorrelation.push(buffer[i].re * normalization_factor);
    }

    Ok(autocorrelation)
}

fn estimate_bpm(flux: &[f32], sample_rate: f32, hop_size: usize) -> Result<f32, BpmError> {
    if flux.is_empty() {
        return Err(BpmError::EmptySpectralFlux);
    }

    // Calculate lag range in terms of flux frames based on BPM range
    let max_lag_frames = (60.0 * sample_rate / (config::BPM_MIN * hop_size as f32)).ceil() as usize;
    let min_lag_frames =
        (60.0 * sample_rate / (config::BPM_MAX * hop_size as f32)).floor() as usize;

    if min_lag_frames == 0 || max_lag_frames <= min_lag_frames {
        return Err(BpmError::InvalidLagRange {
            min_lag: min_lag_frames,
            max_lag: max_lag_frames,
            sample_rate,
            hop_size,
        });
    }

    // Ensure max_lag doesn't exceed flux length for autocorrelation
    let effective_max_lag = max_lag_frames.min(flux.len());
    if effective_max_lag <= min_lag_frames {
        return Err(BpmError::EffectiveLagTooSmall {
            eff_max_lag: effective_max_lag,
            min_lag: min_lag_frames,
        });
    }

    let ac = fft_autocorrelation(flux, effective_max_lag)?;
    if ac.len() <= min_lag_frames {
        return Err(BpmError::AutocorrelationTooShort {
            ac_len: ac.len(),
            min_lag: min_lag_frames,
        });
    }

    // --- ADDED: Smooth the autocorrelation signal ---
    let smoothed_ac = if ac.len() >= 3 {
        let mut smoothed = vec![0.0; ac.len()];
        // Handle edges (simple replication)
        smoothed[0] = ac[0]; // Keep first element as is
        smoothed[ac.len() - 1] = ac[ac.len() - 1]; // Keep last element as is

        // Apply 3-point moving average to the interior
        // Using parallel iterators for potentially large ac vectors
        smoothed[1..ac.len()-1].par_iter_mut().enumerate().for_each(|(i, s)| {
            // i is the index within the slice smoothed[1..ac.len()-1]
            // So the corresponding index in the original `ac` is i + 1
            *s = (ac[i] + ac[i+1] + ac[i+2]) / 3.0;
        });
        smoothed // Use the smoothed version
    } else {
        ac // Not enough points to smooth, use original
    };
    // --- END ADDED ---

    // Find the peak in the *smoothed* autocorrelation within the valid lag range
    let peak_result = smoothed_ac
        .par_iter()
        .enumerate()
        .skip(min_lag_frames) // Skip lags corresponding to > MAX_BPM
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    match peak_result {
        Some((mut peak_lag_index, mut y_0_ref)) if peak_lag_index > 0 => {
            // --- ADDED: Octave error correction (prefer faster tempo if strong evidence) ---
            let prospective_double_bpm_lag_index = (peak_lag_index as f32 / 2.0).round() as usize;

            // Check if this half-period lag is valid and corresponds to a BPM <= BPM_MAX
            if prospective_double_bpm_lag_index >= min_lag_frames &&
               prospective_double_bpm_lag_index < peak_lag_index && // Ensure it's a shorter lag
               prospective_double_bpm_lag_index < smoothed_ac.len() // Boundary check for safety
            {
                let y_at_double_bpm_lag = smoothed_ac[prospective_double_bpm_lag_index];

                const OCTAVE_CORRECTION_THRESHOLD_RATIO: f32 = 0.7; // Tunable
                if y_at_double_bpm_lag > OCTAVE_CORRECTION_THRESHOLD_RATIO * (*y_0_ref) {
                    log::info!(
                        "BPM Octave Correction: Switching from lag {} (value {:.3}) to lag {} (value {:.3})",
                        peak_lag_index, *y_0_ref, prospective_double_bpm_lag_index, y_at_double_bpm_lag
                    );
                    peak_lag_index = prospective_double_bpm_lag_index;
                    y_0_ref = &smoothed_ac[peak_lag_index]; // Update y_0_ref to the new peak's value
                }
            }
            // --- END ADDED ---

            // --- Parabolic Interpolation for Refined Peak (using potentially updated peak_lag_index) ---+
            let y_0_for_interpolation = *y_0_ref; // Dereference y_0_ref for interpolation

            let refined_lag = if peak_lag_index > min_lag_frames && peak_lag_index < smoothed_ac.len() - 1 {
                let y_minus_1 = smoothed_ac[peak_lag_index - 1];
                let y_plus_1 = smoothed_ac[peak_lag_index + 1];
                let denominator = y_minus_1 - 2.0 * y_0_for_interpolation + y_plus_1;

                // Avoid division by zero or near-zero (flat peak)
                if denominator.abs() > 1e-6 {
                    let p = 0.5 * (y_minus_1 - y_plus_1) / denominator;
                    let clamped_p = p.max(-0.70).min(0.70); // Fine-tune BPM lag clamp to +/- 0.70
                    peak_lag_index as f32 + clamped_p
                } else {
                    peak_lag_index as f32 // Fallback for flat peak
                }
            } else {
                peak_lag_index as f32 // Fallback if peak is at edge
            };
            // --- End Parabolic Interpolation ---+

            // Convert lag index (number of frames) back to period in seconds
            let period_secs = refined_lag * hop_size as f32 / sample_rate;
            if period_secs > 1e-6 {
                let bpm = 60.0 / period_secs;
                Ok(bpm.clamp(config::BPM_MIN, config::BPM_MAX))
            } else {
                Err(BpmError::PeriodTooSmall)
            }
        }
        _ => Err(BpmError::NoAutocorrelationPeak),
    }
}

// --- Public Calculation Function ---

/// Analyze BPM and first beat offset in one pass.
pub(crate) fn analyze_bpm(samples: &[f32], sample_rate: f32) -> Result<(f32, f32), BpmError> {
    if samples.is_empty() {
        return Err(BpmError::EmptySamplesForBpm);
    }
    let frame_size = config::BPM_FRAME_SIZE;
    let hop_size = config::BPM_HOP_SIZE;
    let downsample_factor = config::BPM_DOWNSAMPLE_FACTOR;
    let mut processed_samples = samples.to_vec();
    normalize_in_place(&mut processed_samples);
    downsample_in_place(&mut processed_samples, downsample_factor);
    let effective_sample_rate = sample_rate / downsample_factor as f32;
    if processed_samples.is_empty() {
        return Err(BpmError::EmptyAfterDownsample {
            factor: downsample_factor,
            original_count: samples.len(),
        });
    }
    let flux = compute_spectral_flux(&processed_samples, frame_size, hop_size);
    if flux.is_empty() {
        return Err(BpmError::EmptyFluxVector);
    }
    let bpm = estimate_bpm(&flux, effective_sample_rate, hop_size)?;
    let smoothed_flux = if flux.len() >= 3 {
        let mut smoothed = Vec::with_capacity(flux.len());
        smoothed.push(flux[0]);
        
        // Parallel smoothing with pre-allocated output
        let middle_len = flux.len() - 2;
        let mut middle_smoothed = vec![0.0; middle_len];
        middle_smoothed.par_iter_mut().enumerate().for_each(|(i, s)| {
            *s = (flux[i] + flux[i+1] + flux[i+2]) / 3.0;
        });
        smoothed.extend(middle_smoothed);
        smoothed.push(flux[flux.len() - 1]);
        smoothed
    } else {
        flux.clone()
    };
    let mean_smoothed_flux = smoothed_flux.iter().copied().sum::<f32>() / (smoothed_flux.len() as f32);
    let threshold = mean_smoothed_flux * 1.05;
    let peaks = {
        let mut peaks = Vec::new();
        for i in 1..smoothed_flux.len().saturating_sub(1) {
            if smoothed_flux[i] > threshold && smoothed_flux[i] > smoothed_flux[i - 1] && smoothed_flux[i] > smoothed_flux[i + 1] {
                peaks.push(i);
            }
        }
        peaks
    };
    if peaks.is_empty() {
        return Err(BpmError::EmptySpectralFlux);
    }
    const MAX_FIRST_BEAT_CANDIDATE_TIME_SEC: f32 = 45.0;
    let max_candidate_flux_index = (MAX_FIRST_BEAT_CANDIDATE_TIME_SEC * effective_sample_rate / hop_size as f32).round() as usize;
    let early_candidates: Vec<usize> = peaks
        .iter()
        .filter(|&&p_idx| p_idx <= max_candidate_flux_index)
        .cloned()
        .collect();
    let best_first_peak = if !early_candidates.is_empty() {
        early_candidates[0]
    } else {
        peaks[0]
    };
    let refined_first_peak_index = if best_first_peak > 0 && best_first_peak < smoothed_flux.len() - 1 {
        let y_minus_1 = smoothed_flux[best_first_peak - 1];
        let y_0 = smoothed_flux[best_first_peak];
        let y_plus_1 = smoothed_flux[best_first_peak + 1];
        let denominator = y_minus_1 - 2.0 * y_0 + y_plus_1;
        if denominator.abs() > 1e-6 {
            let p = 0.5 * (y_minus_1 - y_plus_1) / denominator;
            let clamped_p = p.max(-0.5).min(0.5);
            best_first_peak as f32 + clamped_p
        } else {
            best_first_peak as f32
        }
    } else {
        best_first_peak as f32
    };
    let first_beat_sec = (refined_first_peak_index * hop_size as f32) / effective_sample_rate;
    Ok((bpm, first_beat_sec))
}
