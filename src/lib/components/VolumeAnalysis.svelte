<script lang="ts">
    import type { VolumeInterval } from "$lib/types";

    // --- Props --- Accepted from parent (TrackPlayer)
    let {
        results,
        audioDuration,
        currentTime,
        seekAudio, // Callback function provided by parent
        maxRms, // Accept maxRms from parent
    }: {
        results: VolumeInterval[] | null;
        audioDuration: number;
        currentTime: number;
        seekAudio: (event: MouseEvent) => void;
        maxRms: number; // Define prop type
    } = $props();

    // --- Derived State ---
    // Ensure duration is never zero for calculations
    const duration = $derived(audioDuration > 0 ? audioDuration : 1);

    // Calculate the maximum RMS value for scaling
    // const maxRms = $derived(...);

    // --- SVG Path Calculation ---
    const SVG_WIDTH = 1000; // Internal resolution width
    const SVG_HEIGHT = 100; // Internal resolution height (amplitude range)

    // --- Event Handlers ---
    function handleWaveformClick(event: MouseEvent) {
        if (!results || results.length === 0) return; // Don't seek if no waveform
        seekAudio(event); // Call the provided seek function
    }

    function handleWaveformKeyDown(event: KeyboardEvent) {
        if (!results || results.length === 0) return;
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            const target = event.currentTarget as HTMLElement;
            const rect = target.getBoundingClientRect();
            const fakeEvent = new MouseEvent("click", {
                clientX: rect.left + rect.width / 2,
                clientY: rect.top + rect.height / 2,
                bubbles: true,
            });
            seekAudio(fakeEvent);
        }
    }

    // --- Derived values for template ---
    const progressPercent = $derived(
        duration > 0 ? (currentTime / duration) * 100 : 0,
    );
    const canInteract = $derived(results && results.length > 0 && duration > 0);
</script>

{#if results && results.length > 0}
    {@const ariaValueNow = progressPercent}
    <div class="analysis-container" aria-label="Audio Volume Waveform">
        <div
            class:waveform-container={true}
            class:interactive={canInteract}
            onclick={handleWaveformClick}
            onkeydown={handleWaveformKeyDown}
            role="slider"
            tabindex={canInteract ? 0 : -1}
            aria-label="Audio waveform progress"
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={ariaValueNow}
            aria-disabled={!canInteract}
        >
            <svg
                class="waveform-svg"
                viewBox={`0 0 ${SVG_WIDTH} ${SVG_HEIGHT}`}
                preserveAspectRatio="none"
            >
                {#if results && results.length > 0}
                    {@const pathData = (() => {
                        let path = `M 0 ${SVG_HEIGHT}`;
                        const currentDuration = duration;
                        const currentMaxRms = maxRms > 0 ? maxRms : 0.0001; // Ensure non-zero
                        for (const interval of results) {
                            const x =
                                (interval.start_time / currentDuration) *
                                SVG_WIDTH;
                            const rmsClamped = Math.max(
                                0,
                                Math.min(interval.rms_amplitude, currentMaxRms),
                            );
                            const y =
                                SVG_HEIGHT -
                                (rmsClamped / currentMaxRms) * SVG_HEIGHT;
                            path += ` L ${x.toFixed(2)} ${y.toFixed(2)}`;
                        }
                        const lastInterval = results[results.length - 1];
                        const lastX =
                            (lastInterval.end_time / currentDuration) *
                            SVG_WIDTH;
                        path += ` L ${lastX.toFixed(2)} ${SVG_HEIGHT}`;
                        path += ` L ${SVG_WIDTH} ${SVG_HEIGHT}`;
                        path += " Z";
                        return path;
                    })()}
                    <path class="waveform-path" d={pathData}></path>
                {/if}
            </svg>
            <!-- Simple progress line overlay -->
            <div
                class="progress-indicator"
                style:left={`${progressPercent}%`}
                aria-hidden="true"
            ></div>
        </div>
    </div>
{:else if !results && audioDuration > 0}
    <p class="analysis-status">Processing audio analysis...</p>
{:else}
    <!-- Optional: Placeholder when no audio is loaded/no results -->
    <div class="analysis-container placeholder" aria-hidden="true">
        <div class="waveform-container">
            <span class="placeholder-text">Load audio to see waveform</span>
        </div>
    </div>
{/if}

<style>
    .analysis-container {
        display: block;
        width: 100%;
        max-width: 1200px;
        margin: 10px 0;
    }
    .analysis-container.placeholder {
        opacity: 0.5;
    }

    .waveform-container {
        position: relative;
        display: block;
        height: 100px;
        background-color: var(--waveform-bg, #e0e0e0);
        border-radius: 3px;
        overflow: hidden;
        outline: none;
        cursor: default; /* Default non-interactive */
    }
    .waveform-container.interactive {
        cursor: pointer;
    }
    .waveform-container:not(.interactive) {
        /* Style for non-interactive state if needed */
        background-color: var(--waveform-bg-disabled, #eee);
    }

    .placeholder-text {
        display: flex;
        align-items: center;
        justify-content: center;
        height: 100%;
        font-style: italic;
        color: var(--placeholder-text-color, #888);
    }

    .waveform-container.interactive:focus-visible {
        box-shadow: 0 0 0 2px var(--accent-color, #4a9eff);
    }

    .waveform-svg {
        display: block;
        width: 100%;
        height: 100%;
    }

    .waveform-path {
        fill: var(--waveform-fill, #6488ac);
        stroke-width: 0;
    }

    .progress-indicator {
        position: absolute;
        top: 0;
        bottom: 0;
        width: 2px;
        background-color: var(--progress-indicator-color, #d9534f);
        pointer-events: none; /* Click goes through to the container */
        transition: left 0.1s linear; /* Smooth progress movement */
    }

    .analysis-status {
        font-style: italic;
        color: var(--status-text-color, #666);
        margin-top: 10px;
        text-align: center;
    }

    @media (prefers-color-scheme: dark) {
        .waveform-container {
            --waveform-bg: #4f4f4f;
            --waveform-bg-disabled: #404040;
        }
        .placeholder-text {
            --placeholder-text-color: #aaa;
        }
        .waveform-path {
            --waveform-fill: #8ab4f8;
        }
        .progress-indicator {
            --progress-indicator-color: #f48481;
        }
        .analysis-status {
            --status-text-color: #bbb;
        }
    }
    /* Light mode defaults are mostly fine */
</style>
