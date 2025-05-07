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
    };
    const { subscribe, set, update } = writable<PlayerState>(initialState);

    let unlistenUpdate: UnlistenFn | null = null;
    let unlistenError: UnlistenFn | null = null;

    async function setupListeners() {
        if (unlistenUpdate) unlistenUpdate();
        if (unlistenError) unlistenError();

        unlistenUpdate = await listen<{
            deckId: string;
            state: PlayerState;
        }>("playback://update", (event) => {
            if (event.payload.deckId === deckId) set(event.payload.state);
        });

        unlistenError = await listen<{ deckId: string; error: string }>(
            "playback://error",
            (event) => {
                if (event.payload.deckId === deckId) {
                    console.error(
                        `[Store ${deckId}] Received error:`,
                        event.payload.error,
                    );
                    update((s) => ({ ...s, error: event.payload.error }));
                }
            },
        );

        console.log(`[Store ${deckId}] Event listeners attached.`);
    }

    async function initialize() {
        console.log(`[Store ${deckId}] Initializing player via Rust...`);
        try {
            await invoke("init_player", { deckId });
            await setupListeners();
            console.log(`[Store ${deckId}] Initialized successfully.`);
        } catch (err) {
            const errorMsg = `Initialization failed: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function loadTrack(path: string) {
        console.log(`[Store ${deckId}] Loading track: ${path}`);
        update((s) => ({
            ...initialState,
            isLoading: true,
        }));
        try {
            await invoke("load_track", { deckId, path });
        } catch (err) {
            const errorMsg = `Failed to load track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, isLoading: false, error: errorMsg }));
        }
    }

    async function play() {
        console.log(`[Store ${deckId}] Playing track...`);
        try {
            await invoke("play_track", { deckId });
        } catch (err) {
            const errorMsg = `Failed to play track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, isPlaying: false, error: errorMsg }));
        }
    }

    async function pause() {
        console.log(`[Store ${deckId}] Pausing track...`);
        try {
            await invoke("pause_track", { deckId });
        } catch (err) {
            const errorMsg = `Failed to pause track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function seek(positionSeconds: number) {
        console.log(`[Store ${deckId}] Seeking to ${positionSeconds}s...`);
        try {
            await invoke("seek_track", { deckId, positionSeconds });
        } catch (err) {
            const errorMsg = `Failed to seek track: ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            update((s) => ({ ...s, error: errorMsg }));
        }
    }

    async function setVolume(level: number) {
        console.log(`[Store ${deckId}] Setting volume to ${level} via set_fader_level...`);
        try {
            await invoke("set_fader_level", { deckId, level: level });
            // No state update needed here, volume is fire-and-forget for now
        } catch (err) {
            const errorMsg = `Failed to set volume (invoking set_fader_level): ${err}`;
            console.error(`[Store ${deckId}]`, errorMsg);
            // Optionally update state with error if needed
            // update((s) => ({ ...s, error: errorMsg }));
        }
    }

    function cleanup() {
        console.log(`[Store ${deckId}] Cleaning up listeners...`);
        if (unlistenUpdate) unlistenUpdate();
        if (unlistenError) unlistenError();
        unlistenUpdate = null;
        unlistenError = null;
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
        cleanup,
        deckId,
    };
}

export type PlayerStore = ReturnType<typeof createPlayerStore>; 