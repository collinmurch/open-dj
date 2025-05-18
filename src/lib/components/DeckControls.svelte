<script lang="ts">
    import type { PlayerStore } from "$lib/stores/playerStore";
    import type { PlayerState, EqParams } from "$lib/types";
    import { formatTime } from "$lib/utils/timeUtils";
    import { invoke } from "@tauri-apps/api/core";
    import Slider from "./Slider.svelte";
    import { syncStore, type SyncStatus } from "$lib/stores/syncStore";

    let {
        filePath = null,
        deckId,
        playerStoreState,
        playerActions,
        trimDb = $bindable(0.0),
        faderLevel = $bindable(1.0),
        eqParams = $bindable({
            lowGainDb: 0.0,
            midGainDb: 0.0,
            highGainDb: 0.0,
        } as EqParams),
        currentBpm = null as number | null,
        originalBpm = null as number | null | undefined,
        pitchRate = 1.0,
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
        originalBpm?: number | null | undefined;
        pitchRate?: number;
    } = $props();

    // --- Volume, Trim & EQ State (remains the same) ---
    let trimDebounceTimeout: number | undefined = undefined;
    let faderDebounceTimeout: number | undefined = undefined;
    let eqDebounceTimeout: number | undefined = undefined;
    const TRIM_DEBOUNCE_MS = 50;
    const FADER_DEBOUNCE_MS = 50;
    const EQ_DEBOUNCE_MS = 50;

    // --- CUE State ---
    let isCueHeld = $state(false); // Track if cue button is currently held down
    let wasPausedAtCueWhenCuePressed = $state(false); // Flag for cue play logic

    // Derived state to check if playback is currently at the cue point
    const isAtCuePoint = $derived(() => {
        const cueTime = playerStoreState.cuePointTime;
        const currentTime = playerStoreState.currentTime;
        return cueTime !== null && Math.abs(currentTime - cueTime) < 0.1; // Tolerance of 100ms
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
        };
    });

    // Effect to update Fader Level in Rust
    $effect(() => {
        const currentFaderLevel = faderLevel;
        invoke("set_fader_level", {
            deckId,
            level: currentFaderLevel,
        }).catch((err) => {
            console.error(
                `[TrackPlayer ${deckId}] Error invoking set_fader_level:`,
                err,
            );
        });
    });

    // Effect to update Trim Gain in Rust
    $effect(() => {
        const currentTrimDb = trimDb;
        invoke("set_trim_gain", {
            deckId,
            gainDb: currentTrimDb,
        }).catch((err) => {
            console.error(
                `[TrackPlayer ${deckId}] Error invoking set_trim_gain:`,
                err,
            );
        });
    });

    // Effect to update EQ parameters in Rust
    $effect(() => {
        // Only send EQ if deck is loaded and ready
        const isDeckReady =
            playerStoreState.duration > 0 && !playerStoreState.isLoading;
        if (!isDeckReady) return;

        const paramsToSend = eqParams;
        invoke("set_eq_params", {
            deckId: deckId,
            params: paramsToSend,
        }).catch((err) => {
            console.error(`Failed to set EQ for ${deckId}:`, err);
        });
    });

    const SEEK_AMOUNT = 5; // Seek 5 seconds

    // Event handler for pitch slider changes from Slider's onchangeValue event
    function handlePitchSliderChange(newPitchValue: number) {
        if (playerActions && typeof playerActions.setPitchRate === "function") {
            playerActions.setPitchRate(newPitchValue).catch((err) => {
                console.error(
                    `[TrackPlayer ${deckId}] Error invoking setPitchRate prop:`,
                    err,
                );
            });
        }
    }

    // --- Event Handlers for Buttons (use playerActions props) ---
    function handlePlayPause() {
        if (playerStoreState.isPlaying) {
            playerActions
                .pause()
                .catch((err) =>
                    console.error(
                        `[TrackPlayer ${deckId}] Error invoking pause prop:`,
                        err,
                    ),
                );
        } else {
            playerActions
                .play()
                .catch((err) =>
                    console.error(
                        `[TrackPlayer ${deckId}] Error invoking play prop:`,
                        err,
                    ),
                );
        }
    }

    function handleSeekBackward() {
        const currentTime = playerStoreState.currentTime;
        const duration = playerStoreState.duration;
        if (duration <= 0) return;
        const newTime = Math.max(0, currentTime - SEEK_AMOUNT);
        playerActions
            .seek(newTime)
            .catch((err) =>
                console.error(
                    `[TrackPlayer ${deckId}] Error invoking seek backward prop:`,
                    err,
                ),
            );
    }

    function handleSeekForward() {
        const currentTime = playerStoreState.currentTime;
        const duration = playerStoreState.duration;
        if (duration <= 0) return;
        const newTime = Math.min(duration, currentTime + SEEK_AMOUNT);
        playerActions
            .seek(newTime)
            .catch((err) =>
                console.error(
                    `[TrackPlayer ${deckId}] Error invoking seek forward prop:`,
                    err,
                ),
            );
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
        if (!playerStoreState.isPlaying && isAtCuePoint()) {
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
            outputMin={-12}
            outputMax={12}
            centerValue={0}
            step={1}
            bind:value={trimDb}
        />
        <Slider
            id="fader-slider-{deckId}"
            label="Fader"
            orientation="vertical"
            outputMin={0}
            outputMax={1}
            step={0.01}
            bind:value={faderLevel}
        />
        <div class="control-group pitch-controls">
            <Slider
                id="{deckId}-pitch"
                label="Pitch"
                orientation="vertical"
                outputMin={0.75}
                outputMax={1.25}
                centerValue={1.0}
                step={0.0001}
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
            outputMin={-26}
            outputMax={6}
            centerValue={0}
            step={1}
            bind:value={eqParams.lowGainDb}
        />
        <Slider
            id="mid-eq-slider-{deckId}"
            label="Mid"
            orientation="vertical"
            outputMin={-26}
            outputMax={6}
            centerValue={0}
            step={1}
            bind:value={eqParams.midGainDb}
        />
        <Slider
            id="high-eq-slider-{deckId}"
            label="High"
            orientation="vertical"
            outputMin={-26}
            outputMax={6}
            centerValue={0}
            step={1}
            bind:value={eqParams.highGainDb}
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
                !!playerStoreState.error ||
                !originalBpm}
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
                !!playerStoreState.error ||
                !originalBpm}
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
        min-width: 100px !important;
        width: 100px !important;
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
        min-width: 60px; /* Slightly wider for MASTER text */
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
    }
</style>
