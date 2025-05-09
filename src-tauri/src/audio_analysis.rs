use crate::errors::AudioAnalysisError;

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
    pub max_rms_amplitude: f32,
}

/// Calculates RMS volume intervals from pre-decoded mono f32 samples.
/// Made pub(crate) to be accessible from audio_processor.
/// Accepts sample_rate as f32.
/// TODO: This function needs a complete rewrite to implement FFT, band energy calculation, and mip-map generation.
pub(crate) fn calculate_rms_intervals(
    samples: &[f32],
    sample_rate: f32,
) -> Result<(Vec<Vec<WaveBin>>, f32), AudioAnalysisError> {
    if samples.is_empty() {
        log::warn!("Waveform Analysis: Cannot calculate from empty samples. Returning a single default WaveBin.");
        return Ok((vec![vec![WaveBin::default()]], 0.0));
    }
    if sample_rate <= 0.0 {
        return Err(AudioAnalysisError::InvalidSampleRate(sample_rate));
    }

    let n_samples_per_bin = 512;
    let num_bins_level_0 = (samples.len() as f64 / n_samples_per_bin as f64).ceil() as usize;

    let mut level_0_bins: Vec<WaveBin> = Vec::with_capacity(num_bins_level_0);
    let mut calculated_max_rms: f32 = 0.0;

    for chunk in samples.chunks(n_samples_per_bin) {
        if chunk.is_empty() {
            continue;
        }
        let sum_sq: f64 = chunk.iter().map(|&s| (s as f64).powi(2)).sum();
        let mean_sq = sum_sq / chunk.len() as f64;
        let rms = mean_sq.sqrt().max(0.0) as f32;
        calculated_max_rms = calculated_max_rms.max(rms);

        level_0_bins.push(WaveBin {
            low: rms * 0.3,
            mid: rms * 0.4,
            high: rms * 0.3,
        });
    }
    
    if calculated_max_rms < f32::EPSILON && !level_0_bins.is_empty() {
        calculated_max_rms = f32::EPSILON;
    }

    let pyramid: Vec<Vec<WaveBin>> = vec![level_0_bins];

    Ok((pyramid, calculated_max_rms))
}
