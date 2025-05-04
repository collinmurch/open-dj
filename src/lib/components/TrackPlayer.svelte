<script lang="ts">
    import { libraryStore } from "$lib/stores/libraryStore";
    import {
        createPlayerStore,
        type PlayerStore,
    } from "$lib/stores/playerStore";
    import VolumeAnalysis from "./VolumeAnalysis.svelte";
    import VerticalSlider from "./VerticalSlider.svelte";

    // --- Props ---
    let {
        filePath = null,
        deckId,
    }: {
        filePath: string | null;
        deckId: string;
    } = $props();

    // --- Time Formatting Utility ---
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

    // --- Component References ---

    // --- Create Player Store Instance ---
    // Use deckId to create a specific store instance
    const playerStore: PlayerStore = createPlayerStore(deckId);
    // Use $playerStore directly in the template for auto-subscription

    // --- Volume State ---
    let volume = $state(1.0); // Default volume (0.0 to 2.0)

    // --- Derived analysis result based on filePath ---
    const trackInfo = $derived(
        $libraryStore.audioFiles.find((track) => track.path === filePath),
    );
    const analysisFeatures = $derived(trackInfo?.features);
    const volumeAnalysisResult = $derived(
        analysisFeatures === undefined
            ? undefined
            : (analysisFeatures?.volume ?? null),
    );

    // Determine if a track is actually loaded based on filePath prop
    const isTrackLoaded = $derived(!!filePath);

    // --- Effects ---

    // Effect to load audio data when filePath prop changes
    $effect(() => {
        const currentFilePath = filePath; // Capture current prop value

        if (!currentFilePath) {
            return;
        }

        // Call the store method to load the track via Rust
        playerStore.loadTrack(currentFilePath).catch((err) => {
            // This catch is mostly for logging invoke errors from the store,
            // state updates (including errors) come via events.
            console.error(
                `[TrackPlayer ${deckId}] Error invoking loadTrack:`,
                err,
            );
        });
    });

    // Effect for component cleanup
    $effect(() => {
        // Return the cleanup function from the store
        return () => {
            playerStore.cleanup();
        };
    });

    // Effect to update volume in Rust when local state changes
    $effect(() => {
        const currentVolume = volume; // Get current value directly
        console.log(
            `TrackPlayer ${deckId}: Volume changed to ${currentVolume}, calling store.setVolume`,
        );
        playerStore.setVolume(currentVolume).catch((err: unknown) => {
            // Added err type
            console.error(
                `[TrackPlayer ${deckId}] Error invoking setVolume:`,
                err,
            );
        });
    });

    const SEEK_AMOUNT = 5; // Seek 5 seconds

    // --- Callbacks ---
    function seekAudioCallback(time: number) {
        playerStore.seek(time).catch((err) => {
            console.error(`[TrackPlayer ${deckId}] Error invoking seek:`, err);
        });
    }

    // --- Event Handlers for Buttons ---
    function handlePlayPause() {
        // Use $playerStore to access reactive state
        if ($playerStore.isPlaying) {
            playerStore
                .pause()
                .catch((err) =>
                    console.error(
                        `[TrackPlayer ${deckId}] Error invoking pause:`,
                        err,
                    ),
                );
        } else {
            playerStore
                .play()
                .catch((err) =>
                    console.error(
                        `[TrackPlayer ${deckId}] Error invoking play:`,
                        err,
                    ),
                );
        }
    }

    function handleSeekBackward() {
        const currentTime = $playerStore.currentTime;
        const duration = $playerStore.duration;
        if (duration <= 0) return;
        const newTime = Math.max(0, currentTime - SEEK_AMOUNT);
        playerStore
            .seek(newTime)
            .catch((err) =>
                console.error(
                    `[TrackPlayer ${deckId}] Error invoking seek backward:`,
                    err,
                ),
            );
    }

    function handleSeekForward() {
        const currentTime = $playerStore.currentTime;
        const duration = $playerStore.duration;
        if (duration <= 0) return;
        const newTime = Math.min(duration, currentTime + SEEK_AMOUNT);
        playerStore
            .seek(newTime)
            .catch((err) =>
                console.error(
                    `[TrackPlayer ${deckId}] Error invoking seek forward:`,
                    err,
                ),
            );
    }
</script>

<div class="track-player-wrapper">
    {#if $playerStore.isLoading}
        <div class="loading-overlay">Loading track...</div>
    {:else if $playerStore.error}
        <p class="error-message">Error: {$playerStore.error}</p>
    {/if}

    <div class="controls">
        <button
            class="seek-button"
            onclick={handleSeekBackward}
            disabled={$playerStore.isLoading ||
                $playerStore.duration <= 0 ||
                !!$playerStore.error}
            aria-label="Seek backward 5 seconds"
        >
            ◀◀
        </button>
        <button
            class="play-pause-button"
            onclick={handlePlayPause}
            disabled={$playerStore.isLoading ||
                $playerStore.duration <= 0 ||
                !!$playerStore.error}
            aria-label={$playerStore.isPlaying ? "Pause" : "Play"}
        >
            {$playerStore.isPlaying ? "Pause" : "Play"}
        </button>
        <button
            class="seek-button"
            onclick={handleSeekForward}
            disabled={$playerStore.isLoading ||
                $playerStore.duration <= 0 ||
                !!$playerStore.error}
            aria-label="Seek forward 5 seconds"
        >
            ▶▶
        </button>
        <span class="time-display">
            {formatTime($playerStore.currentTime)} / {formatTime(
                $playerStore.duration,
            )}
        </span>
    </div>

    <div class="player-body-area">
        <VerticalSlider
            id="volume-slider-{deckId}"
            label="Volume"
            min={0}
            max={2}
            step={0.05}
            bind:value={volume}
            debounceMs={50}
        />
        <div class="waveform-area">
            <VolumeAnalysis
                results={volumeAnalysisResult?.intervals ?? null}
                maxRms={volumeAnalysisResult?.max_rms_amplitude ?? 0}
                isAnalysisPending={analysisFeatures === undefined}
                {isTrackLoaded}
                audioDuration={$playerStore.duration}
                currentTime={$playerStore.currentTime}
                seekAudio={seekAudioCallback}
            />
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
        margin-bottom: 1rem; /* Add margin */
    }

    .track-player-wrapper {
        display: flex;
        flex-direction: column;
        gap: 0.75rem; /* Adjust gap slightly */
        border: 1px solid #ccc;
        padding: 1rem;
        border-radius: 8px;
        background-color: var(--track-bg, #f9f9f9);
        width: 100%;
        /* max-width: 600px; */ /* Allow flexible width based on parent */
        margin-bottom: 0.5rem; /* Reduce margin */
        position: relative;
        min-height: 200px; /* Keep min height */
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

    .controls {
        display: flex;
        align-items: center;
        justify-content: center; /* Center the controls */
        gap: 0.75rem; /* Adjust gap */
        padding-bottom: 0.5rem;
        border-bottom: 1px solid #eee;
        margin-bottom: 1rem;
    }
    .controls button {
        padding: 0.5em 1em;
        font-size: 1em;
        cursor: pointer;
        border: 1px solid #ccc;
        border-radius: 4px;
        background-color: #eee;
    }
    .controls button:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .play-pause-button {
        min-width: 80px; /* Give play/pause a bit more width */
        font-weight: bold;
    }
    .time-display {
        font-family: monospace;
        font-size: 0.9em;
        background-color: #eee;
        padding: 0.2em 0.5em;
        border-radius: 3px;
        margin-left: auto; /* Push time display to the right */
    }

    .player-body-area {
        display: flex;
        flex-direction: row;
        align-items: stretch;
        gap: 1rem;
        flex-grow: 1;
        min-height: 130px;
    }

    .waveform-area {
        flex-grow: 1;
        min-width: 0;
        position: relative;
        display: flex;
        align-items: stretch;
    }

    @media (prefers-color-scheme: dark) {
        .track-player-wrapper {
            border-color: #444;
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
        .controls {
            border-bottom-color: #444;
        }
        .controls button {
            background-color: #555;
            border-color: #777;
            color: #eee;
        }
        .controls button:hover:not(:disabled) {
            background-color: #666;
        }
        .time-display {
            background-color: #555;
            color: #eee;
        }
        /* Styles for volume control in dark mode could go here if needed */
        /* .volume-control input[type="range"] { ... } */
    }
</style>
