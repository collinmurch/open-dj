use std::time::Duration;
use super::state::AudioThreadDeckState;

// Added imports for the new function
use std::collections::HashMap;
use tauri::{AppHandle, Runtime};
use crate::audio::config;
use super::events::{emit_status_update_event, emit_tick_event, emit_pitch_tick_event};
use super::sync;

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

    for (deck_id, deck_state) in local_states.iter_mut() {
        if deck_state.is_playing && !deck_state.sink.is_paused() && !deck_state.sink.empty() {
            let current_time_secs = get_current_playback_time_secs(deck_state); // Calls local function
            let end_buffer = Duration::from_millis(config::AUDIO_THREAD_TIME_UPDATE_INTERVAL_MS + 10);
            let has_ended = deck_state.duration > Duration::ZERO
                && (Duration::from_secs_f64(current_time_secs) + end_buffer >= deck_state.duration);

            if has_ended {
                log::info!("Audio Thread: Track finished for '{}' based on time update.", deck_id);
                deck_state.sink.pause();
                deck_state.is_playing = false;
                deck_state.playback_start_time = None;
                let final_time = deck_state.duration.as_secs_f64();
                deck_state.paused_position = Some(Duration::from_secs_f64(final_time));
                decks_to_update.insert(deck_id.clone(), (final_time, true));
            } else {
                decks_to_update.insert(deck_id.clone(), (current_time_secs, false));
            }
        } else if deck_state.is_master { 
            if !decks_to_update.contains_key(deck_id) { 
                 let current_time_secs = get_current_playback_time_secs(deck_state); // Calls local function
                 decks_to_update.insert(deck_id.clone(), (current_time_secs, false)); 
            }
        }
    }

    let slave_pitch_updates = sync::calculate_pll_pitch_updates(local_states, &decks_to_update);

    for (deck_id, (current_time, has_ended)) in decks_to_update {
        if let Some(deck_state) = local_states.get_mut(&deck_id) {
            let mut pitch_changed_by_pll = false;
            let mut final_emitted_pitch = *deck_state.current_pitch_rate.lock().unwrap();

            if let Some(&new_pitch_from_pll) = slave_pitch_updates.get(&deck_id) {
                let mut rate_lock = deck_state.current_pitch_rate.lock().unwrap();
                if (*rate_lock - new_pitch_from_pll).abs() > 1e-5 { 
                    log::info!("PLL: Applying pitch update for {}. Old: {:.6}, New: {:.6}", deck_id, *rate_lock, new_pitch_from_pll);
                    *rate_lock = new_pitch_from_pll;
                    deck_state.sink.set_speed(new_pitch_from_pll);
                    final_emitted_pitch = new_pitch_from_pll;
                    pitch_changed_by_pll = true;
                }
            }

            if has_ended { emit_status_update_event(app_handle, &deck_id, false); }
            if deck_state.is_playing || has_ended { emit_tick_event(app_handle, &deck_id, current_time); }
            if pitch_changed_by_pll {
                log::info!("PLL: Emitting pitch-tick for {} after update. Rate: {:.6}", deck_id, final_emitted_pitch);
                emit_pitch_tick_event(app_handle, &deck_id, final_emitted_pitch);
            }
        }
    }
} 