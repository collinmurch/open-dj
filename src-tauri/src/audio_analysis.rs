use rayon::prelude::*;

#[derive(serde::Serialize, Debug, Clone)]
pub struct VolumeInterval {
    start_time: f64,
    end_time: f64,
    rms_amplitude: f32,
}

#[derive(serde::Serialize, Debug, Clone)]
pub struct AudioAnalysis {
    pub intervals: Vec<VolumeInterval>,
    pub max_rms_amplitude: f32,
}

/// Calculates RMS volume intervals from pre-decoded mono f32 samples.
/// Made pub(crate) to be accessible from audio_processor.
/// Accepts sample_rate as f32.
pub(crate) fn calculate_rms_intervals(
    samples: &[f32],
    sample_rate: f32,
) -> Result<(Vec<VolumeInterval>, f32), String> {
    // Return Result
    if samples.is_empty() {
        // Return empty results, not an error necessarily, but log warning
        log::warn!("Volume Calc: Cannot calculate RMS from empty samples.");
        return Ok((Vec::new(), 0.0));
    }
    if sample_rate <= 0.0 {
        return Err(format!("Volume Calc: Invalid sample rate: {}", sample_rate));
    }

    const TARGET_INTERVALS_PER_SECOND: f64 = 25.0;
    // Ensure sample_rate cast doesn't truncate to 0
    let samples_per_interval = ((sample_rate as f64) / TARGET_INTERVALS_PER_SECOND)
        .round()
        .max(1.0) as usize;

    let total_duration_seconds = samples.len() as f64 / sample_rate as f64;
    let num_intervals = (samples.len() as f64 / samples_per_interval as f64).ceil() as usize;

    log::debug!(
        "Volume Calc: Calculating {} intervals for {} samples at {} Hz ({} samples/interval)",
        num_intervals,
        samples.len(),
        sample_rate,
        samples_per_interval
    );

    let mut intervals: Vec<VolumeInterval> = Vec::with_capacity(num_intervals);
    let mut max_rms_amplitude: f32 = 0.0;

    for (i, chunk) in samples.chunks(samples_per_interval).enumerate() {
        if chunk.is_empty() {
            continue;
        }
        let sum_sq: f64 = chunk.par_iter().map(|&s| (s as f64).powi(2)).sum(); // Parallel sum
        let mean_sq = sum_sq / chunk.len() as f64;
        let rms = mean_sq.sqrt().max(0.0) as f32;

        max_rms_amplitude = max_rms_amplitude.max(rms);

        let start_sample_index = i * samples_per_interval;
        let end_sample_index = start_sample_index + chunk.len();

        let start_time = start_sample_index as f64 / sample_rate as f64;
        let end_time = (end_sample_index as f64 / sample_rate as f64).min(total_duration_seconds);

        intervals.push(VolumeInterval {
            start_time,
            end_time,
            rms_amplitude: rms,
        });
    }

    // Ensure max_rms is non-zero if we have intervals, preventing division by zero later
    // Use a small epsilon instead of 0.0001 for robustness
    if max_rms_amplitude < f32::EPSILON && !intervals.is_empty() {
        max_rms_amplitude = f32::EPSILON; // Use machine epsilon
    }

    log::debug!(
        "Volume Calc: Calculated RMS for {} intervals. Max RMS: {:.4}",
        intervals.len(),
        max_rms_amplitude
    );

    Ok((intervals, max_rms_amplitude))
}
