use crate::audio::config;
use crate::audio::errors::AudioAnalysisError;
use crate::audio::types::{WaveBin};
use rustfft::{FftPlanner, num_complex::Complex, num_traits::Zero};
use rayon::prelude::*;
use std::f32::consts::PI;
use std::sync::Arc;

impl Default for WaveBin {
    fn default() -> Self {
        WaveBin {
            low: 0.0,
            mid: 0.0,
            high: 0.0,
        }
    }
}


fn get_hann_window(size: usize) -> Arc<Vec<f32>> {
    if size == 0 {
        return Arc::new(Vec::new());
    }
    Arc::new((0..size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (size - 1) as f32).cos()))
        .collect())
}

/// Calculate the multi-band energy levels from pre-decoded mono f32 samples using FFT.
pub(crate) fn calculate_rms_intervals(
    samples: &[f32],
    sample_rate: f32,
) -> Result<(Vec<Vec<WaveBin>>, f32), AudioAnalysisError> {
    if samples.is_empty() {
        log::warn!("Waveform Analysis: Cannot calculate from empty samples. Returning default.");
        return Err(AudioAnalysisError::EmptySamples);
    }
    if sample_rate <= 0.0 {
        return Err(AudioAnalysisError::InvalidSampleRate(sample_rate));
    }
    
    const FRAME_SIZE: usize = config::WAVEFORM_FRAME_SIZE;
    const HOP_SIZE: usize = config::WAVEFORM_HOP_SIZE;
    
    if samples.len() < FRAME_SIZE {
        log::warn!(
            "Waveform Analysis: Not enough samples ({}) for a single frame ({}). Returning default.",
            samples.len(),
            FRAME_SIZE
        );
        let (low, mid, high) = simple_energy_fallback(samples);
        return Ok((
            vec![vec![WaveBin { low, mid, high }]],
            low.max(mid.max(high)).max(f32::EPSILON),
        ));
    }
    
    let mut planner = FftPlanner::new();
    let fft = Arc::new(planner.plan_fft_forward(FRAME_SIZE));
    let hann_window = get_hann_window(FRAME_SIZE);
    let num_frames = (samples.len() - FRAME_SIZE) / HOP_SIZE + 1;
    
    // Pre-compute frequency boundaries for band separation
    let freq_per_bin = sample_rate / FRAME_SIZE as f32;
    let low_mid_bin = (config::LOW_MID_CROSSOVER_HZ / freq_per_bin).round() as usize;
    let mid_high_bin = (config::MID_HIGH_CROSSOVER_HZ / freq_per_bin).round() as usize;
    let max_bin = FRAME_SIZE / 2 + 1;
    
    // Parallel processing of frames for massive performance improvement
    let level_0_bins: Vec<WaveBin> = (0..num_frames)
        .into_par_iter()
        .map(|i| {
            let start = i * HOP_SIZE;
            let end = (start + FRAME_SIZE).min(samples.len());
            let frame_slice = &samples[start..end];
            
            // Thread-local FFT buffer
            let mut fft_buffer: Vec<Complex<f32>> = Vec::with_capacity(FRAME_SIZE);
            fft_buffer.resize(FRAME_SIZE, Complex::zero());
            
            // Apply windowing and copy to buffer in single pass
            for (j, (&sample, &window)) in frame_slice.iter().zip(hann_window.iter()).enumerate() {
                fft_buffer[j] = Complex {
                    re: sample * window,
                    im: 0.0,
                };
            }
            
            fft.process(&mut fft_buffer);
            
            // Fast band energy calculation using pre-computed bin boundaries
            let mut low_energy = 0.0f32;
            let mut mid_energy = 0.0f32;
            let mut high_energy = 0.0f32;
            
            // Vectorized magnitude calculation and band assignment
            for k in 0..max_bin {
                let magnitude = fft_buffer[k].norm();
                if k < low_mid_bin {
                    low_energy += magnitude;
                } else if k < mid_high_bin {
                    mid_energy += magnitude;
                } else {
                    high_energy += magnitude;
                }
            }
            
            WaveBin {
                low: low_energy,
                mid: mid_energy,
                high: high_energy,
            }
        })
        .collect();
    
    // Find maximum energy across all bands in parallel
    let max_overall_band_energy = level_0_bins
        .par_iter()
        .map(|bin| bin.low.max(bin.mid.max(bin.high)))
        .reduce(|| 0.0, f32::max)
        .max(f32::EPSILON);
    
    let pyramid: Vec<Vec<WaveBin>> = vec![level_0_bins];
    Ok((pyramid, max_overall_band_energy))
}

fn simple_energy_fallback(samples: &[f32]) -> (f32, f32, f32) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let total_energy_proxy: f32 = samples.iter().map(|s| s.abs()).sum();
    let energy = total_energy_proxy / samples.len() as f32;

    (energy * 0.3, energy * 0.4, energy * 0.3)
}
