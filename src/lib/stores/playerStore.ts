import { writable } from 'svelte/store';
import type { PlayerState } from '$lib/types';
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Define new payload types matching Rust structs for clarity, though listen() is generic
interface PlaybackPitchTickPayload {
    deckId: string;
    pitchRate: number;
}

interface PlaybackStatusPayload {
    deckId: string;
    isPlaying: boolean;
}

interface PlaybackSyncStatusPayload {
    deckId: string;
    isSyncActive: boolean;
    isMaster: boolean;
}

interface PlaybackLoadPayload {
    deckId: string;
    duration: number;
    cuePointSeconds: number | null;
    originalBpm: number | null;
    firstBeatSec: number | null;
}

interface PlaybackTickPayload {
    deckId: string;
    currentTime: number;
}

interface PlaybackErrorPayload {
    deckId: string;
    error: string;
}

export function createPlayerStore(deckId: string) {
    const initialState: PlayerState = {
        currentTime: 0,
        duration: 0,
        isPlaying: false,
        isLoading: false,
        error: null,
        cuePointTime: null,
        isSyncActive: false,
        isMaster: false,
        pitchRate: 1.0,
    };
    const { subscribe, set, update } = writable<PlayerState>(initialState);

    let unlistenError: UnlistenFn | null = null;
    let unlistenTick: UnlistenFn | null = null;
    let unlistenPitchTick: UnlistenFn | null = null;
    let unlistenStatusUpdate: UnlistenFn | null = null;
    let unlistenSyncStatusUpdate: UnlistenFn | null = null;
    let unlistenLoadUpdate: UnlistenFn | null = null;

    async function setupListeners() {
        if (unlistenError) unlistenError();
        if (unlistenTick) unlistenTick();
        if (unlistenPitchTick) unlistenPitchTick();
        if (unlistenStatusUpdate) unlistenStatusUpdate();
        if (unlistenSyncStatusUpdate) unlistenSyncStatusUpdate();
        if (unlistenLoadUpdate) unlistenLoadUpdate();

        unlistenLoadUpdate = await listen<PlaybackLoadPayload>(
            "playback://load-update",
            (event) => {
                if (event.payload.deckId === deckId) {
                    const { duration, cuePointSeconds, originalBpm, firstBeatSec } = event.payload;
                    update(s => ({
                        ...initialState,
                        duration: duration,
                        cuePointTime: cuePointSeconds,
                        isLoading: false,
                        error: null,
                    }));
                }
            }
        );

        unlistenStatusUpdate = await listen<PlaybackStatusPayload>(
            "playback://status-update",
            (event) => {
                if (event.payload.deckId === deckId) {
                    update(s => ({
                        ...s,
                        isPlaying: event.payload.isPlaying,
                        currentTime: (!event.payload.isPlaying && s.duration > 0 && Math.abs(s.currentTime - s.duration) < 0.1)
                            ? s.duration
                            : s.currentTime,
                    }));
                }
            }
        );

        unlistenTick = await listen<PlaybackTickPayload>("playback://tick", (event) => {
            if (event.payload.deckId === deckId) {
                update(s => ({
                    ...s,
                    currentTime: event.payload.currentTime,
                }));
            }
        });

        unlistenPitchTick = await listen<PlaybackPitchTickPayload>(
            "playback://pitch-tick",
            (event) => {
                if (event.payload.deckId === deckId) {
                    update(s => ({
                        ...s,
                        pitchRate: event.payload.pitchRate,
                    }));
                }
            }
        );

        unlistenSyncStatusUpdate = await listen<PlaybackSyncStatusPayload>(
            "playback://sync-status-update",
            (event) => {
                if (event.payload.deckId === deckId) {
                    update(s => ({
                        ...s,
                        isSyncActive: event.payload.isSyncActive,
                        isMaster: event.payload.isMaster,
                    }));
                }
            }
        );

        unlistenError = await listen<PlaybackErrorPayload>(
            "playback://error",
            (event) => {
                if (event.payload.deckId === deckId) {
                    console.error(
                        `[Store ${deckId}] Received error event:`,
                        event.payload.error,
                    );
                    update((s) => ({
                        ...s,
                        error: event.payload.error,
                        isLoading: false,
                        isPlaying: false,
                    }));
                }
            },
        );
    }

    async function initialize() {
        try {
            await invoke("init_player", { deckId });
            await setupListeners();
        } catch (err) {
            const errorMsg = `Initialization failed: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function loadTrack(path: string, originalBpm?: number | null, firstBeatSec?: number | null) {
        set({
            ...initialState,
            isLoading: true,
        });
        try {
            await invoke("load_track", {
                deckId,
                path,
                originalBpm: originalBpm === null ? undefined : originalBpm,
                firstBeatSec: firstBeatSec === null ? undefined : firstBeatSec,
            });
        } catch (err) {
            const errorMsg = `Failed to load track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, isLoading: false, error: errorMsg }));
        }
    }

    async function play() {
        update(s => ({ ...s, isPlaying: true }));
        try {
            await invoke("play_track", { deckId });
        } catch (err) {
            const errorMsg = `Failed to play track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, isPlaying: false, error: errorMsg }));
        }
    }

    async function pause() {
        update(s => ({ ...s, isPlaying: false }));
        try {
            await invoke("pause_track", { deckId });
        } catch (err) {
            const errorMsg = `Failed to pause track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, isPlaying: true, error: errorMsg }));
        }
    }

    async function seek(positionSeconds: number) {
        try {
            await invoke("seek_track", { deckId, positionSeconds });
        } catch (err) {
            const errorMsg = `Failed to seek track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function setVolume(level: number) {
        try {
            await invoke("set_fader_level", { deckId, level: level });
        } catch (err) {
            const errorMsg = `Failed to set volume (invoking set_fader_level): ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
        }
    }

    async function setCuePoint(positionSeconds: number) {
        update(s => ({ ...s, cuePointTime: positionSeconds }));
        try {
            await invoke("set_cue_point", { deckId, positionSeconds });
        } catch (err) {
            const errorMsg = `Failed to set cue point: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function setPitchRate(rate: number) {
        update(s => ({ ...s, pitchRate: rate }));
        try {
            await invoke("set_pitch_rate", { deckId, rate });
        } catch (err) {
            const errorMsg = `Failed to set pitch rate: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    function cleanup() {
        if (unlistenError) unlistenError();
        if (unlistenTick) unlistenTick();
        if (unlistenPitchTick) unlistenPitchTick();
        if (unlistenStatusUpdate) unlistenStatusUpdate();
        if (unlistenSyncStatusUpdate) unlistenSyncStatusUpdate();
        if (unlistenLoadUpdate) unlistenLoadUpdate();

        unlistenError = null;
        unlistenTick = null;
        unlistenPitchTick = null;
        unlistenStatusUpdate = null;
        unlistenSyncStatusUpdate = null;
        unlistenLoadUpdate = null;
        set(initialState);
    }

    initialize();

    return {
        subscribe,
        loadTrack,
        play,
        pause,
        seek,
        setVolume,
        setCuePoint,
        cleanup,
        deckId,
        setPitchRate,
    };
}

export type PlayerStore = ReturnType<typeof createPlayerStore>; 