<script lang="ts">
    import type { VolumeInterval } from "$lib/types";

    // --- Props ---
    let {
        results = null,
        audioDuration = 0,
        currentTime = 0,
        seekAudio = (time: number) => {},
        maxRms = 0,
        isAnalysisPending = false,
    }: {
        results: VolumeInterval[] | null;
        audioDuration: number;
        currentTime: number;
        seekAudio: (time: number) => void;
        maxRms: number;
        isAnalysisPending?: boolean;
    } = $props();

    // --- Element References & State ---
    let containerElement: HTMLDivElement | null = $state(null); // Renamed for clarity
    let waveformInnerElement: HTMLDivElement | null = $state(null); // NEW: Ref for inner div
    let containerWidth = $state(0);
    const SVG_WIDTH_MULTIPLIER = 5; // Make SVG 5 times wider than container
    const SVG_HEIGHT = 100; // Internal resolution height (amplitude range)

    // --- Derived State ---
    const duration = $derived(audioDuration > 0 ? audioDuration : 1);
    const waveformVisualWidth = $derived(containerWidth * SVG_WIDTH_MULTIPLIER);
    const translateX = $derived(() => {
        if (containerWidth > 0 && duration > 0 && waveformVisualWidth > 0) {
            const targetSvgX = (currentTime / duration) * waveformVisualWidth;
            const translation = containerWidth / 2 - targetSvgX;
            return translation;
        }
        return 0;
    });

    // --- SVG Path Calculation Function ---
    function calculateSvgPath(
        intervals: VolumeInterval[],
        currentDuration: number,
        currentMaxRms: number,
        svgWidth: number,
        svgHeight: number,
    ): string {
        if (
            !intervals ||
            intervals.length === 0 ||
            currentDuration <= 0 ||
            svgWidth <= 0
        ) {
            return "M 0 0 Z"; // Return a minimal path if invalid input
        }

        let path = `M 0 ${svgHeight}`;
        const effectiveMaxRms = currentMaxRms > 0 ? currentMaxRms : 0.0001; // Avoid division by zero

        for (const interval of intervals) {
            const x = (interval.start_time / currentDuration) * svgWidth;
            const rmsClamped = Math.max(
                0,
                Math.min(interval.rms_amplitude, effectiveMaxRms),
            );
            const y = svgHeight - (rmsClamped / effectiveMaxRms) * svgHeight;
            path += ` L ${x.toFixed(2)} ${y.toFixed(2)}`;
        }

        const lastInterval = intervals[intervals.length - 1];
        const lastX = (lastInterval.end_time / currentDuration) * svgWidth;
        path += ` L ${lastX.toFixed(2)} ${svgHeight}`;
        path += ` L ${svgWidth.toFixed(2)} ${svgHeight}`;
        path += " Z";
        return path;
    }

    // --- Derived SVG Path Data ---
    const svgPathData = $derived(() => {
        if (
            results &&
            results.length > 0 &&
            duration > 0 &&
            waveformVisualWidth > 0
        ) {
            return calculateSvgPath(
                results,
                duration,
                maxRms,
                waveformVisualWidth,
                SVG_HEIGHT,
            );
        }
        return "M 0 0 Z"; // Default empty path
    });

    // --- Effects ---
    // Update container width when the element is mounted/resized
    $effect(() => {
        if (containerElement) {
            const updateWidth = () => {
                if (containerElement) {
                    containerWidth = (containerElement as HTMLDivElement)
                        .offsetWidth;
                }
            };
            updateWidth(); // Initial width
            const resizeObserver = new ResizeObserver(updateWidth);
            resizeObserver.observe(containerElement as HTMLDivElement);
            return () => resizeObserver.disconnect();
        }
    });

    // Effect to manually set transform based on translateX
    $effect(() => {
        const el = waveformInnerElement;
        const currentTranslateX = translateX();
        if (el) {
            el.style.transform = `translateX(${currentTranslateX}px)`;
        }
    });

    // --- Event Handlers ---
    function handleWaveformClick(event: MouseEvent) {
        const el = containerElement;
        // Guards ensure necessary values are valid before calculation
        if (
            !el ||
            containerWidth <= 0 ||
            waveformVisualWidth <= 0 ||
            duration <= 0 ||
            !results ||
            results.length === 0
        ) {
            return;
        }

        const rect = el.getBoundingClientRect();
        const clickXInContainer = event.clientX - rect.left;

        // Calculate click offset from the visual center of the container
        const clickOffsetFromCenterPx = clickXInContainer - containerWidth / 2;

        // Calculate how many seconds this pixel offset represents
        const pixelsPerSecond = waveformVisualWidth / duration;
        const timeOffset = clickOffsetFromCenterPx / pixelsPerSecond;

        // Calculate the target time relative to the current time
        let targetTime = currentTime + timeOffset;

        // Clamp targetTime to valid range [0, duration]
        targetTime = Math.max(0, Math.min(targetTime, duration));

        seekAudio(targetTime);
    }

    function handleWaveformKeyDown(event: KeyboardEvent) {
        const el = containerElement;
        // Guards ensure necessary values are valid
        if (
            duration <= 1 ||
            !el ||
            containerWidth <= 0 ||
            waveformVisualWidth <= 0
        ) {
            return;
        }

        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            // Seeking at the center line means seeking to the current time
            seekAudio(currentTime);
        }
    }

    // --- Derived values for template ---
    const canInteract = $derived(duration > 0); // Can interact if duration known
</script>

{#if results && results.length > 0}
    {@const ariaValueNow = currentTime / duration}
    <div class="analysis-container" aria-label="Audio Volume Waveform">
        <div
            bind:this={containerElement}
            class="waveform-scroll-container"
            class:interactive={canInteract}
            onclick={handleWaveformClick}
            onkeydown={handleWaveformKeyDown}
            role="slider"
            tabindex={canInteract ? 0 : -1}
            aria-label="Audio waveform progress"
            aria-valuemin="0"
            aria-valuemax="1"
            aria-valuenow={ariaValueNow}
            aria-disabled={!canInteract}
        >
            <div
                bind:this={waveformInnerElement}
                class="waveform-inner"
                style:width="{waveformVisualWidth}px"
            >
                <svg
                    class="waveform-svg"
                    viewBox={`0 0 ${waveformVisualWidth} ${SVG_HEIGHT}`}
                    preserveAspectRatio="none"
                >
                    {#if svgPathData() !== "M 0 0 Z"}
                        <path class="waveform-path" d={svgPathData()}></path>
                    {/if}
                </svg>
            </div>
        </div>
        {#if canInteract}
            <div class="progress-indicator-fixed" aria-hidden="true"></div>
        {/if}
    </div>
{:else if isAnalysisPending}
    <div class="loading-message">Analyzing audio...</div>
{:else if !results && audioDuration > 0}
    <p class="analysis-status">Processing audio analysis...</p>
{:else}
    <div class="analysis-container placeholder" aria-hidden="true">
        <div class="waveform-scroll-container">
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
        position: relative;
    }
    .analysis-container.placeholder {
        opacity: 0.5;
    }

    .waveform-scroll-container {
        position: relative;
        display: block;
        height: 100px;
        background-color: var(--waveform-bg, #e0e0e0);
        border-radius: 3px;
        outline: none;
        cursor: default;
        overflow: hidden; /* Clip the translating inner element */
    }
    .waveform-scroll-container.interactive {
        cursor: pointer;
    }
    .waveform-scroll-container:not(.interactive) {
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

    .waveform-scroll-container.interactive:focus-visible {
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

    .waveform-inner {
        position: absolute;
        left: 0;
        top: 0;
        height: 100%;
        /* Add back transition for smooth visual updates */
        transition: transform 0.05s linear;
    }

    .progress-indicator-fixed {
        position: absolute;
        top: 0;
        bottom: 0;
        left: 50%;
        transform: translateX(-50%);
        width: 2px;
        background-color: var(--progress-indicator-color, #d9534f);
        pointer-events: none;
        z-index: 10;
    }

    .analysis-status {
        font-style: italic;
        color: var(--status-text-color, #666);
        margin-top: 10px;
        text-align: center;
    }

    @media (prefers-color-scheme: dark) {
        .waveform-scroll-container {
            --waveform-bg: #4f4f4f;
            --waveform-bg-disabled: #404040;
        }
        .placeholder-text {
            --placeholder-text-color: #aaa;
        }
        .waveform-path {
            --waveform-fill: #8ab4f8;
        }
        .progress-indicator-fixed {
            --progress-indicator-color: #f48481;
        }
        .analysis-status {
            --status-text-color: #bbb;
        }
    }

    .loading-message {
        display: flex;
        justify-content: center;
        align-items: center;
        height: 100px;
        background-color: var(--waveform-bg-disabled, #eee);
        border-radius: 3px;
        color: var(--text-muted, #555);
        font-style: italic;
    }

    @media (prefers-color-scheme: dark) {
        .loading-message {
            background-color: var(--waveform-bg-disabled, #404040);
            color: var(--text-muted, #aaa);
        }
    }
</style>
