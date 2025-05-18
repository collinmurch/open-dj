use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

// --- Event Payloads for Frontend ---
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTickEventPayload {
    pub deck_id: String,
    pub current_time: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackErrorEventPayload {
    pub deck_id: String,
    pub error: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPitchTickEventPayload {
    pub deck_id: String,
    pub pitch_rate: f32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatusEventPayload {
    pub deck_id: String,
    pub is_playing: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSyncStatusEventPayload {
    pub deck_id: String,
    #[serde(rename = "isSyncActive")]
    pub is_sync_active: bool,
    #[serde(rename = "isMaster")]
    pub is_master: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackLoadEventPayload {
    pub deck_id: String,
    pub duration: f64,
    pub cue_point_seconds: Option<f64>,
    pub original_bpm: Option<f32>,
    pub first_beat_sec: Option<f32>,
}

// --- Event Emitter Helpers ---

pub(crate) fn emit_tick_event<R: Runtime>(app_handle: &AppHandle<R>, deck_id: &str, current_time: f64) {
    let event_payload = PlaybackTickEventPayload {
        deck_id: deck_id.to_string(),
        current_time,
    };
    if let Err(e) = app_handle.emit("playback://tick", event_payload) {
        log::warn!("Failed to emit playback://tick for {}: {}", deck_id, e);
    }
}

pub(crate) fn emit_error_event<R: Runtime>(app_handle: &AppHandle<R>, deck_id: &str, error_message: &str) {
    let payload = PlaybackErrorEventPayload {
        deck_id: deck_id.to_string(),
        error: error_message.to_string(),
    };
    if let Err(e) = app_handle.emit("playback://error", payload) {
        log::error!("Failed to emit playback://error for {}: {}", deck_id, e);
    }
}

pub(crate) fn emit_pitch_tick_event<R: Runtime>(
    app_handle: &AppHandle<R>,
    deck_id: &str,
    pitch_rate: f32,
) {
    let payload = PlaybackPitchTickEventPayload {
        deck_id: deck_id.to_string(),
        pitch_rate,
    };
    if let Err(e) = app_handle.emit("playback://pitch-tick", payload) {
        log::warn!(
            "Failed to emit playback://pitch-tick for {}: {}",
            deck_id,
            e
        );
    }
}

pub(crate) fn emit_status_update_event<R: Runtime>(
    app_handle: &AppHandle<R>,
    deck_id: &str,
    is_playing: bool,
) {
    let payload = PlaybackStatusEventPayload {
        deck_id: deck_id.to_string(),
        is_playing,
    };
    if let Err(e) = app_handle.emit("playback://status-update", payload) {
        log::warn!(
            "Failed to emit playback://status-update for {}: {}",
            deck_id,
            e
        );
    }
}

pub(crate) fn emit_sync_status_update_event<R: Runtime>(
    app_handle: &AppHandle<R>,
    deck_id: &str,
    is_sync_active: bool,
    is_master: bool,
) {
    let payload = PlaybackSyncStatusEventPayload {
        deck_id: deck_id.to_string(),
        is_sync_active,
        is_master,
    };
    if let Err(e) = app_handle.emit("playback://sync-status-update", payload) {
        log::warn!(
            "Failed to emit playback://sync-status-update for {}: {}",
            deck_id,
            e
        );
    }
}

pub(crate) fn emit_load_update_event<R: Runtime>(
    app_handle: &AppHandle<R>,
    deck_id: &str,
    duration: f64,
    cue_point_seconds: Option<f64>,
    original_bpm: Option<f32>,
    first_beat_sec: Option<f32>,
) {
    let payload = PlaybackLoadEventPayload {
        deck_id: deck_id.to_string(),
        duration,
        cue_point_seconds,
        original_bpm,
        first_beat_sec,
    };
    if let Err(e) = app_handle.emit("playback://load-update", payload) {
        log::warn!(
            "Failed to emit playback://load-update for {}: {}",
            deck_id,
            e
        );
    }
} 