use crate::audio::config;
use crate::audio::errors::AudioEffectsError;
use crate::audio::types::EqParams;
use biquad::{Biquad as _, Coefficients, DirectForm1, ToHertz, Type};
use rodio::{Sample, Source};
use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
    time::Duration,
};

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
    low_shelf: DirectForm1<f32>,
    mid_peak: DirectForm1<f32>,
    high_shelf: DirectForm1<f32>,
    last_params: EqParams,
}

impl<S> EqSource<S>
where
    S: Source,
    S::Item: Sample + Send + Debug,
    f32: From<S::Item>,
{
    pub fn new(
        inner: S,
        params: Arc<Mutex<EqParams>>,
        trim_gain: Arc<Mutex<f32>>,
    ) -> Result<Self, AudioEffectsError> {
        let sample_rate = inner.sample_rate() as f32;
        let current_params = params
            .lock()
            .map_err(|_| AudioEffectsError::EqParamsLockError {
                reason: "Mutex poisoned on creation".to_string(),
            })?
            .clone();

        let low_coeffs = calculate_low_shelf(sample_rate, current_params.low_gain_db)?;
        let mid_coeffs = calculate_mid_peak(sample_rate, current_params.mid_gain_db)?;
        let high_coeffs = calculate_high_shelf(sample_rate, current_params.high_gain_db)?;

        Ok(EqSource {
            inner,
            sample_rate,
            params,
            trim_gain,
            low_shelf: DirectForm1::<f32>::new(low_coeffs),
            mid_peak: DirectForm1::<f32>::new(mid_coeffs),
            high_shelf: DirectForm1::<f32>::new(high_coeffs),
            last_params: current_params,
        })
    }

    fn update_filters_if_needed(&mut self) -> Result<(), AudioEffectsError> {
        let params_changed;
        let new_params;
        {
            let current_params_guard =
                self.params
                    .lock()
                    .map_err(|_| AudioEffectsError::EqParamsLockError {
                        reason: "Mutex poisoned during update check".to_string(),
                    })?;
            params_changed = !self.last_params.approx_eq(&current_params_guard);
            if params_changed {
                new_params = current_params_guard.clone();
            } else {
                return Ok(()); // No changes, exit early
            }
        }

        let low_coeffs = calculate_low_shelf(self.sample_rate, new_params.low_gain_db)?;
        let mid_coeffs = calculate_mid_peak(self.sample_rate, new_params.mid_gain_db)?;
        let high_coeffs = calculate_high_shelf(self.sample_rate, new_params.high_gain_db)?;

        self.low_shelf.update_coefficients(low_coeffs);
        self.mid_peak.update_coefficients(mid_coeffs);
        self.high_shelf.update_coefficients(high_coeffs);

        self.last_params = new_params;
        Ok(())
    }
}

// --- Filter Calculation Helpers ---

fn calculate_low_shelf(
    sample_rate: f32,
    gain_db: f32,
) -> Result<Coefficients<f32>, AudioEffectsError> {
    Coefficients::<f32>::from_params(
        Type::LowShelf(gain_db),
        sample_rate.hz(),
        config::LOW_MID_CROSSOVER_HZ.hz(),
        config::SHELF_Q_FACTOR,
    )
    .map_err(|e| AudioEffectsError::CoefficientCalculationError {
        filter_type: format!("LowShelf: {:?}", e),
    })
}

fn calculate_mid_peak(
    sample_rate: f32,
    gain_db: f32,
) -> Result<Coefficients<f32>, AudioEffectsError> {
    Coefficients::<f32>::from_params(
        Type::PeakingEQ(gain_db),
        sample_rate.hz(),
        config::MID_CENTER_HZ.hz(),
        config::MID_PEAK_Q_FACTOR,
    )
    .map_err(|e| AudioEffectsError::CoefficientCalculationError {
        filter_type: format!("MidPeak: {:?}", e),
    })
}

fn calculate_high_shelf(
    sample_rate: f32,
    gain_db: f32,
) -> Result<Coefficients<f32>, AudioEffectsError> {
    Coefficients::<f32>::from_params(
        Type::HighShelf(gain_db),
        sample_rate.hz(),
        config::MID_HIGH_CROSSOVER_HZ.hz(),
        config::SHELF_Q_FACTOR,
    )
    .map_err(|e| AudioEffectsError::CoefficientCalculationError {
        filter_type: format!("HighShelf: {:?}", e),
    })
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
        if let Err(e) = self.update_filters_if_needed() {
            log::error!(
                "Failed to update EQ filters during playback: {:?}. Audio may be incorrect or stop.",
                e
            );
        }

        let sample_option = self.inner.next();

        if let Some(sample) = sample_option {
            let sample_f32: f32 = sample.into();

            let current_trim_gain = *self
                .trim_gain
                .lock()
                .expect("Trim gain Mutex poisoned in EQSource::next");
            let trimmed_sample = sample_f32 * current_trim_gain;

            let low_processed = self.low_shelf.run(trimmed_sample);
            let mid_processed = self.mid_peak.run(low_processed);
            let high_processed = self.high_shelf.run(mid_processed);

            if !high_processed.is_finite() {
                log::error!(
                    "EQ produced non-finite value: {} (input {} -> trim {} -> low {} -> mid {})",
                    high_processed,
                    sample_f32,
                    trimmed_sample,
                    low_processed,
                    mid_processed
                );
            }

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
