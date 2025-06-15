import type { EqParams, TrackInfo, VolumeAnalysis } from '$lib/types';
import { invoke } from '@tauri-apps/api/core';
import { derived, writable } from 'svelte/store';

export interface DeckState {
    // Core track data
    filePath: string | null;
    volumeAnalysis: VolumeAnalysis | null;
    isWaveformLoading: boolean;

    // Audio control parameters
    faderLevel: number;
    eqParams: EqParams;
    uiSliderPitchRate: number;
}

function createDeckStore(deckId: 'A' | 'B') {
    const initialState: DeckState = {
        filePath: null,
        volumeAnalysis: null,
        isWaveformLoading: false,
        faderLevel: 1.0,
        eqParams: {
            lowGainDb: 0.0,
            midGainDb: 0.0,
            highGainDb: 0.0,
        },
        uiSliderPitchRate: 1.0,
    };

    const { subscribe, set, update } = writable<DeckState>(initialState);

    // Create the store object first
    const store = { subscribe, set, update };


    // Derived store for track loaded status
    const isTrackLoaded = derived(
        store,
        ($deckState) => !!$deckState.filePath
    );


    // Actions
    function setFilePath(path: string | null) {
        update(state => ({ ...state, filePath: path }));
    }

    function setVolumeAnalysis(analysis: VolumeAnalysis | null) {
        update(state => ({ ...state, volumeAnalysis: analysis }));
    }

    function setWaveformLoading(loading: boolean) {
        update(state => ({ ...state, isWaveformLoading: loading }));
    }

    function setFaderLevel(level: number) {
        update(state => ({ ...state, faderLevel: level }));
    }

    function setEqParams(params: EqParams) {
        update(state => ({ ...state, eqParams: params }));
    }

    function setUiSliderPitchRate(rate: number) {
        update(state => ({ ...state, uiSliderPitchRate: rate }));
    }

    function resetEq() {
        setEqParams({
            lowGainDb: 0.0,
            midGainDb: 0.0,
            highGainDb: 0.0,
        });
    }

    async function loadTrackFromLibrary(track: TrackInfo) {
        const currentState = get({ subscribe });

        // Skip if same track is already loaded
        if (currentState.filePath === track.path) {
            console.log(`[DeckStore ${deckId}] Track ${track.path} is already loaded. Skipping reload.`);
            return;
        }

        // Batch the initial updates to prevent multiple reactive cascades
        update(state => ({
            ...state,
            filePath: track.path,
            volumeAnalysis: null,
            isWaveformLoading: true
        }));

        try {
            console.log(`[DeckStore ${deckId}] Loading waveform on-demand: ${track.path}`);

            const result = await invoke<VolumeAnalysis>(
                'get_track_volume_analysis',
                { path: track.path }
            );

            // Batch the final updates
            update(state => ({
                ...state,
                volumeAnalysis: result,
                isWaveformLoading: false
            }));
        } catch (error) {
            console.error(`[DeckStore ${deckId}] Error loading volume analysis: ${track.path}`, error);
            // Batch the error state updates
            update(state => ({
                ...state,
                volumeAnalysis: null,
                isWaveformLoading: false
            }));
        }
    }

    function clearTrack() {
        set(initialState);
    }

    // Helper function to get current state synchronously
    function get<T>(store: { subscribe: (fn: (value: T) => void) => () => void }): T {
        let value: T;
        const unsubscribe = store.subscribe((v: T) => { value = v; });
        unsubscribe();
        return value!;
    }

    return {
        subscribe: store.subscribe,

        // Derived stores
        isTrackLoaded,

        // Actions
        setFilePath,
        setVolumeAnalysis,
        setWaveformLoading,
        setFaderLevel,
        setEqParams,
        setUiSliderPitchRate,
        resetEq,
        loadTrackFromLibrary,
        clearTrack,

        // Getters for immediate access
        get: () => get(store),
        deckId,
    };
}

export type DeckStore = ReturnType<typeof createDeckStore>;

// Create and export deck stores
export const deckAStore = createDeckStore('A');
export const deckBStore = createDeckStore('B');

// Helper function to get deck store by ID
export function getDeckStore(deckId: 'A' | 'B'): DeckStore {
    return deckId === 'A' ? deckAStore : deckBStore;
}
