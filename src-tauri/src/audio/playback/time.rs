use std::time::Duration;
use super::state::AudioThreadDeckState;

// Added imports for the new function
use std::collections::HashMap;
use tauri::{AppHandle, Runtime};
use crate::audio::config;
use super::events::{emit_status_update_event, emit_tick_event, emit_pitch_tick_event};
use super::handlers; // Added for audio_thread_handle_set_pitch_rate
use super::sync::{self, PLL_KI, MAX_PLL_INTEGRAL_ERROR, MAX_PLL_PITCH_ADJUSTMENT};

pub(crate) fn get_current_playback_time_secs(deck_state: &AudioThreadDeckState) -> f64 {
    if deck_state.is_playing {
        if let Some(start_time) = deck_state.playback_start_time {
            let elapsed = start_time.elapsed();
            let current_rate = *deck_state.current_pitch_rate.lock().unwrap();
            let audio_advanced_secs = elapsed.as_secs_f64() * current_rate as f64;
            let base_audio_time_secs = deck_state.paused_position.unwrap_or(Duration::ZERO).as_secs_f64();
            return (base_audio_time_secs + audio_advanced_secs).min(deck_state.duration.as_secs_f64());
        }
    } else if let Some(paused_pos) = deck_state.paused_position {
        return paused_pos
            .as_secs_f64()
            .min(deck_state.duration.as_secs_f64());
    }
    0.0
}

// New function: process_time_slice_updates (moved from handlers.rs)
pub(crate) fn process_time_slice_updates<R: Runtime>(
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    let mut decks_to_update: HashMap<String, (f64, bool)> = HashMap::new();

    for (deck_id, deck_state) in local_states.iter() { // Changed to iter() as we only read here for now
        if deck_state.is_playing && !deck_state.sink.is_paused() && !deck_state.sink.empty() {
            let current_time_secs = get_current_playback_time_secs(deck_state); 
            let end_buffer = Duration::from_millis(config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS + 10);
            let has_ended = deck_state.duration > Duration::ZERO
                && (Duration::from_secs_f64(current_time_secs) + end_buffer >= deck_state.duration);

            decks_to_update.insert(deck_id.clone(), (current_time_secs, has_ended));
         
        } else if deck_state.is_master { 
            if !decks_to_update.contains_key(deck_id) { 
                 let current_time_secs = get_current_playback_time_secs(deck_state); 
                 decks_to_update.insert(deck_id.clone(), (current_time_secs, false)); 
            }
        }
    }

    let slave_pitch_info_map = sync::calculate_pll_pitch_updates(local_states, &decks_to_update);

    for (deck_id_str, (current_time, has_ended)) in decks_to_update {
        let deck_id = deck_id_str.as_str(); // Use &str for local_states access

        let mut pitch_to_apply_to_engine: Option<f32> = None;

        if let Some(deck_state_for_pll_calc) = local_states.get_mut(deck_id) { // Get mutable state once for this deck
            if let Some(&(proportional_correction, signed_error)) = slave_pitch_info_map.get(deck_id) {
                let dt = Duration::from_millis(config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS).as_secs_f32();
                deck_state_for_pll_calc.pll_integral_error += signed_error * dt;
                deck_state_for_pll_calc.pll_integral_error = deck_state_for_pll_calc.pll_integral_error.clamp(-MAX_PLL_INTEGRAL_ERROR, MAX_PLL_INTEGRAL_ERROR);

                const PLL_P_CORRECTION_DEADBAND: f32 = 0.015; // If phase error < 1.5% of a beat, P-term is zero
                let effective_proportional_correction = if signed_error.abs() < PLL_P_CORRECTION_DEADBAND {
                    log::trace!("PLL P-Term DEAD BAND active for {}: signed_error {:.4} < deadband {:.4}", deck_id, signed_error, PLL_P_CORRECTION_DEADBAND);
                    0.0
                } else {
                    proportional_correction // Original P-term calculated as -signed_error * PLL_KP
                };

                let integral_correction = deck_state_for_pll_calc.pll_integral_error * PLL_KI;
                let total_correction = effective_proportional_correction + integral_correction;
                let clamped_total_correction = total_correction.clamp(-MAX_PLL_PITCH_ADJUSTMENT, MAX_PLL_PITCH_ADJUSTMENT);
                
                let final_target_pitch_for_slave = deck_state_for_pll_calc.target_pitch_rate_for_bpm_match + clamped_total_correction;
                let engine_pitch_candidate = final_target_pitch_for_slave.clamp(0.5, 2.0); // Final clamp for engine safety

                // Check if this new candidate pitch is different enough from current actual engine pitch
                let current_engine_pitch = *deck_state_for_pll_calc.current_pitch_rate.lock().unwrap();
                if (current_engine_pitch - engine_pitch_candidate).abs() > 1e-6 { // Threshold for applying to engine
                    pitch_to_apply_to_engine = Some(engine_pitch_candidate);
                }
                 log::info!(
                    "PLL Applied {}: TargetRateBPM={:.4}, P_corr(eff)={:.4} (raw P={:.4}), SgnErr={:.4}, I_corr={:.4} (IntegralSum={:.4}), TotalCorr(Clamped)={:.4}, FinalEnginePitch={:.4}", 
                    deck_id, deck_state_for_pll_calc.target_pitch_rate_for_bpm_match, effective_proportional_correction, proportional_correction, signed_error, integral_correction, deck_state_for_pll_calc.pll_integral_error, clamped_total_correction, engine_pitch_candidate
                );
            }
        } else {
            log::warn!("process_time_slice_updates: Deck state for '{}' not found during main PLL application phase.", deck_id);
            // Continue to next deck if state not found, though this shouldn't happen if decks_to_update is sourced from local_states
        }

        if let Some(new_engine_pitch) = pitch_to_apply_to_engine {
            handlers::audio_thread_handle_set_pitch_rate(
                deck_id,
                new_engine_pitch,
                false, // is_major_adjustment = false for PLL
                local_states, // local_states is mutably borrowed here
                app_handle,
            );
        }

        // Phase 3: Process UI updates and track end logic with a fresh mutable borrow of deck_state.
        // This ensures we are working with the state that might have been modified by set_pitch_rate.
        if let Some(deck_state) = local_states.get_mut(deck_id) {
            let final_engine_pitch = *deck_state.current_pitch_rate.lock().unwrap();

            // Decide if UI needs an update based on the actual engine pitch
            const UI_PITCH_UPDATE_THRESHOLD: f32 = 0.0005;
            let pitch_previously_sent_to_ui = deck_state.last_ui_pitch_rate.unwrap_or(deck_state.manual_pitch_rate);
            let mut ui_pitch_updated_this_cycle = false;

            if (final_engine_pitch - pitch_previously_sent_to_ui).abs() > UI_PITCH_UPDATE_THRESHOLD {
                emit_pitch_tick_event(app_handle, deck_id, final_engine_pitch);
                deck_state.last_ui_pitch_rate = Some(final_engine_pitch);
                log::info!(
                    "UI_PITCH_THROTTLE: Emitting THICK pitch-tick for {} to UI. Rate: {:.6} (Old UI: {:.6})",
                    deck_id, final_engine_pitch, pitch_previously_sent_to_ui
                );
                ui_pitch_updated_this_cycle = true;
            }

            // Handle track ending
            if has_ended {
                deck_state.sink.pause();
                deck_state.is_playing = false;
                deck_state.playback_start_time = None;
                let final_time_val = deck_state.duration.as_secs_f64();
                deck_state.paused_position = Some(Duration::from_secs_f64(final_time_val));
                emit_status_update_event(app_handle, deck_id, false);
            }

            // Emit time tick if playing or just ended
            if deck_state.is_playing || has_ended {
                emit_tick_event(app_handle, deck_id, current_time);
            }

            // Log if PLL changed pitch but UI wasn't updated due to thresholding
            if pitch_to_apply_to_engine.is_some() && !ui_pitch_updated_this_cycle && (final_engine_pitch - pitch_previously_sent_to_ui).abs() > 1e-6 {
                log::trace!(
                    "UI_PITCH_THROTTLE: Minor PLL pitch for {}. Engine: {:.6}. UI not updated (Prev UI: {:.6})",
                    deck_id, final_engine_pitch, pitch_previously_sent_to_ui
                );
            }
        } else {
            log::warn!("process_time_slice_updates: Deck state for '{}' became unavailable after potential pitch update call.", deck_id);
        }
    }
} 