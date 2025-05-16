use std::time::Duration;
use super::state::AudioThreadDeckState;

// Added imports for the new function
use std::collections::HashMap;
use tauri::{AppHandle, Runtime};
use crate::audio::config;
use super::events::{emit_status_update_event, emit_tick_event, emit_pitch_tick_event};
use super::sync::{self, MAX_PLL_INTEGRAL_ERROR};

pub(crate) fn get_current_playback_time_secs(deck_state: &AudioThreadDeckState) -> f64 {
    let is_playing = *deck_state.is_playing.lock().unwrap();
    let sample_rate = deck_state.sample_rate;

    if sample_rate == 0.0 { // Avoid division by zero if track not loaded properly
        return 0.0;
    }

    if is_playing {
        let current_idx = *deck_state.current_sample_index.lock().unwrap();
        // In Phase 1, pitch rate is 1.0, so no multiplication needed here.
        // This will be adjusted in Phase 2 for resampling.
        let current_time = current_idx as f64 / sample_rate as f64;
        return current_time.min(deck_state.duration.as_secs_f64()); // Ensure time doesn't exceed duration
    } else if let Some(paused_idx) = *deck_state.paused_position_samples.lock().unwrap() {
        let paused_time = paused_idx as f64 / sample_rate as f64;
        return paused_time.min(deck_state.duration.as_secs_f64());
    }
    0.0
}

// New function: process_time_slice_updates (moved from handlers.rs)
pub(crate) fn process_time_slice_updates<R: Runtime>(
    local_states: &mut HashMap<String, AudioThreadDeckState>,
    app_handle: &AppHandle<R>,
) {
    let mut decks_to_process: HashMap<String, (f64, bool, bool)> = HashMap::new(); // deck_id -> (current_time, is_logically_playing, is_master_deck)

    // First pass: Collect current times and playing states
    for (deck_id, deck_state) in local_states.iter() {
        let current_time_secs = get_current_playback_time_secs(deck_state);
        let is_logically_playing = *deck_state.is_playing.lock().unwrap(); // Current status from audio callback
        let is_master_deck = deck_state.is_master;

        // A deck needs processing if it's playing, or if it's a master (for sync purposes later)
        if is_logically_playing || is_master_deck {
            // The track has effectively ended if it's not playing anymore AND its current time is at or beyond duration.
            // The primary end-of-track is handled by the data callback setting is_playing to false.
            // This `has_ended_in_ui_timeline` is more for UI and ensuring the final tick goes out correctly if needed.
            let has_ended_in_ui_timeline = !is_logically_playing && deck_state.duration > Duration::ZERO && 
                                          current_time_secs >= deck_state.duration.as_secs_f64() - 0.01; // Small epsilon

            decks_to_process.insert(deck_id.clone(), (current_time_secs, is_logically_playing, has_ended_in_ui_timeline));
        }
    }

    // --- PLL Logic Placeholder (to be fully functional in Phase 5) ---
    // Convert decks_to_process to the format expected by calculate_pll_pitch_updates if needed
    // For now, let's assume it expects current_time and a simple ended flag.
    let decks_for_pll: HashMap<String, (f64, bool)> = decks_to_process.iter()
        .map(|(id, (time, _, ended_ui))| (id.clone(), (*time, *ended_ui)))
        .collect();
    let slave_pitch_info_map = sync::calculate_pll_pitch_updates(local_states, &decks_for_pll);
    // --- End PLL Logic Placeholder ---

    for (deck_id_str, (current_time, is_logically_playing, has_ended_in_ui_timeline)) in decks_to_process {
        let deck_id = deck_id_str.as_str();

        // --- Apply PLL Pitch Adjustments (Placeholder for Phase 1) ---
        let _pitch_to_apply_to_engine: Option<f32> = None;
        if let Some(deck_state_for_pll_calc) = local_states.get_mut(deck_id) { 
            if deck_state_for_pll_calc.is_sync_active { // Only consider for active slaves
                if let Some(&(_proportional_correction, signed_error)) = slave_pitch_info_map.get(deck_id) {
                    // ... (Full PLL logic from original file, but for Phase 1 it won't change actual pitch) ...
                    // For Phase 1, we do not call handlers::audio_thread_handle_set_pitch_rate from here
                    // as actual pitch change is deferred.
                    // We can still log the calculated corrections for observation.
                    let dt = Duration::from_millis(config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS).as_secs_f32();
                    deck_state_for_pll_calc.pll_integral_error += signed_error * dt; // Update integral for later phases
                    deck_state_for_pll_calc.pll_integral_error = deck_state_for_pll_calc.pll_integral_error.clamp(-MAX_PLL_INTEGRAL_ERROR, MAX_PLL_INTEGRAL_ERROR);
                    // ... logging from original ...
                }
            }
        }
        // --- End Apply PLL Pitch Adjustments ---


        if let Some(deck_state) = local_states.get_mut(deck_id) {
            let final_engine_pitch = *deck_state.current_pitch_rate.lock().unwrap(); // For Phase 1, this is 1.0

            // UI Pitch update (will just reflect 1.0 or manual setting in Phase 1)
            const UI_PITCH_UPDATE_THRESHOLD: f32 = 0.0005;
            let pitch_previously_sent_to_ui = deck_state.last_ui_pitch_rate.unwrap_or(deck_state.manual_pitch_rate);
            if (final_engine_pitch - pitch_previously_sent_to_ui).abs() > UI_PITCH_UPDATE_THRESHOLD {
                emit_pitch_tick_event(app_handle, deck_id, final_engine_pitch);
                deck_state.last_ui_pitch_rate = Some(final_engine_pitch);
            }

            // Handle track ending state propagation for UI
            // The audio callback sets is_playing to false. This loop picks that up.
            if has_ended_in_ui_timeline && !is_logically_playing {
                // If it was playing before this time slice but now is_logically_playing is false due to callback
                // and we are at/past duration, ensure paused state is set correctly at the very end.
                if deck_state.sample_rate > 0.0 { // Guard against unloaded track
                    let end_sample_idx = deck_state.decoded_samples.len().saturating_sub(1);
                    *deck_state.current_sample_index.lock().unwrap() = end_sample_idx;
                    *deck_state.paused_position_samples.lock().unwrap() = Some(end_sample_idx);
                }
                // Emit status update if it hasn't been emitted by a direct pause command
                emit_status_update_event(app_handle, deck_id, false); 
            }

            // Emit time tick if it was playing during this slice, or if it just ended and UI needs final tick.
            if is_logically_playing || has_ended_in_ui_timeline {
                emit_tick_event(app_handle, deck_id, current_time);
            }
        }
    }
} 