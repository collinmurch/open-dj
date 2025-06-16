<script lang="ts">
    import { getDeckStore } from "$lib/stores/deckStore";
    import { libraryStore } from "$lib/stores/libraryStore";
    import type { PlayerStore } from "$lib/stores/playerStore";
    import { syncStore } from "$lib/stores/syncStore";
    import type { EqParams, TrackInfo } from "$lib/types";
    import { invoke } from "@tauri-apps/api/core";
    import { AUDIO_CONSTANTS } from "$lib/constants";
    import DeckControls from "./DeckControls.svelte";

    let {
        deckId,
        cueAudioDeck = $bindable(),
        crossfaderValue = 0.5,
        playerStore,
        onLoadTrack,
    }: {
        deckId: 'A' | 'B';
        cueAudioDeck?: 'A' | 'B' | null;
        crossfaderValue?: number;
        playerStore: PlayerStore;
        onLoadTrack: (deckId: 'A' | 'B') => Promise<void>;
    } = $props();

    // Get deck store for this deck
    const deckStore = getDeckStore(deckId);

    // Reactive deck state
    const deckState = $derived($deckStore);
    const playerStoreState = $derived($playerStore);
    
    // Track info lookup using runes (like in main page)
    const trackInfo = $derived.by(() => {
        const deckState = $deckStore;
        const libraryState = $libraryStore;
        if (!deckState.filePath) return undefined;
        return libraryState.audioFiles.find(track => track.path === deckState.filePath);
    });

    // Computed BPM with pitch rate
    const currentBpm = $derived.by(() => {
        const bpm = trackInfo?.metadata?.bpm;
        const rate = playerStoreState.pitchRate ?? 1.0;
        return bpm && rate ? bpm * rate : null;
    });

    // Check if this deck is the cue audio source
    const isCueAudioActive = $derived(cueAudioDeck === deckId);

    // --- Effects for integrating deck state with backend ---

    // Effect to update backend fader level based on individual fader and crossfader
    let lastFaderLevel = 0;
    let lastCrossfaderEffect = $state<number | null>(null);
    $effect(() => {
        const individualLevel = deckState.faderLevel;
        const crossfadeEffect = deckId === 'A'
            ? 1.0 - crossfaderValue  // A is attenuated as crossfader goes right
            : crossfaderValue;       // B is attenuated as crossfader goes left

        const finalLevel = Math.max(0.0, Math.min(1.0, individualLevel * crossfadeEffect));

        // Check if crossfader effect changed significantly to reduce micro-movement updates
        const crossfaderChangedSignificantly = lastCrossfaderEffect === null || 
            Math.abs(crossfadeEffect - lastCrossfaderEffect) > AUDIO_CONSTANTS.FADER_LEVEL_CHANGE_THRESHOLD;

        // Only update if level changed significantly and deck is ready
        if (playerStoreState.duration > 0 && 
            (Math.abs(finalLevel - lastFaderLevel) > AUDIO_CONSTANTS.FADER_LEVEL_CHANGE_THRESHOLD || crossfaderChangedSignificantly)) {
            lastFaderLevel = finalLevel;
            lastCrossfaderEffect = crossfadeEffect;
            (async () => {
                try {
                    await invoke("set_fader_level", {
                        deckId,
                        level: finalLevel,
                    });
                } catch (err) {
                    console.error(`[DeckController ${deckId}] Error setting fader level:`, err);
                }
            })();
        }
    });

    // Effect to sync UI pitch rate with player store (prevent circular updates)
    let lastPlayerPitchRate = $state<number | null>(null);
    $effect(() => {
        const storeRate = playerStoreState.pitchRate;

        // Only update if player store rate actually changed
        if (storeRate !== lastPlayerPitchRate) {
            lastPlayerPitchRate = storeRate;
            if (storeRate !== null) {
                deckStore.setUiSliderPitchRate(storeRate);
            } else {
                deckStore.setUiSliderPitchRate(1.0);
            }
        }
    });

    // Effect to update sync store when player store sync flags change
    let lastSyncState = $state<{isSync: boolean, isMaster: boolean} | null>(null);
    $effect(() => {
        const isSync = playerStoreState.isSyncActive;
        const isMaster = playerStoreState.isMaster;
        
        // Only update sync store if sync state actually changed
        if (!lastSyncState || lastSyncState.isSync !== isSync || lastSyncState.isMaster !== isMaster) {
            lastSyncState = { isSync, isMaster };
            syncStore.updateDeckSyncFlags(deckId, isSync, isMaster);
        }
    });

    // Effect to update EQ parameters in backend
    let lastEqParams: EqParams | null = null;
    $effect(() => {
        const isDeckReady = playerStoreState.duration > 0 && !playerStoreState.isLoading;
        if (!isDeckReady) return;

        const paramsToSend = deckState.eqParams;

        // Only update if EQ params actually changed
        const paramsChanged = !lastEqParams ||
            lastEqParams.lowGainDb !== paramsToSend.lowGainDb ||
            lastEqParams.midGainDb !== paramsToSend.midGainDb ||
            lastEqParams.highGainDb !== paramsToSend.highGainDb;

        if (paramsChanged) {
            lastEqParams = { ...paramsToSend };
            (async () => {
                try {
                    await invoke("set_eq_params", {
                        deckId: deckId,
                        params: paramsToSend,
                    });
                } catch (err) {
                    console.error(`[DeckController ${deckId}] Failed to set EQ:`, err);
                }
            })();
        }
    });

    // --- Event Handlers ---

    function handlePitchChange(newRate: number) {
        const isSlave = playerStoreState.isSyncActive && !playerStoreState.isMaster;
        if (isSlave) return;

        deckStore.setUiSliderPitchRate(newRate);
        void playerStore.setPitchRate(newRate);
    }

    async function toggleCueAudio() {
        try {
            if (isCueAudioActive) {
                // Turn off cue audio for this deck
                cueAudioDeck = null;
                await invoke("set_cue_deck", { deckId: null });
            } else {
                // Turn on cue audio for this deck (turns off the other)
                cueAudioDeck = deckId;
                await invoke("set_cue_deck", { deckId });
            }
        } catch (error) {
            console.error(`[DeckController ${deckId}] Failed to toggle cue audio:`, error);
        }
    }

    function handleSeek(time: number) {
        playerStore.seek(time);
    }

    async function loadTrack() {
        await onLoadTrack(deckId);
    }

    // Load track to this deck from a TrackInfo object
    async function loadTrackFromInfo(track: TrackInfo) {
        const bpm = track.metadata?.bpm ?? null;
        const firstBeat = track.metadata?.firstBeatSec ?? null;

        // Update deck store
        await deckStore.loadTrackFromLibrary(track);

        // Load track in player store
        playerStore.loadTrack(track.path, bpm, firstBeat);
    }

    // Public methods that can be called by parent
    export const api = {
        loadTrackFromInfo,
        loadTrack,
        clearTrack: () => deckStore.clearTrack(),
        getDeckStore: () => deckStore,
        getPlayerStore: () => playerStore,
    };
</script>

<div class="deck-controller" class:deck-a={deckId === 'A'} class:deck-b={deckId === 'B'}>
    <DeckControls
        {deckId}
        filePath={deckState.filePath}
        playerStoreState={playerStoreState}
        playerActions={playerStore}
        eqParams={deckState.eqParams}
        faderLevel={deckState.faderLevel}
        pitchRate={deckState.uiSliderPitchRate}
        onPitchChange={handlePitchChange}
        onEqChange={(params) => deckStore.setEqParams(params)}
        onFaderChange={(level) => deckStore.setFaderLevel(level)}
        currentBpm={currentBpm}
        isCueAudioActive={isCueAudioActive}
        onToggleCueAudio={toggleCueAudio}
    />
</div>

<style>
    .deck-controller {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.5rem;
        border: 3px solid transparent;
        border-radius: 8px;
        padding: 0.25rem;
        transition: border-color 0.3s ease, background-color 0.3s ease;
        width: 100%;
        max-width: 800px;
        min-width: 700px;
    }

    .deck-controller.deck-a {
        border-color: var(--deck-a-border-light, hsl(255, 40%, 60%));
        background-color: var(--deck-a-deck-bg-light, hsl(255, 50%, 80%));
    }

    .deck-controller.deck-b {
        border-color: var(--deck-b-border-light, hsl(210, 15%, 88%));
        background-color: var(--deck-b-deck-bg-light, hsl(210, 30%, 99%));
    }


    @media (prefers-color-scheme: dark) {
        .deck-controller.deck-a {
            border-color: var(--deck-a-border-dark, hsl(260, 40%, 20%));
            background-color: var(--deck-a-deck-bg-dark, hsl(260, 30%, 10%));
        }

        .deck-controller.deck-b {
            border-color: var(--deck-b-border-dark, hsl(210, 15%, 30%));
            background-color: var(--deck-b-deck-bg-dark, hsl(210, 10%, 15%));
        }
    }
</style>
