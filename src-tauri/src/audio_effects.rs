use rodio::{Sample, Source};
use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
    time::Duration,
};
// Use biquad types directly
use crate::playback_types::EqParams;
use biquad::{Biquad as _, Coefficients, DirectForm1, ToHertz, Type};

/// A custom Rodio source that applies Trim Gain and 3-band EQ (Low Shelf, Peaking, High Shelf)
/// to an inner source.
///
/// It uses an Arc<Mutex<EqParams>> to allow for thread-safe, real-time parameter updates.
#[derive(Clone)]
pub struct EqSource<S>
where
    S: Source,
    S::Item: Sample + Send,
{
    inner: S,
    sample_rate: f32,
    params: Arc<Mutex<EqParams>>,
    trim_gain: Arc<Mutex<f32>>,
    low_shelf: Arc<Mutex<DirectForm1<f32>>>,
    mid_peak: Arc<Mutex<DirectForm1<f32>>>,
    high_shelf: Arc<Mutex<DirectForm1<f32>>>,
    last_params: EqParams,
}

// Define default filter parameters
const LOW_MID_CROSSOVER_HZ: f32 = 250.0;
const MID_HIGH_CROSSOVER_HZ: f32 = 3000.0;
const MID_CENTER_HZ: f32 = 1000.0;
const MID_Q_FACTOR: f32 = std::f32::consts::FRAC_1_SQRT_2; // Butterworth Q for peaking
const SHELF_Q_FACTOR: f32 = 0.5; // Changed from 1/sqrt(2)

impl<S> EqSource<S>
where
    S: Source,
    S::Item: Sample + Send + Debug,
    f32: From<S::Item>,
{
    pub fn new(inner: S, params: Arc<Mutex<EqParams>>, trim_gain: Arc<Mutex<f32>>) -> Self {
        let sample_rate = inner.sample_rate() as f32;
        let current_params = params
            .lock()
            .expect("Failed to lock EQ params on creation")
            .clone();

        let low_coeffs = calculate_low_shelf(sample_rate, current_params.low_gain_db);
        let mid_coeffs = calculate_mid_peak(sample_rate, current_params.mid_gain_db);
        let high_coeffs = calculate_high_shelf(sample_rate, current_params.high_gain_db);

        EqSource {
            inner,
            sample_rate,
            params,
            trim_gain,
            low_shelf: Arc::new(Mutex::new(DirectForm1::<f32>::new(low_coeffs))),
            mid_peak: Arc::new(Mutex::new(DirectForm1::<f32>::new(mid_coeffs))),
            high_shelf: Arc::new(Mutex::new(DirectForm1::<f32>::new(high_coeffs))),
            last_params: current_params,
        }
    }

    fn update_filters_if_needed(&mut self) {
        let params_changed;
        let new_params;
        {
            let current_params_guard = self
                .params
                .lock()
                .expect("Failed to lock EQ params for checking");
            params_changed = !self.last_params.approx_eq(&current_params_guard);
            if params_changed {
                new_params = current_params_guard.clone();
            } else {
                return; // No changes, exit early
            }
        } // Lock released here

        // Recalculate coefficients outside the lock
        log::debug!(
            "EQ Params changed, recalculating coefficients: {:?}",
            new_params
        );
        let low_coeffs = calculate_low_shelf(self.sample_rate, new_params.low_gain_db);
        let mid_coeffs = calculate_mid_peak(self.sample_rate, new_params.mid_gain_db);
        let high_coeffs = calculate_high_shelf(self.sample_rate, new_params.high_gain_db);

        // --- DEBUG LOG: Log calculated coefficients ---
        log::debug!("New Low Shelf Coeffs: {:?}", low_coeffs);
        // --- END DEBUG LOG ---

        // Lock each filter individually to update coefficients
        self.low_shelf
            .lock()
            .expect("Failed to lock low shelf")
            .update_coefficients(low_coeffs);
        self.mid_peak
            .lock()
            .expect("Failed to lock mid peak")
            .update_coefficients(mid_coeffs);
        self.high_shelf
            .lock()
            .expect("Failed to lock high shelf")
            .update_coefficients(high_coeffs);

        self.last_params = new_params; // Update local cache
    }
}

// --- Filter Calculation Helpers ---

fn calculate_low_shelf(sample_rate: f32, gain_db: f32) -> Coefficients<f32> {
    Coefficients::<f32>::from_params(
        Type::LowShelf(gain_db),
        sample_rate.hz(),
        LOW_MID_CROSSOVER_HZ.hz(),
        SHELF_Q_FACTOR,
    )
    .expect("Failed to calculate low shelf coeffs")
}

fn calculate_mid_peak(sample_rate: f32, gain_db: f32) -> Coefficients<f32> {
    Coefficients::<f32>::from_params(
        Type::PeakingEQ(gain_db),
        sample_rate.hz(),
        MID_CENTER_HZ.hz(),
        MID_Q_FACTOR,
    )
    .expect("Failed to calculate mid peak coeffs")
}

fn calculate_high_shelf(sample_rate: f32, gain_db: f32) -> Coefficients<f32> {
    let coeffs = Coefficients::<f32>::from_params(
        Type::HighShelf(gain_db),
        sample_rate.hz(),
        MID_HIGH_CROSSOVER_HZ.hz(),
        SHELF_Q_FACTOR,
    )
    .expect("Failed to calculate high shelf coeffs");
    // Log the calculated coefficients using INFO level
    log::info!("Calculated High Shelf Coeffs (Gain: {}dB): {:?}", gain_db, coeffs);
    coeffs
}

// --- Source Trait Implementation ---

impl<S> Iterator for EqSource<S>
where
    S: Source,
    S::Item: Sample + Send + Debug,
    f32: From<S::Item>,
    S::Item: From<f32>,
{
    type Item = S::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.update_filters_if_needed();

        let sample_option = self.inner.next();

        if let Some(sample) = sample_option {
            let sample_f32: f32 = sample.into();

            // Apply Trim Gain first
            let current_trim_gain = *self.trim_gain.lock().expect("Failed to lock trim gain");
            let trimmed_sample = sample_f32 * current_trim_gain;

            // Apply Filters sequentially
            let low_processed = self
                .low_shelf
                .lock()
                .expect("Failed to lock low shelf in next")
                .run(trimmed_sample);
            let mid_processed = self
                .mid_peak
                .lock()
                .expect("Failed to lock mid peak in next")
                .run(low_processed);
            let high_processed = self
                .high_shelf
                .lock()
                .expect("Failed to lock high shelf in next")
                .run(mid_processed);

            // --- DEBUG LOGGING (High Shelf Output) ---
            // Keep this temporarily to see the final output of the chain
            if sample_f32 != 0.0 {
                log::debug!(
                    "Sample In: {:.6}, Final EQ Out: {:.6}", // Changed label
                    sample_f32,
                    high_processed
                );
                if !high_processed.is_finite() {
                    log::error!(
                        "!!! Final EQ produced non-finite value: {} (input was {})", // Changed label
                        high_processed, sample_f32
                    );
                }
            }
            // --- END DEBUG LOGGING ---

            Some(high_processed.into())
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S> Source for EqSource<S>
where
    S: Source,
    S::Item: Sample + Send + Debug,
    f32: From<S::Item>,
    S::Item: From<f32>,
{
    #[inline]
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }

    #[inline]
    fn channels(&self) -> u16 {
        // Ensure the source is mono for this EQ setup, or adapt filters for stereo
        debug_assert_eq!(
            self.inner.channels(),
            1,
            "EQ Source currently only supports mono input"
        );
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}
