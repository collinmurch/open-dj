<script lang="ts">
    // Removed libraryStore import as analysis results are handled by parent
    // Removed createPlayerStore as store is passed in
    import Slider from "./Slider.svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { onDestroy } from "svelte";
    import type { PlayerStore } from "$lib/stores/playerStore"; // For typing playerActions
    import type { PlayerState } from "$lib/types"; // For typing playerStoreState

    // --- Props --- Modified to accept store state and actions
    let {
        filePath = null, // Still needed to trigger initial load
        deckId,
        playerStoreState, // The reactive Svelte store state ($playerStoreA or $playerStoreB)
        playerActions, // The object with action methods from the store
    }: {
        filePath: string | null;
        deckId: string;
        playerStoreState: PlayerState;
        playerActions: Pick<
            PlayerStore,
            "loadTrack" | "play" | "pause" | "seek" | "cleanup" | "setVolume"
        >;
    } = $props();

    // --- Time Formatting Utility (remains the same) ---
    function formatTime(totalSeconds: number): string {
        if (isNaN(totalSeconds) || totalSeconds < 0) {
            return "00:00";
        }
        const minutes = Math.floor(totalSeconds / 60);
        const seconds = Math.floor(totalSeconds % 60);
        const paddedMinutes = String(minutes).padStart(2, "0");
        const paddedSeconds = String(seconds).padStart(2, "0");
        return `${paddedMinutes}:${paddedSeconds}`;
    }

    // --- Volume, Trim & EQ State (remains the same) ---
    let trimDb = $state(0.0);
    let faderLevel = $state(1.0);
    let lowGainDb = $state(0.0);
    let midGainDb = $state(0.0);
    let highGainDb = $state(0.0);
    let trimDebounceTimeout: number | undefined = undefined;
    let faderDebounceTimeout: number | undefined = undefined;
    let eqDebounceTimeout: number | undefined = undefined;
    const TRIM_DEBOUNCE_MS = 50;
    const FADER_DEBOUNCE_MS = 50;
    const EQ_DEBOUNCE_MS = 50;

    // --- REMOVED Derived analysis result based on filePath ---
    // This is now handled in +page.svelte

    // Determine if a track is actually loaded based on filePath prop
    // const isTrackLoaded = $derived(!!filePath); // Not directly needed here for VolumeAnalysis anymore

    // --- Effects ---

    // Effect to load audio data when filePath prop changes
    $effect(() => {
        const currentFilePath = filePath;
        if (!currentFilePath) {
            trimDb = 0;
            faderLevel = 1.0;
            lowGainDb = 0;
            midGainDb = 0;
            highGainDb = 0;
            // playerActions.cleanup(); // Let parent handle cleanup if store is managed by parent or do it on $destroy
            return;
        }
        // Use the loadTrack action passed via props
        playerActions.loadTrack(currentFilePath).catch((err) => {
            console.error(
                `[TrackPlayer ${deckId}] Error invoking loadTrack prop:`, // Log adjusted
                err,
            );
        });
    });

    // Effect for component cleanup (if needed for timeouts)
    $effect(() => {
        return () => {
            // playerActions.cleanup(); // Parent should manage store lifecycle now
            if (trimDebounceTimeout !== undefined)
                clearTimeout(trimDebounceTimeout);
            if (faderDebounceTimeout !== undefined)
                clearTimeout(faderDebounceTimeout);
            if (eqDebounceTimeout !== undefined)
                clearTimeout(eqDebounceTimeout);
        };
    });

    // Effects for Trim, Fader, EQ remain largely the same, invoking Rust via Tauri
    // Effect to update Fader Level in Rust
    $effect(() => {
        const currentFaderLevel = faderLevel;
        if (faderDebounceTimeout !== undefined)
            clearTimeout(faderDebounceTimeout);
        faderDebounceTimeout = setTimeout(async () => {
            console.log(`Updating Fader for ${deckId} to ${currentFaderLevel}`);
            try {
                await invoke("set_fader_level", {
                    deckId,
                    level: currentFaderLevel,
                });
            } catch (err: unknown) {
                console.error(
                    `[TrackPlayer ${deckId}] Error invoking set_fader_level:`,
                    err,
                );
            }
        }, FADER_DEBOUNCE_MS);
    });

    // Effect to update Trim Gain in Rust
    $effect(() => {
        const currentTrimDb = trimDb;
        if (trimDebounceTimeout !== undefined)
            clearTimeout(trimDebounceTimeout);
        trimDebounceTimeout = setTimeout(async () => {
            console.log(`Updating Trim for ${deckId} to ${currentTrimDb} dB`);
            try {
                await invoke("set_trim_gain", {
                    deckId,
                    gainDb: currentTrimDb,
                });
            } catch (err: unknown) {
                console.error(
                    `[TrackPlayer ${deckId}] Error invoking set_trim_gain:`,
                    err,
                );
            }
        }, TRIM_DEBOUNCE_MS);
    });

    // Effect to update EQ parameters in Rust
    $effect(() => {
        const paramsToSend = {
            low_gain_db: lowGainDb,
            mid_gain_db: midGainDb,
            high_gain_db: highGainDb,
        };
        if (eqDebounceTimeout !== undefined) clearTimeout(eqDebounceTimeout);
        eqDebounceTimeout = setTimeout(async () => {
            console.log(`Updating EQ for ${deckId}:`, paramsToSend);
            try {
                await invoke("set_eq_params", {
                    deckId,
                    lowGainDb: paramsToSend.low_gain_db,
                    midGainDb: paramsToSend.mid_gain_db,
                    highGainDb: paramsToSend.high_gain_db,
                });
            } catch (err) {
                console.error(`Failed to set EQ for ${deckId}:`, err);
            }
        }, EQ_DEBOUNCE_MS);
    });

    const SEEK_AMOUNT = 5; // Seek 5 seconds

    // --- REMOVED Callbacks for VolumeAnalysis (seekAudioCallback) ---

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
</script>

<div class="track-player-wrapper">
    {#if playerStoreState.isLoading}
        <div class="loading-overlay">Loading track...</div>
    {:else if playerStoreState.error}
        <p class="error-message">Error: {playerStoreState.error}</p>
    {/if}

    <!-- REMOVED Waveform Area -->

    <!-- Mixer Controls - Unchanged structurally, but parent div might change -->
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
            debounceMs={0}
        />
        <Slider
            id="fader-slider-{deckId}"
            label="Fader"
            orientation="vertical"
            outputMin={0}
            outputMax={1}
            step={0.01}
            bind:value={faderLevel}
            debounceMs={0}
        />
        <Slider
            id="low-eq-slider-{deckId}"
            label="Low"
            orientation="vertical"
            outputMin={-26}
            outputMax={6}
            centerValue={0}
            step={1}
            bind:value={lowGainDb}
            debounceMs={0}
        />
        <Slider
            id="mid-eq-slider-{deckId}"
            label="Mid"
            orientation="vertical"
            outputMin={-26}
            outputMax={6}
            centerValue={0}
            step={1}
            bind:value={midGainDb}
            debounceMs={0}
        />
        <Slider
            id="high-eq-slider-{deckId}"
            label="High"
            orientation="vertical"
            outputMin={-26}
            outputMax={6}
            centerValue={0}
            step={1}
            bind:value={highGainDb}
            debounceMs={0}
        />
    </div>

    <!-- Transport Controls (Play/Pause/Seek/Time) -->
    <div class="transport-controls">
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
        <span class="time-display">
            {formatTime(playerStoreState.currentTime)} / {formatTime(
                playerStoreState.duration,
            )}
        </span>
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

    .track-player-wrapper {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        border: 1px solid var(--section-border-light, #ccc); /* Lightened border */
        padding: 0.75rem; /* Slightly reduced padding */
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
        padding-top: 0.5rem; /* Reduced padding */
        width: 100%;
    }
    .transport-controls button {
        padding: 0.5em 1em;
        font-size: 1em;
        cursor: pointer;
        border: 1px solid #ccc;
        border-radius: 4px;
        background-color: #eee;
    }
    .transport-controls button:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .play-pause-button {
        min-width: 80px;
        font-weight: bold;
    }
    .time-display {
        font-family: monospace;
        font-size: 0.9em;
        background-color: #eee;
        padding: 0.2em 0.5em;
        border-radius: 3px;
        margin-left: auto;
    }

    .mixer-controls-horizontal {
        display: flex;
        flex-direction: row;
        justify-content: space-around;
        align-items: flex-start;
        gap: 0.5rem;
        padding: 0.75rem 0;
        width: 100%;
        /* Removed borders, will be simpler component now */
        /* margin-top: 0.75rem; */ /* Removed top margin */
        margin-bottom: 0.25rem;
    }

    /* REMOVED .waveform-area styles */
    /* REMOVED :global(.track-player-waveform .waveform-scroll-container) */

    @media (prefers-color-scheme: dark) {
        .track-player-wrapper {
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
        .transport-controls button:hover:not(:disabled) {
            background-color: #666;
        }
        .time-display {
            background-color: #555;
            color: #eee;
        }
        /* Removed .mixer-controls-horizontal dark theme border overrides */
        /* Removed .waveform-area dark theme background override */
    }
</style>
