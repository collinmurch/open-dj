<script lang="ts">
    import type { PlayerStore } from "$lib/stores/playerStore";
    import { syncStore, type SyncStatus } from "$lib/stores/syncStore";
    import type { EqParams, PlayerState } from "$lib/types";
    import { formatTime } from "$lib/utils/timeUtils";
    import { invoke } from "@tauri-apps/api/core";
    import { AUDIO_CONSTANTS, SYNC_CONSTANTS, EQ_CONSTANTS, TRIM_CONSTANTS, FADER_CONSTANTS } from "$lib/constants";
    import Slider from "./Slider.svelte";

    let {
        deckId,
        playerStoreState,
        playerActions,
        trimDb = 0.0,
        faderLevel = 1.0,
        eqParams = {
            lowGainDb: 0.0,
            midGainDb: 0.0,
            highGainDb: 0.0,
        } as EqParams,
        currentBpm = null as number | null,
        pitchRate = 1.0,
        onPitchChange,
        onEqChange,
        onFaderChange,
        isCueAudioActive = false,
        onToggleCueAudio,
    }: {
        filePath: string | null;
        deckId: string;
        playerStoreState: PlayerState;
        playerActions: Pick<
            PlayerStore,
            | "loadTrack"
            | "play"
            | "pause"
            | "seek"
            | "cleanup"
            | "setVolume"
            | "setCuePoint"
            | "setPitchRate"
        >;
        trimDb?: number;
        faderLevel?: number;
        eqParams?: EqParams;
        currentBpm?: number | null;
        pitchRate?: number;
        onPitchChange: (newRate: number) => void;
        onEqChange?: (params: EqParams) => void;
        onFaderChange?: (level: number) => void;
        isCueAudioActive?: boolean;
        onToggleCueAudio: () => void;
    } = $props();

    // --- Volume, Trim & EQ State (remains the same) ---
    let trimDebounceTimeout: number | undefined = undefined;
    let faderDebounceTimeout: number | undefined = undefined;
    let eqDebounceTimeout: number | undefined = undefined;

    // --- CUE State ---
    let isCueHeld = $state(false); // Track if cue button is currently held down
    let wasPausedAtCueWhenCuePressed = $state(false); // Flag for cue play logic

    // Derived state to check if playback is currently at the cue point
    const isAtCuePoint = $derived.by(() => {
        const cueTime = playerStoreState.cuePointTime;
        const currentTime = playerStoreState.currentTime;
        return cueTime !== null && Math.abs(currentTime - cueTime) < AUDIO_CONSTANTS.CUE_POINT_TOLERANCE_SECONDS;
    });

    // --- Sync State Access ---
    const syncButtonStatus = $derived.by((): SyncStatus => {
        if (playerStoreState.isMaster) return "master";
        if (playerStoreState.isSyncActive) return "synced";
        return "off";
    });

    // --- Effects ---

    // Effect for component cleanup (if needed for timeouts)
    $effect(() => {
        return () => {
            if (trimDebounceTimeout !== undefined)
                clearTimeout(trimDebounceTimeout);
            if (faderDebounceTimeout !== undefined)
                clearTimeout(faderDebounceTimeout);
            if (eqDebounceTimeout !== undefined)
                clearTimeout(eqDebounceTimeout);
            if (trimUpdateTimeout !== undefined)
                clearTimeout(trimUpdateTimeout);
        };
    });

    // Effect to update Trim Gain in Rust with debouncing
    let lastTrimDb = $state<number | null>(null);
    let trimUpdateTimeout: number | undefined = undefined;
    $effect(() => {
        const currentTrimDb = trimDb;
        
        // Skip micro-adjustments to prevent excessive calls
        if (lastTrimDb !== null && Math.abs(currentTrimDb - lastTrimDb) < 0.1) {
            return;
        }
        
        // Debounce the backend call to prevent excessive IPC during slider dragging
        clearTimeout(trimUpdateTimeout);
        trimUpdateTimeout = setTimeout(async () => {
            try {
                await invoke("set_trim_gain", {
                    deckId,
                    gainDb: currentTrimDb,
                });
                lastTrimDb = currentTrimDb;
            } catch (err) {
                console.error(
                    `[TrackPlayer ${deckId}] Error invoking set_trim_gain:`,
                    err,
                );
            }
        }, 16); // 60fps debouncing - smooth but not excessive
    });


    // Event handler for pitch slider changes from Slider's onchangeValue event
    function handlePitchSliderChange(newPitchValue: number) {
        // Use the new onPitchChange prop for immediate, responsive updates
        onPitchChange(newPitchValue);
    }

    // --- Event Handlers for Buttons (use playerActions props) ---
    async function handlePlayPause() {
        try {
            if (playerStoreState.isPlaying) {
                await playerActions.pause();
            } else {
                await playerActions.play();
            }
        } catch (err) {
            console.error(
                `[TrackPlayer ${deckId}] Error invoking ${playerStoreState.isPlaying ? 'pause' : 'play'} prop:`,
                err,
            );
        }
    }

    async function handleSeekBackward() {
        const currentTime = playerStoreState.currentTime;
        const duration = playerStoreState.duration;
        if (duration <= 0) return;
        const newTime = Math.max(0, currentTime - AUDIO_CONSTANTS.SEEK_AMOUNT_SECONDS);
        try {
            await playerActions.seek(newTime);
        } catch (err) {
            console.error(
                `[TrackPlayer ${deckId}] Error invoking seek backward prop:`,
                err,
            );
        }
    }

    async function handleSeekForward() {
        const currentTime = playerStoreState.currentTime;
        const duration = playerStoreState.duration;
        if (duration <= 0) return;
        const newTime = Math.min(duration, currentTime + AUDIO_CONSTANTS.SEEK_AMOUNT_SECONDS);
        try {
            await playerActions.seek(newTime);
        } catch (err) {
            console.error(
                `[TrackPlayer ${deckId}] Error invoking seek forward prop:`,
                err,
            );
        }
    }

    // --- CUE Button Handlers ---
    function handleCueClick() {
        if (playerStoreState.isPlaying) {
            playerActions.setCuePoint(playerStoreState.currentTime);
        } else {
            if (playerStoreState.cuePointTime !== null) {
                playerActions.seek(playerStoreState.cuePointTime);
            } else {
                playerActions.setCuePoint(0.0);
                playerActions.seek(0.0);
            }
        }
    }

    function handleCuePointerDown() {
        isCueHeld = true;
        if (!playerStoreState.isPlaying && isAtCuePoint) {
            wasPausedAtCueWhenCuePressed = true;
            playerActions.play();
        } else {
            wasPausedAtCueWhenCuePressed = false;
        }
    }

    function handleCuePointerUp() {
        isCueHeld = false;
        if (wasPausedAtCueWhenCuePressed) {
            playerActions.pause().then(() => {
                if (playerStoreState.cuePointTime !== null) {
                    playerActions.seek(playerStoreState.cuePointTime);
                }
            });
        }
        wasPausedAtCueWhenCuePressed = false;
    }

    // --- Sync Button Handler ---
    function handleSyncToggle() {
        const currentDeckId = deckId === "A" ? "A" : "B";
        if (syncButtonStatus === "off") {
            syncStore.enableSync(currentDeckId);
        } else {
            syncStore.disableSync(currentDeckId);
        }
    }
</script>

<div class="deck-controls-wrapper">
    {#if playerStoreState.isLoading}
        <div class="loading-overlay">Loading track...</div>
    {:else if playerStoreState.error}
        <p class="error-message">Error: {playerStoreState.error}</p>
    {/if}

    <div class="mixer-controls-horizontal">
        <Slider
            id="trim-slider-{deckId}"
            label="Trim (dB)"
            orientation="vertical"
            outputMin={TRIM_CONSTANTS.TRIM_GAIN_MIN_DB}
            outputMax={TRIM_CONSTANTS.TRIM_GAIN_MAX_DB}
            centerValue={TRIM_CONSTANTS.TRIM_GAIN_DEFAULT_DB}
            step={TRIM_CONSTANTS.TRIM_STEP_DB}
            bind:value={trimDb}
        />
        <Slider
            id="fader-slider-{deckId}"
            label="Fader"
            orientation="vertical"
            outputMin={FADER_CONSTANTS.FADER_MIN}
            outputMax={FADER_CONSTANTS.FADER_MAX}
            step={FADER_CONSTANTS.FADER_STEP}
            value={faderLevel}
            onchangeValue={onFaderChange}
        />
        <div class="control-group pitch-controls">
            <Slider
                id="{deckId}-pitch"
                label="Pitch"
                orientation="vertical"
                outputMin={SYNC_CONSTANTS.PITCH_RATE_MIN}
                outputMax={SYNC_CONSTANTS.PITCH_RATE_MAX}
                centerValue={SYNC_CONSTANTS.PITCH_RATE_DEFAULT}
                step={SYNC_CONSTANTS.PITCH_SLIDER_STEP}
                value={pitchRate}
                onchangeValue={handlePitchSliderChange}
                disabled={playerStoreState.isSyncActive &&
                    !playerStoreState.isMaster}
            />
        </div>
        <Slider
            id="low-eq-slider-{deckId}"
            label="Low"
            orientation="vertical"
            outputMin={EQ_CONSTANTS.EQ_GAIN_MIN_DB}
            outputMax={EQ_CONSTANTS.EQ_GAIN_MAX_DB}
            centerValue={EQ_CONSTANTS.EQ_GAIN_DEFAULT_DB}
            step={EQ_CONSTANTS.EQ_STEP_DB}
            value={eqParams.lowGainDb}
            onchangeValue={(value) => onEqChange?.({ ...eqParams, lowGainDb: value })}
        />
        <Slider
            id="mid-eq-slider-{deckId}"
            label="Mid"
            orientation="vertical"
            outputMin={EQ_CONSTANTS.EQ_GAIN_MIN_DB}
            outputMax={EQ_CONSTANTS.EQ_GAIN_MAX_DB}
            centerValue={EQ_CONSTANTS.EQ_GAIN_DEFAULT_DB}
            step={EQ_CONSTANTS.EQ_STEP_DB}
            value={eqParams.midGainDb}
            onchangeValue={(value) => onEqChange?.({ ...eqParams, midGainDb: value })}
        />
        <Slider
            id="high-eq-slider-{deckId}"
            label="High"
            orientation="vertical"
            outputMin={EQ_CONSTANTS.EQ_GAIN_MIN_DB}
            outputMax={EQ_CONSTANTS.EQ_GAIN_MAX_DB}
            centerValue={EQ_CONSTANTS.EQ_GAIN_DEFAULT_DB}
            step={EQ_CONSTANTS.EQ_STEP_DB}
            value={eqParams.highGainDb}
            onchangeValue={(value) => onEqChange?.({ ...eqParams, highGainDb: value })}
        />
    </div>

    <!-- Transport Controls (Play/Pause/Seek/Time) -->
    <div class="transport-controls">
        <button
            class="cue-button"
            class:held={isCueHeld}
            onclick={handleCueClick}
            onpointerdown={handleCuePointerDown}
            onpointerup={handleCuePointerUp}
            onpointerleave={handleCuePointerUp}
            disabled={playerStoreState.isLoading ||
                playerStoreState.duration <= 0 ||
                !!playerStoreState.error}
            aria-label="Set or return to Cue point"
        >
            CUE
        </button>
        <button
            class="sync-button"
            class:active={syncButtonStatus === "synced" ||
                syncButtonStatus === "master"}
            class:master={syncButtonStatus === "master"}
            onclick={handleSyncToggle}
            disabled={playerStoreState.isLoading ||
                playerStoreState.duration <= 0 ||
                !!playerStoreState.error}
            aria-label={syncButtonStatus === "off"
                ? "Enable Sync"
                : "Disable Sync"}
        >
            {syncButtonStatus === "master"
                ? "MASTER"
                : syncButtonStatus === "synced"
                  ? "SYNCED"
                  : "SYNC"}
        </button>
        <button
            class="cue-audio-button"
            class:active={isCueAudioActive}
            onclick={onToggleCueAudio}
            disabled={playerStoreState.isLoading ||
                playerStoreState.duration <= 0 ||
                !!playerStoreState.error}
            aria-label={isCueAudioActive
                ? "Disable cue audio for this deck"
                : "Enable cue audio for this deck"}
        >
            🎧
        </button>
        <button
            class="seek-button"
            onclick={handleSeekBackward}
            disabled={playerStoreState.isLoading ||
                playerStoreState.duration <= 0 ||
                !!playerStoreState.error}
            aria-label="Seek backward 5 seconds"
        >
            ◀◀
        </button>
        <button
            class="play-pause-button"
            onclick={handlePlayPause}
            disabled={playerStoreState.isLoading ||
                playerStoreState.duration <= 0 ||
                !!playerStoreState.error}
            aria-label={playerStoreState.isPlaying ? "Pause" : "Play"}
        >
            {playerStoreState.isPlaying ? "Pause" : "Play"}
        </button>
        <button
            class="seek-button"
            onclick={handleSeekForward}
            disabled={playerStoreState.isLoading ||
                playerStoreState.duration <= 0 ||
                !!playerStoreState.error}
            aria-label="Seek forward 5 seconds"
        >
            ▶▶
        </button>
        <div class="deck-info-row">
            {#if currentBpm !== null}
                <span class="current-bpm">{currentBpm.toFixed(1)} BPM</span>
            {/if}
            <span class="time-info"
                >{formatTime(playerStoreState.currentTime)} / {formatTime(
                    playerStoreState.duration,
                )}</span
            >
        </div>
    </div>
</div>

<style>
    .error-message {
        text-align: center;
        padding: 1rem;
        font-style: italic;
        color: var(--error-text, #d9534f);
        background-color: var(--error-bg, #fdd);
        border: 1px solid var(--error-border, #fbb);
        border-radius: 4px;
        margin-bottom: 1rem;
    }

    .deck-controls-wrapper {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        border: 1px solid var(--section-border-light, #ccc);
        padding: 0.75rem;
        border-radius: 8px;
        background-color: var(--track-bg, #f9f9f9);
        width: 100%;
        position: relative;
    }

    .loading-overlay {
        position: absolute;
        inset: 0;
        background-color: rgba(200, 200, 200, 0.7);
        display: flex;
        justify-content: center;
        align-items: center;
        font-size: 1.2em;
        color: #333;
        border-radius: 8px;
        z-index: 10;
    }

    .transport-controls {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 0.75rem;
        padding-top: 0.5rem;
        width: 100%;
    }
    .transport-controls button {
        box-sizing: border-box;
        padding: 0.5em 1em;
        font-size: 1em;
        cursor: pointer;
        border: 1px solid #ccc;
        border-radius: 4px;
        background-color: #eee;
        min-width: 50px;
        text-align: center;
        font-weight: 500; /* Match all buttons */
        display: flex;
        align-items: center;
        justify-content: center;
        min-height: 2.5em;
        height: 2.5em;
        padding: 0;
    }
    .cue-button {
        background-color: var(--cue-button-bg, #f0ad4e);
        color: var(--cue-button-text, #fff);
        border-color: var(--cue-button-border, #eea236);
    }
    .cue-button.held {
        background-color: var(--cue-button-held-bg, #ec971f);
        border-color: var(--cue-button-held-border, #d58512);
    }

    .play-pause-button {
        width: 100px !important;
        min-width: 100px !important;
        font-weight: 500;
        background-color: #eee;
        display: flex;
        align-items: center;
        justify-content: center;
        min-height: 2.5em;
        height: 2.5em;
        padding: 0;
    }

    .mixer-controls-horizontal {
        display: flex;
        flex-direction: row;
        justify-content: space-around;
        align-items: flex-start;
        gap: 0.5rem;
        padding: 0.75rem 0;
        width: 100%;
        margin-bottom: 0.25rem;
    }

    .deck-info-row {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: 0.75rem;
        padding-top: 0.5rem;
        width: 100%;
    }

    .current-bpm {
        font-family: monospace;
        font-size: 0.9em;
        background-color: #eee;
        padding: 0.2em 0.5em;
        border-radius: 3px;
        color: orange;
        margin-right: 0;
        font-weight: 400;
        letter-spacing: 0.02em;
    }
    .time-info {
        font-family: monospace;
        font-size: 0.9em;
        background-color: #eee;
        padding: 0.2em 0.5em;
        border-radius: 3px;
        margin-left: 0;
    }

    .sync-button {
        background-color: var(--sync-button-off-bg, #777);
        color: var(--sync-button-off-text, #eee);
        border-color: var(--sync-button-off-border, #666);
        width: 120px !important;
        padding: 0.5em 0.8em !important;
        transition:
            background-color 0.2s ease,
            border-color 0.2s ease;
    }
    .sync-button.active {
        background-color: var(--sync-button-on-bg, #5cb85c);
        color: var(--sync-button-on-text, #fff);
        border-color: var(--sync-button-on-border, #4cae4c);
    }
    .sync-button.master {
        background-color: var(--sync-button-master-bg, #337ab7);
        color: var(--sync-button-master-text, #fff);
        border-color: var(--sync-button-master-border, #2e6da4);
    }

    .cue-audio-button {
        background-color: var(--cue-audio-button-bg, #6c757d);
        color: var(--cue-audio-button-text, #fff);
        border-color: var(--cue-audio-button-border, #5a6268);
        min-width: 45px;
        font-size: 1.2em;
        transition:
            background-color 0.2s ease,
            border-color 0.2s ease,
            transform 0.1s ease;
    }
    .cue-audio-button:hover:not(:disabled) {
        background-color: var(--cue-audio-button-hover-bg, #5a6268);
        transform: scale(1.05);
    }
    .cue-audio-button.active {
        background-color: var(--cue-audio-button-active-bg, #6c757d);
        color: var(--cue-audio-button-active-text, #fff);
        border-color: var(--cue-audio-button-active-border, #495057);
        box-shadow: 0 0 4px rgba(108, 117, 125, 0.3);
    }

    @media (prefers-color-scheme: dark) {
        .deck-controls-wrapper {
            border-color: var(--section-border-light, #444);
            background-color: var(--track-bg, #3a3a3a);
        }
        .error-message {
            color: var(--error-text-dark, #f48481);
            background-color: var(--error-bg-dark, #5e3e3e);
            border: 1px solid var(--error-border-dark, #a75c5c);
        }
        .loading-overlay {
            background-color: rgba(50, 50, 50, 0.7);
            color: #eee;
        }
        .transport-controls button {
            background-color: #555;
            border-color: #777;
            color: #eee;
        }
        .cue-button {
            background-color: var(--cue-button-bg-dark, #d9534f);
            color: var(--cue-button-text-dark, #fff);
            border-color: var(--cue-button-border-dark, #d43f3a);
        }
        .cue-button.held {
            background-color: var(--cue-button-held-bg-dark, #c9302c);
            border-color: var(--cue-button-held-border-dark, #ac2925);
        }
        .transport-controls button:disabled {
            opacity: 0.5;
            cursor: not-allowed;
            background-color: #555;
            color: #888;
            border-color: #666;
        }
        .transport-controls button:hover:not(:disabled):not(.cue-button) {
            background-color: #666;
        }
        .cue-button:hover:not(:disabled) {
            background-color: var(--cue-button-held-bg-dark, #c9302c);
        }
        .current-bpm {
            background-color: #555;
            color: orange;
        }
        .time-info {
            background-color: #555;
            color: #eee;
        }
        .sync-button {
            background-color: var(--sync-button-off-bg-dark, #5a5a5a);
            color: var(--sync-button-off-text-dark, #ccc);
            border-color: var(--sync-button-off-border-dark, #444);
        }
        .sync-button.active {
            background-color: var(--sync-button-on-bg-dark, #449d44);
            color: var(--sync-button-on-text-dark, #fff);
            border-color: var(--sync-button-on-border-dark, #398439);
        }
        .sync-button.master {
            background-color: var(--sync-button-master-bg-dark, #286090);
            color: var(--sync-button-master-text-dark, #fff);
            border-color: var(--sync-button-master-border-dark, #204d74);
        }
        .cue-audio-button {
            background-color: var(--cue-audio-button-bg-dark, #495057);
            color: var(--cue-audio-button-text-dark, #ccc);
            border-color: var(--cue-audio-button-border-dark, #3a3f44);
        }
        .cue-audio-button:hover:not(:disabled) {
            background-color: var(--cue-audio-button-hover-bg-dark, #3a3f44);
        }
        .cue-audio-button.active {
            background-color: var(--cue-audio-button-active-bg-dark, #6c757d);
            color: var(--cue-audio-button-active-text-dark, #fff);
            border-color: var(--cue-audio-button-active-border-dark, #5a6268);
            box-shadow: 0 0 4px rgba(108, 117, 125, 0.4);
        }
    }
</style>
