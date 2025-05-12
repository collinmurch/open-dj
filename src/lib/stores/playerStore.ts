import { writable } from 'svelte/store';
import type { PlayerState } from '$lib/types';
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

    let unlistenUpdate: UnlistenFn | null = null;
    let unlistenError: UnlistenFn | null = null;
    let unlistenTick: UnlistenFn | null = null;

    async function setupListeners() {
        if (unlistenUpdate) unlistenUpdate();
        if (unlistenError) unlistenError();
        if (unlistenTick) unlistenTick();

        unlistenUpdate = await listen<{
            deckId: string;
            state: {
                isPlaying: boolean;
                isLoading: boolean;
                currentTime: number;
                duration: number | null;
                error: string | null;
                cuePointSeconds: number | null;
                pitchRate: number | null;
                isSyncActive: boolean;
                isMaster: boolean;
            };
        }>("playback://update", (event) => {
            if (event.payload.deckId === deckId) {
                const rustState = event.payload.state;
                // console.log(`[Store ${deckId}] RX Payload: isPlaying=${rustState.isPlaying}, isLoading=${rustState.isLoading}, time=${rustState.currentTime.toFixed(3)}, duration=${rustState.duration}, sync=${rustState.isSyncActive}, master=${rustState.isMaster}, pitch=${rustState.pitchRate?.toFixed(4)}`);

                update(s => {
                    const newDuration = rustState.duration !== null ? rustState.duration : s.duration;
                    const newCuePointTime = rustState.cuePointSeconds !== null ? rustState.cuePointSeconds : s.cuePointTime;
                    const newPitchRate = rustState.pitchRate !== null ? rustState.pitchRate : s.pitchRate;

                    // --- Robust Comparison ---
                    const TIME_TOLERANCE = 1e-5; // 0.01 ms
                    const PITCH_TOLERANCE = 1e-6;

                    const hasChanged =
                        s.isPlaying !== rustState.isPlaying ||
                        s.isLoading !== rustState.isLoading ||
                        Math.abs(s.currentTime - rustState.currentTime) > TIME_TOLERANCE ||
                        Math.abs(s.duration - newDuration) > TIME_TOLERANCE || // Compare calculated newDuration
                        s.error !== rustState.error ||
                        // Handle null comparison for cuePointTime
                        (s.cuePointTime === null ? newCuePointTime !== null : (newCuePointTime === null || Math.abs(s.cuePointTime - newCuePointTime) > TIME_TOLERANCE)) ||
                        s.isSyncActive !== rustState.isSyncActive ||
                        s.isMaster !== rustState.isMaster ||
                        // Handle null comparison for pitchRate
                        (s.pitchRate === null ? newPitchRate !== null : (newPitchRate === null || Math.abs(s.pitchRate - newPitchRate) > PITCH_TOLERANCE));
                    // --- End Robust Comparison ---

                    if (hasChanged) {
                        // console.log(`[Store ${deckId}] State Change Detected.`);
                        return { // Return the new state object ONLY if changed
                            ...s,
                            isPlaying: rustState.isPlaying,
                            isLoading: rustState.isLoading,
                            currentTime: rustState.currentTime,
                            duration: newDuration,
                            error: rustState.error,
                            cuePointTime: newCuePointTime,
                            isSyncActive: rustState.isSyncActive,
                            isMaster: rustState.isMaster,
                            pitchRate: newPitchRate,
                        };
                    } else {
                        // console.log(`[Store ${deckId}] State Unchanged. Skipping update.`);
                        return s; // Return the existing state if no change detected
                    }
                });
            }
        });

        unlistenTick = await listen<{
            deckId: string;
            currentTime: number;
        }>("playback://tick", (event) => {
            if (event.payload.deckId === deckId) {
                const newCurrentTime = event.payload.currentTime;
                update(s => ({
                    ...s,
                    currentTime: newCurrentTime,
                    isLoading: s.isLoading,
                    error: s.error,
                }));
            }
        });

        unlistenError = await listen<{ deckId: string; error: string }>(
            "playback://error",
            (event) => {
                if (event.payload.deckId === deckId) {
                    console.error(
                        `[Store ${deckId}] Received error event:`,
                        event.payload.error,
                    );
                    update((s) => ({ ...s, error: event.payload.error, isLoading: false }));
                }
            },
        );

        // console.log(`[Store ${deckId}] Event listeners attached.`);
    }

    async function initialize() {
        // console.log(`[Store ${deckId}] Initializing player via Rust...`);
        try {
            await invoke("init_player", { deckId });
            await setupListeners();
            // console.log(`[Store ${deckId}] Initialized successfully.`);
        } catch (err) {
            const errorMsg = `Initialization failed: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function loadTrack(path: string, originalBpm?: number | null, firstBeatSec?: number | null) {
        // console.log(`[Store ${deckId}] Loading track: ${path}, BPM: ${originalBpm}, FirstBeat: ${firstBeatSec}`);
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
        // console.log(`[Store ${deckId}] Playing track...`);
        try {
            await invoke("play_track", { deckId });
        } catch (err) {
            const errorMsg = `Failed to play track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, isPlaying: false, error: errorMsg }));
        }
    }

    async function pause() {
        // console.log(`[Store ${deckId}] Pausing track...`);
        try {
            await invoke("pause_track", { deckId });
        } catch (err) {
            const errorMsg = `Failed to pause track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function seek(positionSeconds: number) {
        // console.log(`[Store ${deckId}] Seeking to ${positionSeconds}s...`);
        try {
            await invoke("seek_track", { deckId, positionSeconds });
        } catch (err) {
            const errorMsg = `Failed to seek track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function setVolume(level: number) {
        // console.log(`[Store ${deckId}] Setting volume to ${level} via set_fader_level...`);
        try {
            await invoke("set_fader_level", { deckId, level: level });
        } catch (err) {
            const errorMsg = `Failed to set volume (invoking set_fader_level): ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
        }
    }

    async function setCuePoint(positionSeconds: number) {
        // console.log(`[Store ${deckId}] Setting cue point to ${positionSeconds}s...`);
        try {
            await invoke("set_cue_point", { deckId, positionSeconds });
        } catch (err) {
            const errorMsg = `Failed to set cue point: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function setPitchRate(rate: number) {
        try {
            await invoke("set_pitch_rate", { deckId, rate });
        } catch (err) {
            const errorMsg = `Failed to set pitch rate: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
        }
    }

    function cleanup() {
        // console.log(`[Store ${deckId}] Cleaning up listeners...`);
        if (unlistenUpdate) unlistenUpdate();
        if (unlistenError) unlistenError();
        if (unlistenTick) unlistenTick();
        unlistenUpdate = null;
        unlistenError = null;
        unlistenTick = null;
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