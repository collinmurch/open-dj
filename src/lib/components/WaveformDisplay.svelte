<script lang="ts">
    import type { EqParams, VolumeAnalysis } from "$lib/types";
    import WebGLWaveformRenderer from "./WebGLWaveformRenderer.svelte";

    let {
        volumeAnalysis = null as VolumeAnalysis | null,
        audioDuration = 0,
        currentTime = 0,
        isPlaying = false,
        isAnalysisPending = false,
        isTrackLoaded = false,
        cuePointTime = null as number | null,
        firstBeatSec = null as number | null,
        bpm = null as number | null,
        seekAudio = (time: number) => {},
        lowBandColor = [0.1, 0.2, 0.7] as [number, number, number],
        midBandColor = [0.2, 0.7, 0.2] as [number, number, number],
        highBandColor = [0.3, 0.7, 0.9] as [number, number, number],
        eqParams = {
            lowGainDb: 0.0,
            midGainDb: 0.0,
            highGainDb: 0.0,
        } as EqParams,
        faderLevel = 1.0,
        pitchRate = 1.0,
    }: {
        volumeAnalysis: VolumeAnalysis | null;
        audioDuration: number;
        currentTime?: number;
        isPlaying?: boolean;
        isAnalysisPending?: boolean;
        isTrackLoaded?: boolean;
        cuePointTime?: number | null;
        firstBeatSec?: number | null;
        bpm?: number | null;
        seekAudio?: (time: number) => void;
        lowBandColor?: [number, number, number];
        midBandColor?: [number, number, number];
        highBandColor?: [number, number, number];
        eqParams?: EqParams;
        faderLevel?: number;
        pitchRate?: number;
    } = $props();

    // Check if we have valid waveform data to display
    const hasWaveformData = $derived(
        volumeAnalysis?.levels?.[0]?.length ? volumeAnalysis.levels[0].length > 0 : false
    );

    // Determine what status message to show
    const statusMessage = $derived.by(() => {
        if (!isTrackLoaded) {
            return "Load audio to see waveform";
        } else if (isAnalysisPending) {
            return "Analyzing audio...";
        } else if (!hasWaveformData) {
            return "Waveform data not available";
        }
        return null;
    });

    const showRenderer = $derived(
        isTrackLoaded && hasWaveformData
    );
</script>

<div class="waveform-display-container">
    {#if showRenderer}
        <WebGLWaveformRenderer
            {volumeAnalysis}
            {audioDuration}
            {currentTime}
            {isTrackLoaded}
            {cuePointTime}
            {firstBeatSec}
            {bpm}
            {seekAudio}
            {lowBandColor}
            {midBandColor}
            {highBandColor}
            {eqParams}
            {faderLevel}
            {pitchRate}
        />
    {/if}

    {#if statusMessage}
        <div class="status-message" class:loading={isAnalysisPending}>
            {statusMessage}
        </div>
    {/if}
</div>

<style>
    .waveform-display-container {
        position: relative;
        min-height: 80px;
        width: 100%;
        height: 100%;
        overflow: hidden;
    }

    .status-message {
        position: absolute;
        top: 0;
        left: 0;
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 100%;
        font-style: italic;
        color: #555;
        background-color: rgba(30, 30, 30, 0.8);
        border-radius: 4px;
        pointer-events: none;
    }

    .status-message.loading {
        color: #ccc;
    }

    @media (prefers-color-scheme: light) {
        .status-message {
            color: #555;
            background-color: rgba(233, 233, 233, 0.8);
        }

        .status-message.loading {
            color: #333;
        }
    }
</style>
