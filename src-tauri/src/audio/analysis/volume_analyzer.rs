use crate::audio::errors::AudioAnalysisError; // Adjusted path
use crate::audio::config; // Adjusted path
use rustfft::{FftPlanner, num_complex::Complex, num_traits::Zero}; 
use std::f32::consts::PI; 

#[derive(serde::Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct WaveBin {
    pub low: f32,
    pub mid: f32,
    pub high: f32,
}

impl Default for WaveBin {
    fn default() -> Self {
        WaveBin { low: 0.0, mid: 0.0, high: 0.0 }
    }
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct AudioAnalysis {
    pub levels: Vec<Vec<WaveBin>>,
    pub max_band_energy: f32, 
}

/// Helper function to generate a Hann window.
fn get_hann_window(size: usize) -> Vec<f32> {
    if size == 0 {
        return Vec::new();
    }
    (0..size)
        .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (size - 1) as f32).cos()))
        .collect()
}

/// Calculates multi-band energy levels from pre-decoded mono f32 samples using FFT.
pub(crate) fn calculate_rms_intervals(
    samples: &[f32],
    sample_rate: f32,
) -> Result<(Vec<Vec<WaveBin>>, f32), AudioAnalysisError> {
    if samples.is_empty() {
        log::warn!("Waveform Analysis: Cannot calculate from empty samples. Returning default.");
        return Ok((vec![vec![WaveBin::default()]], 0.0));
    }
    if sample_rate <= 0.0 {
        return Err(AudioAnalysisError::InvalidSampleRate(sample_rate));
    }

    const FRAME_SIZE: usize = 1024;
    const HOP_SIZE: usize = FRAME_SIZE / 2; // 50% overlap

    if samples.len() < FRAME_SIZE {
        log::warn!(
            "Waveform Analysis: Not enough samples ({}) for a single frame ({}). Returning default.",
            samples.len(),
            FRAME_SIZE
        );
        let (low, mid, high) = simple_energy_fallback(samples);
         return Ok((vec![vec![WaveBin { low, mid, high }]], low.max(mid.max(high)).max(f32::EPSILON) ));
    }

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(FRAME_SIZE);
    let hann_window = get_hann_window(FRAME_SIZE);

    let num_frames = (samples.len() - FRAME_SIZE) / HOP_SIZE + 1;
    let mut level_0_bins: Vec<WaveBin> = Vec::with_capacity(num_frames);
    let mut max_overall_band_energy: f32 = 0.0;

    let mut fft_buffer: Vec<Complex<f32>> = vec![Complex::zero(); FRAME_SIZE];

    for i in 0..num_frames {
        let start = i * HOP_SIZE;
        let end = start + FRAME_SIZE;
        let frame_slice = &samples[start..end];

        for (j, sample) in frame_slice.iter().enumerate() {
            fft_buffer[j] = Complex { re: sample * hann_window[j], im: 0.0 };
        }

        fft.process(&mut fft_buffer);

        let mut low_energy = 0.0f32;
        let mut mid_energy = 0.0f32;
        let mut high_energy = 0.0f32;

        for k in 0..(FRAME_SIZE / 2 + 1) {
            let magnitude = fft_buffer[k].norm(); 
            let freq_k = k as f32 * sample_rate / FRAME_SIZE as f32;

            if freq_k < config::LOW_MID_CROSSOVER_HZ {
                low_energy += magnitude;
            } else if freq_k < config::MID_HIGH_CROSSOVER_HZ {
                mid_energy += magnitude;
            } else {
                high_energy += magnitude;
            }
        }
        
        level_0_bins.push(WaveBin { low: low_energy, mid: mid_energy, high: high_energy });

        max_overall_band_energy = max_overall_band_energy.max(low_energy).max(mid_energy).max(high_energy);
    }
    
    if max_overall_band_energy < f32::EPSILON && !level_0_bins.is_empty() {
        max_overall_band_energy = f32::EPSILON; 
    }

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