// use crate::audio::config; // Commented out for CPAL Phase 1
// use crate::audio::errors::AudioEffectsError; // Commented out for CPAL Phase 1
// use crate::audio::types::EqParams; // Commented out for CPAL Phase 1
// use biquad::{Biquad as _, Coefficients, DirectForm1, ToHertz, Type}; // Commented out for CPAL Phase 1
// use rodio::{Sample, Source}; // Commented out for CPAL Phase 1
// use std::{ // Commented out for CPAL Phase 1
//     fmt::Debug, // Commented out for CPAL Phase 1
//     sync::{Arc, Mutex}, // Commented out for CPAL Phase 1
//     time::Duration, // Commented out for CPAL Phase 1
// }; // Commented out for CPAL Phase 1

// /// A custom Rodio source that applies Trim Gain and 3-band EQ (Low Shelf, Peaking, High Shelf)
// /// to an inner source.
// ///
// /// It uses an Arc<Mutex<EqParams>> to allow for thread-safe, real-time parameter updates.
// #[derive(Clone)] // Commented out for CPAL Phase 1
// pub struct EqSource<S> // Commented out for CPAL Phase 1
// where // Commented out for CPAL Phase 1
//     S: Source, // Commented out for CPAL Phase 1
//     S::Item: Sample + Send, // Commented out for CPAL Phase 1
// { // Commented out for CPAL Phase 1
//     inner: S, // Commented out for CPAL Phase 1
//     sample_rate: f32, // Commented out for CPAL Phase 1
//     params: Arc<Mutex<EqParams>>, // Commented out for CPAL Phase 1
//     trim_gain: Arc<Mutex<f32>>, // Commented out for CPAL Phase 1
//     low_shelf: DirectForm1<f32>, // Commented out for CPAL Phase 1
//     mid_peak: DirectForm1<f32>, // Commented out for CPAL Phase 1
//     high_shelf: DirectForm1<f32>, // Commented out for CPAL Phase 1
//     last_params: EqParams, // Commented out for CPAL Phase 1
// } // Commented out for CPAL Phase 1

// impl<S> EqSource<S> // Commented out for CPAL Phase 1
// where // Commented out for CPAL Phase 1
//     S: Source, // Commented out for CPAL Phase 1
//     S::Item: Sample + Send + Debug, // Commented out for CPAL Phase 1
//     f32: From<S::Item>, // Commented out for CPAL Phase 1
// { // Commented out for CPAL Phase 1
//     pub fn new( // Commented out for CPAL Phase 1
//         inner: S, // Commented out for CPAL Phase 1
//         params: Arc<Mutex<EqParams>>, // Commented out for CPAL Phase 1
//         trim_gain: Arc<Mutex<f32>>, // Commented out for CPAL Phase 1
//     ) -> Result<Self, AudioEffectsError> { // Commented out for CPAL Phase 1
//         let sample_rate = inner.sample_rate() as f32; // Commented out for CPAL Phase 1
//         let current_params = params // Commented out for CPAL Phase 1
//             .lock() // Commented out for CPAL Phase 1
//             .map_err(|_| AudioEffectsError::EqParamsLockError { // Commented out for CPAL Phase 1
//                 reason: "Mutex poisoned on creation".to_string(), // Commented out for CPAL Phase 1
//             })? // Commented out for CPAL Phase 1
//             .clone(); // Commented out for CPAL Phase 1
// 
//         let low_coeffs = calculate_low_shelf(sample_rate, current_params.low_gain_db)?; // Commented out for CPAL Phase 1
//         let mid_coeffs = calculate_mid_peak(sample_rate, current_params.mid_gain_db)?; // Commented out for CPAL Phase 1
//         let high_coeffs = calculate_high_shelf(sample_rate, current_params.high_gain_db)?; // Commented out for CPAL Phase 1
// 
//         Ok(EqSource { // Commented out for CPAL Phase 1
//             inner, // Commented out for CPAL Phase 1
//             sample_rate, // Commented out for CPAL Phase 1
//             params, // Commented out for CPAL Phase 1
//             trim_gain, // Commented out for CPAL Phase 1
//             low_shelf: DirectForm1::<f32>::new(low_coeffs), // Commented out for CPAL Phase 1
//             mid_peak: DirectForm1::<f32>::new(mid_coeffs), // Commented out for CPAL Phase 1
//             high_shelf: DirectForm1::<f32>::new(high_coeffs), // Commented out for CPAL Phase 1
//             last_params: current_params, // Commented out for CPAL Phase 1
//         }) // Commented out for CPAL Phase 1
//     } // Commented out for CPAL Phase 1
// 
//     fn update_filters_if_needed(&mut self) -> Result<(), AudioEffectsError> { // Commented out for CPAL Phase 1
//         let params_changed; // Commented out for CPAL Phase 1
//         let new_params; // Commented out for CPAL Phase 1
//         { // Commented out for CPAL Phase 1
//             let current_params_guard = // Commented out for CPAL Phase 1
//                 self.params // Commented out for CPAL Phase 1
//                     .lock() // Commented out for CPAL Phase 1
//                     .map_err(|_| AudioEffectsError::EqParamsLockError { // Commented out for CPAL Phase 1
//                         reason: "Mutex poisoned during update check".to_string(), // Commented out for CPAL Phase 1
//                     })?; // Commented out for CPAL Phase 1
//             params_changed = !self.last_params.approx_eq(&current_params_guard); // Commented out for CPAL Phase 1
//             if params_changed { // Commented out for CPAL Phase 1
//                 new_params = current_params_guard.clone(); // Commented out for CPAL Phase 1
//             } else { // Commented out for CPAL Phase 1
//                 return Ok(()); // No changes, exit early // Commented out for CPAL Phase 1
//             } // Commented out for CPAL Phase 1
//         } // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//         let low_coeffs = calculate_low_shelf(self.sample_rate, new_params.low_gain_db)?; // Commented out for CPAL Phase 1
//         let mid_coeffs = calculate_mid_peak(self.sample_rate, new_params.mid_gain_db)?; // Commented out for CPAL Phase 1
//         let high_coeffs = calculate_high_shelf(self.sample_rate, new_params.high_gain_db)?; // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//         self.low_shelf.update_coefficients(low_coeffs); // Commented out for CPAL Phase 1
//         self.mid_peak.update_coefficients(mid_coeffs); // Commented out for CPAL Phase 1
//         self.high_shelf.update_coefficients(high_coeffs); // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//         self.last_params = new_params; // Commented out for CPAL Phase 1
//         Ok(()) // Commented out for CPAL Phase 1
//     } // Commented out for CPAL Phase 1
// } // Commented out for CPAL Phase 1

// --- Filter Calculation Helpers ---
/* // Commented out for CPAL Phase 1
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
*/ // Commented out for CPAL Phase 1

// --- Source Trait Implementation ---

// impl<S> Iterator for EqSource<S> // Commented out for CPAL Phase 1
// where // Commented out for CPAL Phase 1
//     S: Source, // Commented out for CPAL Phase 1
//     S::Item: Sample + Send + Debug, // Commented out for CPAL Phase 1
//     f32: From<S::Item>, // Commented out for CPAL Phase 1
//     S::Item: From<f32>, // Commented out for CPAL Phase 1
// { // Commented out for CPAL Phase 1
//     type Item = S::Item; // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//     #[inline] // Commented out for CPAL Phase 1
//     fn next(&mut self) -> Option<Self::Item> { // Commented out for CPAL Phase 1
//         if let Err(e) = self.update_filters_if_needed() { // Commented out for CPAL Phase 1
//             log::error!( // Commented out for CPAL Phase 1
//                 "Failed to update EQ filters during playback: {:?}. Audio may be incorrect or stop.", // Commented out for CPAL Phase 1
//                 e // Commented out for CPAL Phase 1
//             ); // Commented out for CPAL Phase 1
//         } // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//         let sample_option = self.inner.next(); // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//         if let Some(sample) = sample_option { // Commented out for CPAL Phase 1
//             let sample_f32: f32 = sample.into(); // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//             let current_trim_gain = *self // Commented out for CPAL Phase 1
//                 .trim_gain // Commented out for CPAL Phase 1
//                 .lock() // Commented out for CPAL Phase 1
//                 .expect("Trim gain Mutex poisoned in EQSource::next"); // Commented out for CPAL Phase 1
//             let trimmed_sample = sample_f32 * current_trim_gain; // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//             let low_processed = self.low_shelf.run(trimmed_sample); // Commented out for CPAL Phase 1
//             let mid_processed = self.mid_peak.run(low_processed); // Commented out for CPAL Phase 1
//             let high_processed = self.high_shelf.run(mid_processed); // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//             if !high_processed.is_finite() { // Commented out for CPAL Phase 1
//                 log::error!( // Commented out for CPAL Phase 1
//                     "EQ produced non-finite value: {} (input {} -> trim {} -> low {} -> mid {})", // Commented out for CPAL Phase 1
//                     high_processed, // Commented out for CPAL Phase 1
//                     sample_f32, // Commented out for CPAL Phase 1
//                     trimmed_sample, // Commented out for CPAL Phase 1
//                     low_processed, // Commented out for CPAL Phase 1
//                     mid_processed // Commented out for CPAL Phase 1
//                 ); // Commented out for CPAL Phase 1
//             } // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//             Some(high_processed.into()) // Commented out for CPAL Phase 1
//         } else { // Commented out for CPAL Phase 1
//             None // Commented out for CPAL Phase 1
//         } // Commented out for CPAL Phase 1
//     } // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//     #[inline] // Commented out for CPAL Phase 1
//     fn size_hint(&self) -> (usize, Option<usize>) { // Commented out for CPAL Phase 1
//         self.inner.size_hint() // Commented out for CPAL Phase 1
//     } // Commented out for CPAL Phase 1
// } // Commented out for CPAL Phase 1

// impl<S> Source for EqSource<S> // Commented out for CPAL Phase 1
// where // Commented out for CPAL Phase 1
//     S: Source, // Commented out for CPAL Phase 1
//     S::Item: Sample + Send + Debug, // Commented out for CPAL Phase 1
//     f32: From<S::Item>, // Commented out for CPAL Phase 1
//     S::Item: From<f32>, // Commented out for CPAL Phase 1
// { // Commented out for CPAL Phase 1
//     #[inline] // Commented out for CPAL Phase 1
//     fn current_frame_len(&self) -> Option<usize> { // Commented out for CPAL Phase 1
//         self.inner.current_frame_len() // Commented out for CPAL Phase 1
//     } // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//     #[inline] // Commented out for CPAL Phase 1
//     fn channels(&self) -> u16 { // Commented out for CPAL Phase 1
//         self.inner.channels() // Commented out for CPAL Phase 1
//     } // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//     #[inline] // Commented out for CPAL Phase 1
//     fn sample_rate(&self) -> u32 { // Commented out for CPAL Phase 1
//         self.inner.sample_rate() // Commented out for CPAL Phase 1
//     } // Commented out for CPAL Phase 1
//  // Commented out for CPAL Phase 1
//     #[inline] // Commented out for CPAL Phase 1
//     fn total_duration(&self) -> Option<Duration> { // Commented out for CPAL Phase 1
//         self.inner.total_duration() // Commented out for CPAL Phase 1
//     } // Commented out for CPAL Phase 1
// } // Commented out for CPAL Phase 1
