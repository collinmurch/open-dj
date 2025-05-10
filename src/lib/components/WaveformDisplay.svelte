<script lang="ts">
    import type { VolumeAnalysis, WaveBin } from "$lib/types";

    let {
        volumeAnalysis = null,
        audioDuration = 0,
        currentTime = 0,
        seekAudio = (time: number) => {},
        isAnalysisPending = false,
        isTrackLoaded = false,
        className = "",
        waveformColor = "var(--waveform-fill-default, #6488ac)",
        cuePointTime = null as number | null,
    }: {
        volumeAnalysis: VolumeAnalysis | null;
        audioDuration: number;
        currentTime: number;
        seekAudio: (time: number) => void;
        isAnalysisPending?: boolean;
        isTrackLoaded?: boolean;
        className?: string;
        waveformColor?: string;
        cuePointTime?: number | null;
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

    const currentMaxRms = $derived(volumeAnalysis?.max_rms_amplitude ?? 0.0001);
    const activeMipLevel = $derived.by(() => {
        if (
            volumeAnalysis &&
            volumeAnalysis.levels &&
            volumeAnalysis.levels.length > 0
        ) {
            if (
                volumeAnalysis.levels[0] &&
                volumeAnalysis.levels[0].length > 0
            ) {
                return volumeAnalysis.levels[0];
            }
        }
        return null;
    });

    const translateX = $derived.by(() => {
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

    // --- SVG Path Calculation Function (Modified) ---
    function calculateSvgPath(
        bins: WaveBin[] | null,
        currentDuration: number,
        currentMaxOverallRms: number,
        svgWidth: number,
        svgHeightToUse: number,
    ): string {
        if (
            !bins ||
            bins.length === 0 ||
            currentDuration <= 0 ||
            svgWidth <= 0 ||
            svgHeightToUse <= 0 ||
            currentMaxOverallRms <= 0
        ) {
            return "M 0 0 Z";
        }

        const roundedSvgHeight = Math.round(svgHeightToUse);
        let path = `M 0 ${roundedSvgHeight}`;

        const numBins = bins.length;
        const binWidthOnSvg = svgWidth / numBins;
        const effectiveMaxRms =
            currentMaxOverallRms > 0 ? currentMaxOverallRms : 0.0001;

        let hasLoggedNonZeroEnergy = false;

        for (let i = 0; i < numBins; i++) {
            const waveBin = bins[i];
            const binEnergy = waveBin.low + waveBin.mid + waveBin.high;

            const normalizedEnergy =
                Math.max(0, Math.min(binEnergy, effectiveMaxRms)) /
                effectiveMaxRms;
            const y = Math.round(svgHeightToUse * (1 - normalizedEnergy));

            const xStart = Math.round(i * binWidthOnSvg);
            const xEnd = Math.round((i + 1) * binWidthOnSvg);

            if (i === 0) {
                path += ` L ${xStart} ${y}`;
            }
            path += ` L ${xEnd} ${y}`;
        }

        path += ` L ${Math.round(svgWidth)} ${roundedSvgHeight}`;
        path += " Z";
        return path;
    }

    // --- Derived SVG Path Data ---
    const svgPathData = $derived.by(() => {
        if (
            activeMipLevel &&
            duration > 0 &&
            waveformVisualWidth > 0 &&
            svgHeight > 0
        ) {
            return calculateSvgPath(
                activeMipLevel,
                duration,
                currentMaxRms,
                waveformVisualWidth,
                svgHeight,
            );
        }
        return "M 0 0 Z";
    });

    // --- NEW: Derived Cue Marker Position ---
    const cueMarkerLeftOffset = $derived.by(() => {
        const currentContainerWidth = roundedContainerWidth;
        if (
            cuePointTime !== null &&
            duration > 0 &&
            waveformVisualWidth > 0 &&
            currentContainerWidth > 0
        ) {
            const svgX = (cuePointTime / duration) * waveformVisualWidth;
            const containerX = translateX + svgX;
            if (containerX >= 0 && containerX <= currentContainerWidth) {
                return `${containerX}px`;
            }
        }
        return null;
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
        const currentTranslateX = translateX;
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
            !activeMipLevel
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
        isTrackLoaded && duration > 0 && activeMipLevel,
    );
</script>

{#if isTrackLoaded && activeMipLevel}
    {@const ariaValueNow = duration > 0 ? currentTime / duration : 0}
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
                    viewBox="0 0 {waveformVisualWidth} {svgHeight}"
                    preserveAspectRatio="none"
                >
                    {#if svgPathData !== "M 0 0 Z"}
                        <path
                            class="waveform-path"
                            d={svgPathData}
                            style="fill: {waveformColor};"
                        ></path>
                    {/if}
                </svg>
            </div>
        </div>
        {#if canInteract}
            <div class="progress-indicator-fixed" aria-hidden="true"></div>
        {/if}
        <!-- NEW: Cue Marker -->
        {#if cueMarkerLeftOffset !== null}
            <div
                class="cue-marker"
                style:left={cueMarkerLeftOffset}
                aria-hidden="true"
            ></div>
        {/if}
    </div>
{:else if isTrackLoaded && isAnalysisPending}
    <div class="loading-message">Analyzing audio...</div>
{:else if isTrackLoaded && !activeMipLevel && audioDuration > 0}
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
        will-change: transform;
        position: absolute;
        left: 0;
        top: 0;
        height: 100%;
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

    .cue-marker {
        position: absolute;
        top: 0;
        bottom: 0;
        width: 2px;
        background-color: #cfe2ff;
        pointer-events: none;
        z-index: 9;
        transition: left 0.05s linear;
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
        .cue-marker {
            background-color: #2a6bba;
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
