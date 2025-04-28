<script lang="ts">
    import {
        analysisResults,
        type VolumeInterval,
        audioProgress,
        audioDuration,
    } from "$lib/stores/audioStore";

    // Reactive assignment for analysis results
    $: results = $analysisResults;

    // Reactive assignment for current playback time
    $: currentTime = $audioProgress;

    // Reactive assignment for audio duration, ensuring it's never zero
    $: duration = $audioDuration > 0 ? $audioDuration : 1;

    // Calculate the maximum RMS value for scaling
    $: maxRms = results
        ? Math.max(0.0001, ...results.map((r) => r.rms_amplitude))
        : 0.0001;

    // --- SVG Path Calculation ---
    const SVG_WIDTH = 1000; // Internal resolution width
    const SVG_HEIGHT = 100; // Internal resolution height (amplitude range)

    $: svgPathData = (() => {
        if (!results || results.length === 0) {
            return "";
        }

        // Start path at bottom-left
        let path = `M 0 ${SVG_HEIGHT}`;

        // Add points for each interval
        for (const interval of results) {
            // Map time to X coordinate (0 to SVG_WIDTH)
            const x = (interval.start_time / duration) * SVG_WIDTH;

            // Map RMS to Y coordinate (SVG_HEIGHT is 0 RMS, 0 is max RMS)
            // Clamp RMS between 0 and maxRms
            const rmsClamped = Math.max(
                0,
                Math.min(interval.rms_amplitude, maxRms),
            );
            const y = SVG_HEIGHT - (rmsClamped / maxRms) * SVG_HEIGHT;

            // Add line segment to the point
            path += ` L ${x.toFixed(2)} ${y.toFixed(2)}`;
        }

        // Add final point at the end time of the last interval
        const lastX =
            (results[results.length - 1].end_time / duration) * SVG_WIDTH;
        path += ` L ${lastX.toFixed(2)} ${SVG_HEIGHT}`;

        // Close the path to create a filled shape
        path += " Z";

        return path;
    })();
    // --- End SVG Path Calculation ---
</script>

{#if results && results.length > 0}
    {@const progressPercent = (currentTime / duration) * 100}
    <div class="analysis-container" aria-label="Audio Volume Waveform">
        <div class="waveform-container">
            <svg
                class="waveform-svg"
                viewBox={`0 0 ${SVG_WIDTH} ${SVG_HEIGHT}`}
                preserveAspectRatio="none"
            >
                <path class="waveform-path" d={svgPathData}></path>
            </svg>
            <div
                class="progress-indicator"
                style={`left: ${progressPercent}%;`}
                aria-hidden="true"
            ></div>
        </div>
    </div>
{:else if results === null && $audioDuration > 0}
    <p class="analysis-status">Processing audio analysis...</p>
{/if}

<style>
    .analysis-container {
        display: block;
        width: 100%;
        max-width: 1200px;
        margin: 10px 0;
    }

    .waveform-container {
        position: relative;
        display: block;
        height: 100px; /* Increased height */
        background-color: #e0e0e0;
        border-radius: 3px;
        overflow: hidden;
        cursor: default;
    }

    .waveform-svg {
        display: block;
        width: 100%;
        height: 100%;
    }

    .waveform-path {
        fill: #6488ac;
        stroke-width: 0; /* No border */
    }

    .progress-indicator {
        position: absolute;
        top: 0;
        bottom: 0;
        width: 2px;
        background-color: #d9534f;
        pointer-events: none;
        z-index: 10;
    }

    .analysis-status {
        font-style: italic;
        color: #666;
        margin-top: 10px;
        text-align: center;
    }

    @media (prefers-color-scheme: dark) {
        .waveform-container {
            background-color: #4f4f4f;
        }
        .waveform-path {
            fill: #8ab4f8;
        }
        .progress-indicator {
            background-color: #f48481;
        }
        .analysis-status {
            color: #bbb;
        }
    }
</style>
