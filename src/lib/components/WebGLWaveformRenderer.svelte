<script lang="ts">
    import type { EqParams, VolumeAnalysis, WaveBin } from "$lib/types";
    import {
        createProgram,
        createShader,
        cueLineFragmentShaderSource,
        cueLineVertexShaderSource,
        playheadFragmentShaderSource,
        playheadVertexShaderSource,
        waveformFragmentShaderSource,
        waveformVertexShaderSource,
    } from "$lib/utils/webglWaveformUtils";
    import { onDestroy, onMount } from "svelte";

    let {
        volumeAnalysis = null as VolumeAnalysis | null,
        audioDuration = 0,
        currentTime = 0,
        isTrackLoaded = false,
        cuePointTime = null as number | null,
        firstBeatSec = null as number | null,
        bpm = null as number | null,
        seekAudio = (_: number) => {},
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

    // WebGL state
    let canvas = $state<HTMLCanvasElement | null>(null);
    let gl = $state<WebGL2RenderingContext | null>(null);

    // Rendering state
    interface BandResources {
        vao: WebGLVertexArrayObject | null;
        vbo: WebGLBuffer | null;
        vertexCount: number;
    }

    let waveformProgram = $state<WebGLProgram | null>(null);
    let playheadProgram = $state<WebGLProgram | null>(null);
    let cueLineProgram = $state<WebGLProgram | null>(null);

    // Cache uniform locations
    let waveformUniforms = $state<{
        timeAtPlayheadLoc: WebGLUniformLocation | null;
        zoomFactorLoc: WebGLUniformLocation | null;
        eqMultipliersLoc: WebGLUniformLocation | null;
        colorLoc: WebGLUniformLocation | null;
    }>({ timeAtPlayheadLoc: null, zoomFactorLoc: null, eqMultipliersLoc: null, colorLoc: null });

    let playheadUniforms = $state<{
        colorLoc: WebGLUniformLocation | null;
    }>({ colorLoc: null });

    let cueLineUniforms = $state<{
        ndcXLoc: WebGLUniformLocation | null;
        colorLoc: WebGLUniformLocation | null;
    }>({ ndcXLoc: null, colorLoc: null });

    let lowBand = $state<BandResources>({ vao: null, vbo: null, vertexCount: 0 });
    let midBand = $state<BandResources>({ vao: null, vbo: null, vertexCount: 0 });
    let highBand = $state<BandResources>({ vao: null, vbo: null, vertexCount: 0 });

    let playheadVAO = $state<WebGLVertexArrayObject | null>(null);
    let cueLineVAO = $state<WebGLVertexArrayObject | null>(null);

    let animationFrameId = $state<number | null>(null);
    let initialDimensionsSet = $state(false);

    // Constants
    const PLAYHEAD_COLOR = [1.0, 0.2, 0.2];
    const CUE_LINE_COLOR = [0.14, 0.55, 0.96];
    const HEIGHT_GAIN_FACTOR = 2.0;
    const PLAYHEAD_NDC_HALF_WIDTH = 0.002;
    const CUE_LINE_NDC_HALF_WIDTH = 0.002;

    // Zoom calculation constants
    const _INITIAL_NORMALIZED_TIME_ZOOM_BASE = 75.0;
    const _REFERENCE_AUDIO_DURATION_FOR_CALIBRATION = 180.0;
    const PITCH_AGNOSTIC_ZOOM_SCALAR = _INITIAL_NORMALIZED_TIME_ZOOM_BASE / _REFERENCE_AUDIO_DURATION_FOR_CALIBRATION;

    // Computed properties
    const activeMipLevel = $derived.by(() => {
        if (volumeAnalysis?.levels?.[0]?.length && volumeAnalysis.levels[0].length > 0) {
            return volumeAnalysis.levels[0];
        }
        return null;
    });

    const effectiveZoomFactor = $derived.by(() => {
        if (pitchRate === 0 || audioDuration === 0) {
            return _INITIAL_NORMALIZED_TIME_ZOOM_BASE;
        }
        return (PITCH_AGNOSTIC_ZOOM_SCALAR * audioDuration) / pitchRate;
    });


    // Handle canvas click for seeking
    function handleClick(event: MouseEvent) {
        if (!gl || !canvas || audioDuration <= 0 || !isTrackLoaded) return;

        const rect = canvas.getBoundingClientRect();
        const clickXInCanvas = event.clientX - rect.left;
        const canvasWidth = canvas.clientWidth;

        const clickedNdcX = (clickXInCanvas / canvasWidth) * 2.0 - 1.0;
        const normalizedCenterTime = audioDuration > 0 ? currentTime / audioDuration : 0;
        let normalizedTargetTime = normalizedCenterTime + clickedNdcX / effectiveZoomFactor;

        normalizedTargetTime = Math.max(0, Math.min(normalizedTargetTime, 1.0));
        const targetTimeSeconds = normalizedTargetTime * audioDuration;
        const clampedTargetTimeSeconds = Math.max(0, Math.min(targetTimeSeconds, audioDuration));

        seekAudio(clampedTargetTimeSeconds);
    }

    function setupWebGLContext(canvas: HTMLCanvasElement): WebGL2RenderingContext | null {
        const context = canvas.getContext("webgl2");
        if (!context) {
            console.error("WebGL2 not supported");
            return null;
        }

        context.enable(context.BLEND);
        context.blendFunc(context.SRC_ALPHA, context.ONE);
        return context;
    }

    function initPrograms() {
        if (!gl) return false;

        // Waveform program
        const wfVert = createShader(gl, gl.VERTEX_SHADER, waveformVertexShaderSource);
        const wfFrag = createShader(gl, gl.FRAGMENT_SHADER, waveformFragmentShaderSource);
        if (wfVert && wfFrag) {
            waveformProgram = createProgram(gl, wfVert, wfFrag);
            gl.deleteShader(wfVert);
            gl.deleteShader(wfFrag);

            // Cache waveform uniform locations
            if (waveformProgram) {
                waveformUniforms.timeAtPlayheadLoc = gl.getUniformLocation(waveformProgram, "u_normalized_time_at_playhead");
                waveformUniforms.zoomFactorLoc = gl.getUniformLocation(waveformProgram, "u_zoom_factor");
                waveformUniforms.eqMultipliersLoc = gl.getUniformLocation(waveformProgram, "u_eq_multipliers");
                waveformUniforms.colorLoc = gl.getUniformLocation(waveformProgram, "u_waveform_color_with_alpha");
            }
        }

        // Playhead program
        const phVert = createShader(gl, gl.VERTEX_SHADER, playheadVertexShaderSource);
        const phFrag = createShader(gl, gl.FRAGMENT_SHADER, playheadFragmentShaderSource);
        if (phVert && phFrag) {
            playheadProgram = createProgram(gl, phVert, phFrag);
            gl.deleteShader(phVert);
            gl.deleteShader(phFrag);

            // Cache playhead uniform locations
            if (playheadProgram) {
                playheadUniforms.colorLoc = gl.getUniformLocation(playheadProgram, "u_playhead_color");
            }
        }

        // Cue line program
        const clVert = createShader(gl, gl.VERTEX_SHADER, cueLineVertexShaderSource);
        const clFrag = createShader(gl, gl.FRAGMENT_SHADER, cueLineFragmentShaderSource);
        if (clVert && clFrag) {
            cueLineProgram = createProgram(gl, clVert, clFrag);
            gl.deleteShader(clVert);
            gl.deleteShader(clFrag);

            // Cache cue line uniform locations
            if (cueLineProgram) {
                cueLineUniforms.ndcXLoc = gl.getUniformLocation(cueLineProgram, "u_cue_line_ndc_x");
                cueLineUniforms.colorLoc = gl.getUniformLocation(cueLineProgram, "u_cue_line_color");
            }
        }

        return true;
    }

    function initBuffers() {
        if (!gl) return;

        // Initialize waveform VAOs and VBOs
        lowBand.vao = gl.createVertexArray();
        lowBand.vbo = gl.createBuffer();
        midBand.vao = gl.createVertexArray();
        midBand.vbo = gl.createBuffer();
        highBand.vao = gl.createVertexArray();
        highBand.vbo = gl.createBuffer();

        // Initialize playhead
        playheadVAO = gl.createVertexArray();
        const playheadVBO = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, playheadVBO);
        const playheadVerts = new Float32Array([
            -PLAYHEAD_NDC_HALF_WIDTH, 1.0,
            PLAYHEAD_NDC_HALF_WIDTH, 1.0,
            -PLAYHEAD_NDC_HALF_WIDTH, -1.0,
            PLAYHEAD_NDC_HALF_WIDTH, -1.0,
        ]);
        gl.bufferData(gl.ARRAY_BUFFER, playheadVerts, gl.STATIC_DRAW);
        gl.bindVertexArray(playheadVAO);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

        // Initialize cue line
        cueLineVAO = gl.createVertexArray();
        const cueLineVBO = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, cueLineVBO);
        const cueLineVerts = new Float32Array([
            -CUE_LINE_NDC_HALF_WIDTH, 1.0,
            CUE_LINE_NDC_HALF_WIDTH, 1.0,
            -CUE_LINE_NDC_HALF_WIDTH, -1.0,
            CUE_LINE_NDC_HALF_WIDTH, -1.0,
        ]);
        gl.bufferData(gl.ARRAY_BUFFER, cueLineVerts, gl.STATIC_DRAW);
        gl.bindVertexArray(cueLineVAO);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

        gl.bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
    }

    function updateWaveformGeometry() {
        const currentActiveMip = activeMipLevel;
        if (!gl || !currentActiveMip || !volumeAnalysis || audioDuration <= 0) {
            lowBand.vertexCount = 0;
            midBand.vertexCount = 0;
            highBand.vertexCount = 0;
            return;
        }

        const bins = currentActiveMip;
        const maxRms = volumeAnalysis.maxBandEnergy > 0 ? volumeAnalysis.maxBandEnergy : 0.0001;

        const vertexDataLow: number[] = [];
        const vertexDataMid: number[] = [];
        const vertexDataHigh: number[] = [];

        bins.forEach((bin: WaveBin, index: number) => {
            const binDurationForThisMip = audioDuration / bins.length;
            const timeSec = index * binDurationForThisMip;
            const normalizedTimeX = audioDuration > 0 ? timeSec / audioDuration : 0;

            const yTopLow = Math.min(1.0, (bin.low / maxRms) * HEIGHT_GAIN_FACTOR);
            const yBottomLow = -yTopLow;
            vertexDataLow.push(normalizedTimeX, yBottomLow, 0.0);
            vertexDataLow.push(normalizedTimeX, yTopLow, 0.0);

            const yTopMid = Math.min(1.0, (bin.mid / maxRms) * HEIGHT_GAIN_FACTOR);
            const yBottomMid = -yTopMid;
            vertexDataMid.push(normalizedTimeX, yBottomMid, 1.0);
            vertexDataMid.push(normalizedTimeX, yTopMid, 1.0);

            const yTopHigh = Math.min(1.0, (bin.high / maxRms) * HEIGHT_GAIN_FACTOR);
            const yBottomHigh = -yTopHigh;
            vertexDataHigh.push(normalizedTimeX, yBottomHigh, 2.0);
            vertexDataHigh.push(normalizedTimeX, yTopHigh, 2.0);
        });

        lowBand.vertexCount = vertexDataLow.length / 3;
        midBand.vertexCount = vertexDataMid.length / 3;
        highBand.vertexCount = vertexDataHigh.length / 3;

        setupBandGeometry(lowBand.vao!, lowBand.vbo!, vertexDataLow);
        setupBandGeometry(midBand.vao!, midBand.vbo!, vertexDataMid);
        setupBandGeometry(highBand.vao!, highBand.vbo!, vertexDataHigh);
    }

    function setupBandGeometry(vao: WebGLVertexArrayObject, vbo: WebGLBuffer, vertexData: number[]) {
        if (!gl) return;

        gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(vertexData), gl.STATIC_DRAW);

        gl.bindVertexArray(vao);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 1, gl.FLOAT, false, 3 * Float32Array.BYTES_PER_ELEMENT, 0);
        gl.enableVertexAttribArray(1);
        gl.vertexAttribPointer(1, 1, gl.FLOAT, false, 3 * Float32Array.BYTES_PER_ELEMENT, 1 * Float32Array.BYTES_PER_ELEMENT);
        gl.enableVertexAttribArray(2);
        gl.vertexAttribPointer(2, 1, gl.FLOAT, false, 3 * Float32Array.BYTES_PER_ELEMENT, 2 * Float32Array.BYTES_PER_ELEMENT);

        gl.bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
    }

    function render() {
        if (!gl || !canvas || !initialDimensionsSet) return;
        if (canvas.width === 0 || canvas.height === 0) return;

        gl.clearColor(0.0, 0.0, 0.0, 0.0);
        gl.clear(gl.COLOR_BUFFER_BIT);

        // Draw waveform bands
        if (waveformProgram && lowBand.vao && midBand.vao && highBand.vao &&
            lowBand.vertexCount > 0 && midBand.vertexCount > 0 && highBand.vertexCount > 0 &&
            waveformUniforms.timeAtPlayheadLoc && waveformUniforms.zoomFactorLoc &&
            waveformUniforms.eqMultipliersLoc && waveformUniforms.colorLoc) {

            gl.useProgram(waveformProgram);

            const normalizedTimeAtPlayhead = audioDuration > 0 ? currentTime / audioDuration : 0;
            gl.uniform1f(waveformUniforms.timeAtPlayheadLoc, normalizedTimeAtPlayhead);
            gl.uniform1f(waveformUniforms.zoomFactorLoc, effectiveZoomFactor);

            const lowMultiplier = Math.pow(10, eqParams.lowGainDb / 20);
            const midMultiplier = Math.pow(10, eqParams.midGainDb / 20);
            const highMultiplier = Math.pow(10, eqParams.highGainDb / 20);
            gl.uniform3f(waveformUniforms.eqMultipliersLoc, lowMultiplier, midMultiplier, highMultiplier);

            const faderAlpha = Math.max(0, Math.min(1, faderLevel));

            // Draw low band
            gl.uniform4f(waveformUniforms.colorLoc, lowBandColor[0], lowBandColor[1], lowBandColor[2], faderAlpha);
            gl.bindVertexArray(lowBand.vao);
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, lowBand.vertexCount);

            // Draw mid band
            gl.uniform4f(waveformUniforms.colorLoc, midBandColor[0], midBandColor[1], midBandColor[2], faderAlpha);
            gl.bindVertexArray(midBand.vao);
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, midBand.vertexCount);

            // Draw high band
            gl.uniform4f(waveformUniforms.colorLoc, highBandColor[0], highBandColor[1], highBandColor[2], faderAlpha);
            gl.bindVertexArray(highBand.vao);
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, highBand.vertexCount);

            gl.bindVertexArray(null);
        }

        // Draw playhead
        if (playheadProgram && playheadVAO && playheadUniforms.colorLoc) {
            gl.useProgram(playheadProgram);
            gl.uniform3fv(playheadUniforms.colorLoc, PLAYHEAD_COLOR);
            gl.bindVertexArray(playheadVAO);
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
            gl.bindVertexArray(null);
        }

        // Draw cue line
        if (cuePointTime !== null && cueLineProgram && cueLineVAO && audioDuration > 0 &&
            cueLineUniforms.ndcXLoc && cueLineUniforms.colorLoc) {
            gl.useProgram(cueLineProgram);

            const normalizedCueTime = cuePointTime / audioDuration;
            const normalizedPlayheadCenterTime = currentTime / audioDuration;
            const cueNdcX = (normalizedCueTime - normalizedPlayheadCenterTime) * effectiveZoomFactor;

            if (cueNdcX >= -1.1 && cueNdcX <= 1.1) {
                gl.uniform1f(cueLineUniforms.ndcXLoc, cueNdcX);
                gl.uniform3fv(cueLineUniforms.colorLoc, CUE_LINE_COLOR);
                gl.bindVertexArray(cueLineVAO);
                gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
                gl.bindVertexArray(null);
            }
        }

        // Draw beat lines
        if (firstBeatSec !== null && bpm !== null && cueLineProgram && cueLineVAO &&
            audioDuration > 0 && isTrackLoaded &&
            cueLineUniforms.ndcXLoc && cueLineUniforms.colorLoc) {

            gl.useProgram(cueLineProgram);
            const originalBeatInterval = 60.0 / bpm;
            const normalizedPlayheadCenterTime = currentTime / audioDuration;

            let beatCount = 0;
            for (let baseBeatTimeSec = firstBeatSec; baseBeatTimeSec < audioDuration + originalBeatInterval; baseBeatTimeSec += originalBeatInterval) {
                if (baseBeatTimeSec < 0) continue;

                const normalizedBeat = baseBeatTimeSec / audioDuration;
                const beatNdcX = (normalizedBeat - normalizedPlayheadCenterTime) * effectiveZoomFactor;

                if (beatNdcX >= -1.1 && beatNdcX <= 1.1) {
                    gl.uniform1f(cueLineUniforms.ndcXLoc, beatNdcX);
                    gl.uniform3fv(cueLineUniforms.colorLoc, [1.0, 0.55, 0.0]); // Orange
                    gl.bindVertexArray(cueLineVAO);
                    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
                    gl.bindVertexArray(null);
                    beatCount++;
                }
            }
        }
    }

    function resizeCanvas() {
        if (!gl || !canvas) return;

        const newWidth = canvas.clientWidth;
        const newHeight = canvas.clientHeight;

        if (canvas.width !== newWidth || canvas.height !== newHeight) {
            canvas.width = newWidth;
            canvas.height = newHeight;
            gl.viewport(0, 0, canvas.width, canvas.height);
        }

        if (newWidth > 0 && newHeight > 0 && !initialDimensionsSet) {
            initialDimensionsSet = true;
        }
    }

    function continuousRender() {
        if (!isTrackLoaded || !gl || !initialDimensionsSet) {
            animationFrameId = null;
            return;
        }

        render();
        animationFrameId = requestAnimationFrame(continuousRender);
    }

    // Effect to update geometry ONLY when the track/analysis actually changes
    // Use regular variables to avoid proxy comparison issues
    let lastProcessedVolumeAnalysis: VolumeAnalysis | null = null;
    let lastProcessedAudioDuration = 0;

    $effect(() => {
        const newVolumeAnalysis = volumeAnalysis;
        const newAudioDuration = audioDuration;

        const analysisObjectChanged = newVolumeAnalysis !== lastProcessedVolumeAnalysis;
        const durationChangedSignificantly =
            newAudioDuration !== lastProcessedAudioDuration && newAudioDuration > 0;

        if (analysisObjectChanged || (newVolumeAnalysis && durationChangedSignificantly)) {
            lastProcessedVolumeAnalysis = newVolumeAnalysis;
            lastProcessedAudioDuration = newAudioDuration;

            if (activeMipLevel && gl && newAudioDuration > 0) {
                updateWaveformGeometry();
            } else {
                lowBand.vertexCount = 0;
                midBand.vertexCount = 0;
                highBand.vertexCount = 0;
            }
        } else if (!newVolumeAnalysis && lastProcessedVolumeAnalysis !== null) {
            // Explicitly clear if volumeAnalysis becomes null (track unloaded)
            lowBand.vertexCount = 0;
            midBand.vertexCount = 0;
            highBand.vertexCount = 0;
            lastProcessedVolumeAnalysis = null;
            lastProcessedAudioDuration = 0;
        }
    });

    $effect(() => {
        if (isTrackLoaded && initialDimensionsSet && gl) {
            if (animationFrameId === null) {
                animationFrameId = requestAnimationFrame(continuousRender);
            }
        } else {
            if (animationFrameId !== null) {
                cancelAnimationFrame(animationFrameId);
                animationFrameId = null;
            }
        }
    });

    onMount(() => {
        if (canvas) {
            gl = setupWebGLContext(canvas);
            if (gl) {
                initPrograms();
                initBuffers();
                resizeCanvas();

                const resizeObserver = new ResizeObserver(() => {
                    resizeCanvas();
                    if (gl && initialDimensionsSet && animationFrameId === null) {
                        render();
                    }
                });
                resizeObserver.observe(canvas);

                return () => {
                    resizeObserver.disconnect();
                };
            }
        }
    });

    onDestroy(() => {
        if (animationFrameId !== null) {
            cancelAnimationFrame(animationFrameId);
        }

        if (gl) {
            // Cleanup WebGL resources
            if (waveformProgram) gl.deleteProgram(waveformProgram);
            if (playheadProgram) gl.deleteProgram(playheadProgram);
            if (cueLineProgram) gl.deleteProgram(cueLineProgram);

            if (lowBand.vao) gl.deleteVertexArray(lowBand.vao);
            if (lowBand.vbo) gl.deleteBuffer(lowBand.vbo);
            if (midBand.vao) gl.deleteVertexArray(midBand.vao);
            if (midBand.vbo) gl.deleteBuffer(midBand.vbo);
            if (highBand.vao) gl.deleteVertexArray(highBand.vao);
            if (highBand.vbo) gl.deleteBuffer(highBand.vbo);

            if (playheadVAO) gl.deleteVertexArray(playheadVAO);
            if (cueLineVAO) gl.deleteVertexArray(cueLineVAO);
        }
    });
</script>

<canvas
    bind:this={canvas}
    style="display: block; width: 100%; height: 100%;"
    onclick={handleClick}
    class:is-clickable={isTrackLoaded}
></canvas>

<style>
    canvas.is-clickable {
        cursor: pointer;
    }
</style>
