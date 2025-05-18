use crate::audio::config;
use crate::audio::errors::AudioEffectsError;

use biquad::{Coefficients, ToHertz, Type};

pub(crate) fn calculate_low_shelf(
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

pub(crate) fn calculate_mid_peak(
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

pub(crate) fn calculate_high_shelf(
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
