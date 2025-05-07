<script lang="ts">
    import type { VolumeInterval } from "$lib/types";

    let {
        results = null,
        audioDuration = 0,
        currentTime = 0,
        seekAudio = (time: number) => {},
        maxRms = 0,
        isAnalysisPending = false,
        isTrackLoaded = false,
        className = "",
        waveformColor = "var(--waveform-fill-default, #6488ac)",
    }: {
        results: VolumeInterval[] | null;
        audioDuration: number;
        currentTime: number;
        seekAudio: (time: number) => void;
        maxRms: number;
        isAnalysisPending?: boolean;
        isTrackLoaded?: boolean;
        className?: string;
        waveformColor?: string;
    } = $props();

    // --- Element References & State ---
    let containerElement: HTMLDivElement | null = $state(null);
    let waveformInnerElement: HTMLDivElement | null = $state(null);
    let containerWidth = $state(0);
    let containerHeight = $state(0);
    const SVG_WIDTH_MULTIPLIER = 5;

    // --- Derived State ---
    const duration = $derived(audioDuration > 0 ? audioDuration : 1);
    const roundedContainerWidth = $derived(Math.round(containerWidth));
    const waveformVisualWidth = $derived(
        roundedContainerWidth * SVG_WIDTH_MULTIPLIER,
    );
    const svgHeight = $derived(
        containerHeight > 0 ? Math.round(containerHeight) : 80,
    );

    const translateX = $derived(() => {
        if (
            roundedContainerWidth > 0 &&
            duration > 0 &&
            waveformVisualWidth > 0
        ) {
            const targetSvgX = (currentTime / duration) * waveformVisualWidth;
            const translation = roundedContainerWidth / 2 - targetSvgX;
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
        svgHeightToUse: number,
    ): string {
        if (
            !intervals ||
            intervals.length === 0 ||
            currentDuration <= 0 ||
            svgWidth <= 0 ||
            svgHeightToUse <= 0
        ) {
            return "M 0 0 Z";
        }

        // Round the constant height for path start/end
        const roundedSvgHeight = Math.round(svgHeightToUse);
        let path = `M 0 ${roundedSvgHeight}`;
        const effectiveMaxRms = currentMaxRms > 0 ? currentMaxRms : 0.0001;

        for (const interval of intervals) {
            // Round calculated x and y coordinates
            const x = Math.round(
                (interval.start_time / currentDuration) * svgWidth,
            );
            const rmsClamped = Math.max(
                0,
                Math.min(interval.rms_amplitude, effectiveMaxRms),
            );
            const y = Math.round(
                svgHeightToUse -
                    (rmsClamped / effectiveMaxRms) * svgHeightToUse,
            );
            // Use integer values in the path string
            path += ` L ${x} ${y}`;
        }

        const lastInterval = intervals[intervals.length - 1];
        // Round final x coordinates
        const lastX = Math.round(
            (lastInterval.end_time / currentDuration) * svgWidth,
        );
        const roundedSvgWidth = Math.round(svgWidth);
        path += ` L ${lastX} ${roundedSvgHeight}`;
        path += ` L ${roundedSvgWidth} ${roundedSvgHeight}`;
        path += " Z";
        return path;
    }

    // --- Derived SVG Path Data ---
    const svgPathData = $derived(() => {
        if (
            results &&
            results.length > 0 &&
            duration > 0 &&
            waveformVisualWidth > 0 &&
            svgHeight > 0
        ) {
            return calculateSvgPath(
                results,
                duration,
                maxRms,
                waveformVisualWidth,
                svgHeight,
            );
        }

        return "M 0 0 Z"; // Default empty path
    });

    // --- Effects ---
    $effect(() => {
        if (containerElement) {
            const updateDimensions = () => {
                if (containerElement) {
                    containerWidth = (containerElement as HTMLDivElement)
                        .offsetWidth;
                    containerHeight = (containerElement as HTMLDivElement)
                        .offsetHeight;
                }
            };
            updateDimensions();
            const resizeObserver = new ResizeObserver(updateDimensions);
            resizeObserver.observe(containerElement as HTMLDivElement);
            return () => resizeObserver.disconnect();
        }
    });

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
        if (
            !el ||
            roundedContainerWidth <= 0 ||
            waveformVisualWidth <= 0 ||
            duration <= 0 ||
            !results ||
            results.length === 0
        ) {
            return;
        }
        const rect = el.getBoundingClientRect();
        const clickXInContainer = event.clientX - rect.left;
        const clickOffsetFromCenterPx =
            clickXInContainer - roundedContainerWidth / 2;
        const pixelsPerSecond = waveformVisualWidth / duration;
        const timeOffset = clickOffsetFromCenterPx / pixelsPerSecond;
        let targetTime = currentTime + timeOffset;
        targetTime = Math.max(0, Math.min(targetTime, duration));
        seekAudio(targetTime);
    }

    function handleWaveformKeyDown(event: KeyboardEvent) {
        const el = containerElement;
        if (
            duration <= 1 ||
            !el ||
            roundedContainerWidth <= 0 ||
            waveformVisualWidth <= 0
        ) {
            return;
        }

        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            seekAudio(currentTime);
        }
    }

    // --- Derived values for template ---
    const canInteract = $derived(
        isTrackLoaded && duration > 0 && results && results.length > 0,
    );
</script>

{#if isTrackLoaded && results && results.length > 0}
    {@const ariaValueNow = currentTime / duration}
    <div
        class="analysis-container {className}"
        aria-label="Audio Volume Waveform"
    >
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
                style:height="{svgHeight}px"
            >
                <svg
                    class="waveform-svg"
                    viewBox={`0 0 ${waveformVisualWidth} ${svgHeight}`}
                    preserveAspectRatio="none"
                >
                    {#if svgPathData() !== "M 0 0 Z"}
                        <path
                            class="waveform-path"
                            d={svgPathData()}
                            style="fill: {waveformColor};"
                        ></path>
                    {/if}
                </svg>
            </div>
        </div>
        {#if canInteract}
            <div class="progress-indicator-fixed" aria-hidden="true"></div>
        {/if}
    </div>
{:else if isTrackLoaded && isAnalysisPending}
    <div class="loading-message">Analyzing audio...</div>
{:else if isTrackLoaded && !results && audioDuration > 0}
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
        height: 100%;
        margin: 0;
        position: relative;
    }
    .analysis-container.placeholder {
        opacity: 0.5;
    }

    .waveform-scroll-container {
        position: relative;
        display: block;
        height: 100%;
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
        stroke-width: 0;
    }

    .waveform-inner {
        /* Hint to the browser that transform will change */
        will-change: transform;
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
