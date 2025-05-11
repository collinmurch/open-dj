<script lang="ts">
    import type { VolumeAnalysis, WaveBin, EqParams } from "$lib/types";
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

    // --- Component Props ---
    let {
        // Core Data & State
        volumeAnalysis = null as VolumeAnalysis | null,
        audioDuration = 0,
        currentTime = 0,
        isPlaying = false,
        isAnalysisPending = false,
        isTrackLoaded = false,
        cuePointTime = null as number | null,

        // Callbacks
        seekAudio = (time: number) => {},

        // Appearance & Styling
        lowBandColor = [0.1, 0.2, 0.7] as [number, number, number],
        midBandColor = [0.2, 0.7, 0.2] as [number, number, number],
        highBandColor = [0.3, 0.7, 0.9] as [number, number, number],

        // EQ & Fader Levels
        eqParams = {
            lowGainDb: 0.0,
            midGainDb: 0.0,
            highGainDb: 0.0,
        } as EqParams,
        faderLevel = 1.0,
        pitchRate = 1.0,
    }: {
        // Core Data & State
        volumeAnalysis: VolumeAnalysis | null;
        audioDuration: number;
        currentTime?: number;
        isPlaying?: boolean;
        isAnalysisPending?: boolean;
        isTrackLoaded?: boolean;
        cuePointTime?: number | null;

        // Callbacks
        seekAudio?: (time: number) => void;

        // Appearance & Styling
        lowBandColor?: [number, number, number];
        midBandColor?: [number, number, number];
        highBandColor?: [number, number, number];

        // EQ & Fader Levels
        eqParams?: EqParams;
        faderLevel?: number;
        pitchRate?: number;
    } = $props();

    // --- WebGL State & Refs ---
    let glContext = $state<{
        canvas: HTMLCanvasElement | null;
        ctx: WebGL2RenderingContext | null;
    }>({
        canvas: null,
        ctx: null,
    });

    // Band/Waveform resources
    interface BandResources {
        vao: WebGLVertexArrayObject | null;
        vbo: WebGLBuffer | null;
        vertexCount: number;
    }

    let waveformRendering = $state<{
        program: WebGLProgram | null;
        low: BandResources;
        mid: BandResources;
        high: BandResources;
        uniforms: {
            colorLoc: WebGLUniformLocation | null;
            timeAtPlayheadLoc: WebGLUniformLocation | null;
            zoomFactorLoc: WebGLUniformLocation | null;
        };
    }>({
        program: null,
        low: { vao: null, vbo: null, vertexCount: 0 },
        mid: { vao: null, vbo: null, vertexCount: 0 },
        high: { vao: null, vbo: null, vertexCount: 0 },
        uniforms: {
            colorLoc: null,
            timeAtPlayheadLoc: null,
            zoomFactorLoc: null,
        },
    });

    // Playhead resources
    let playheadRendering = $state<{
        program: WebGLProgram | null;
        vao: WebGLVertexArrayObject | null;
        vbo: WebGLBuffer | null;
        uniforms: {
            colorLoc: WebGLUniformLocation | null;
        };
    }>({
        program: null,
        vao: null,
        vbo: null,
        uniforms: {
            colorLoc: null,
        },
    });

    // Cue Line resources
    let cueLineRendering = $state<{
        program: WebGLProgram | null;
        vao: WebGLVertexArrayObject | null;
        vbo: WebGLBuffer | null;
        uniforms: {
            ndcXLoc: WebGLUniformLocation | null;
            colorLoc: WebGLUniformLocation | null;
        };
    }>({
        program: null,
        vao: null,
        vbo: null,
        uniforms: {
            ndcXLoc: null,
            colorLoc: null,
        },
    });

    let initialDimensionsSet = $state(false);

    // --- Interpolation State ---
    let interpolationCtx = $state({
        internalDisplayTime: 0,
        lastHostUpdateTime: performance.now(),
        lastHostTimeValue: 0,
        hostIsPlaying: false,
    });

    // --- Animation State ---
    let seekAnimationCtx = $state({
        isActive: false,
        startTime: 0,
        startDisplayTime: 0,
        targetDisplayTime: 0,
    });

    const SEEK_ANIMATION_DURATION_MS = 80;
    const SEEK_TRIGGER_THRESHOLD_SECONDS = 0.2; // If currentTime jumps by more than this, animate

    // --- Constants ---
    const PLAYHEAD_COLOR = [1.0, 0.2, 0.2]; // A visible red
    const CUE_LINE_COLOR = [0.14, 0.55, 0.96]; // Changed to selection blue (approx hsl(210, 90%, 55%))
    const INITIAL_ZOOM_FACTOR = 75.0;
    const HEIGHT_GAIN_FACTOR = 2.0;
    const PLAYHEAD_NDC_HALF_WIDTH = 0.002; // For a total width of 0.008 NDC
    const CUE_LINE_NDC_HALF_WIDTH = 0.002; // For a total width of 0.008 NDC

    // --- State for controlling geometry updates ---
    let lastProcessedVolumeAnalysis: VolumeAnalysis | null | undefined =
        $state(undefined);
    let lastProcessedAudioDuration: number = $state(NaN);
    let lastProcessedLowGain: number = $state(NaN);
    let lastProcessedMidGain: number = $state(NaN);
    let lastProcessedHighGain: number = $state(NaN);
    let animationFrameId: number | null = $state(null);

    // --- Derived values ---
    const activeMipLevel = $derived(() => {
        if (
            volumeAnalysis &&
            volumeAnalysis.levels &&
            volumeAnalysis.levels[0] &&
            volumeAnalysis.levels[0].length > 0
        ) {
            return volumeAnalysis.levels[0];
        }
        return null;
    });

    const effectiveZoomFactor = $derived(() => {
        if (pitchRate === 0) return INITIAL_ZOOM_FACTOR;
        return INITIAL_ZOOM_FACTOR / pitchRate;
    });

    // Effect to update geometry ONLY when the track/analysis actually changes
    $effect(() => {
        const newVolumeAnalysis = volumeAnalysis;
        const newAudioDuration = audioDuration;
        // Capture EQ gains for dependency tracking by $effect
        const currentLowGain = eqParams.lowGainDb;
        const currentMidGain = eqParams.midGainDb;
        const currentHighGain = eqParams.highGainDb;

        const analysisObjectChanged =
            newVolumeAnalysis !== lastProcessedVolumeAnalysis;
        const durationChangedSignificantly =
            newAudioDuration !== lastProcessedAudioDuration &&
            newAudioDuration > 0;
        const eqGainsChanged =
            currentLowGain !== lastProcessedLowGain ||
            currentMidGain !== lastProcessedMidGain ||
            currentHighGain !== lastProcessedHighGain;

        if (
            analysisObjectChanged ||
            (newVolumeAnalysis && durationChangedSignificantly) ||
            (newVolumeAnalysis && eqGainsChanged) // Added eqGainsChanged condition
        ) {
            lastProcessedVolumeAnalysis = newVolumeAnalysis;
            lastProcessedAudioDuration = newAudioDuration;
            lastProcessedLowGain = currentLowGain;
            lastProcessedMidGain = currentMidGain;
            lastProcessedHighGain = currentHighGain;

            if (activeMipLevel() && glContext.ctx && newAudioDuration > 0) {
                updateWaveformGeometry(); // Uses activeMipLevel() and newAudioDuration implicitly
                // render() will be called by the continuousRender loop if active
            } else {
                waveformRendering.low.vertexCount = 0;
                waveformRendering.mid.vertexCount = 0;
                waveformRendering.high.vertexCount = 0;
            }
        } else if (!newVolumeAnalysis && lastProcessedVolumeAnalysis !== null) {
            // Explicitly clear if volumeAnalysis becomes null (track unloaded)
            waveformRendering.low.vertexCount = 0;
            waveformRendering.mid.vertexCount = 0;
            waveformRendering.high.vertexCount = 0;
            lastProcessedVolumeAnalysis = null;
            lastProcessedAudioDuration = NaN;
        }
    });

    // Effect to sync with host state for interpolation
    $effect(() => {
        const hostTimeProp = currentTime; // Prop value from playerStore
        const hostPlayingStatusProp = isPlaying; // Prop value

        const timeDelta = Math.abs(
            hostTimeProp - interpolationCtx.lastHostTimeValue,
        );

        if (
            timeDelta > SEEK_TRIGGER_THRESHOLD_SECONDS &&
            !seekAnimationCtx.isActive
        ) {
            // Likely a seek or significant jump, trigger animation
            console.log(
                `[WebGLWaveform] Animation triggered. From: ${$state.snapshot(interpolationCtx.internalDisplayTime).toFixed(3)}s to: ${hostTimeProp.toFixed(3)}s`,
            );
            seekAnimationCtx.isActive = true;
            seekAnimationCtx.startTime = performance.now();
            seekAnimationCtx.startDisplayTime =
                interpolationCtx.internalDisplayTime; // Current visual position
            seekAnimationCtx.targetDisplayTime = hostTimeProp; // Target from prop
        } else if (!seekAnimationCtx.isActive) {
            // Not animating, so update baseline for normal interpolation or static display
            interpolationCtx.internalDisplayTime = hostTimeProp;
        }
        // Always update these for the next frame's interpolation logic or delta check
        interpolationCtx.lastHostTimeValue = hostTimeProp;
        interpolationCtx.lastHostUpdateTime = performance.now();
        interpolationCtx.hostIsPlaying = hostPlayingStatusProp;
    });

    // Effect to manage the continuous render loop
    $effect(() => {
        if (isTrackLoaded && initialDimensionsSet && glContext.ctx) {
            if (animationFrameId === null) {
                console.log("[WebGLWaveform] Starting continuous render loop.");
                animationFrameId = requestAnimationFrame(continuousRender);
            }
        } else {
            if (animationFrameId !== null) {
                console.log("[WebGLWaveform] Stopping continuous render loop.");
                cancelAnimationFrame(animationFrameId);
                animationFrameId = null;
            }
        }
    });

    // --- Event Handlers ---
    function handleWaveformClick(event: MouseEvent) {
        if (
            !glContext.ctx ||
            !glContext.canvas ||
            audioDuration <= 0 ||
            !isTrackLoaded
        ) {
            return;
        }

        const rect = glContext.canvas.getBoundingClientRect();
        const clickXInCanvas = event.clientX - rect.left;
        const canvasWidth = glContext.canvas.clientWidth;

        // Convert click X to NDC space (-1 to 1)
        const clickedNdcX = (clickXInCanvas / canvasWidth) * 2.0 - 1.0;

        // Current normalized time at the center of the view (playhead position)
        const normalizedCenterTime =
            audioDuration > 0
                ? interpolationCtx.internalDisplayTime / audioDuration
                : 0;

        // Calculate target normalized time based on click offset from center
        let normalizedTargetTime =
            normalizedCenterTime + clickedNdcX / INITIAL_ZOOM_FACTOR;

        // Clamp normalized time to [0, 1]
        normalizedTargetTime = Math.max(0, Math.min(normalizedTargetTime, 1.0));

        const targetTimeSeconds = normalizedTargetTime * audioDuration;

        // Ensure final time is also clamped, just in case of floating point issues
        const clampedTargetTimeSeconds = Math.max(
            0,
            Math.min(targetTimeSeconds, audioDuration),
        );

        console.log(
            `[WebGLWaveform] Clicked. Target time: ${clampedTargetTimeSeconds.toFixed(3)}s`,
        );
        seekAudio(clampedTargetTimeSeconds);
    }

    function setupWebGLContext(
        canvas: HTMLCanvasElement,
    ): WebGL2RenderingContext | null {
        const context = canvas.getContext("webgl2");
        if (!context) {
            console.error("WebGL2 not supported or context creation failed");
            return null;
        }

        context.enable(context.BLEND);
        context.blendFunc(context.SRC_ALPHA, context.ONE);
        return context;
    }

    function initWaveformResources(gl: WebGL2RenderingContext): boolean {
        const wfVert = createShader(
            gl,
            gl.VERTEX_SHADER,
            waveformVertexShaderSource,
        );
        const wfFrag = createShader(
            gl,
            gl.FRAGMENT_SHADER,
            waveformFragmentShaderSource,
        );
        if (!wfVert || !wfFrag) return false;

        waveformRendering.program = createProgram(gl, wfVert, wfFrag);
        gl.deleteShader(wfVert);
        gl.deleteShader(wfFrag);

        if (!waveformRendering.program) {
            console.error("Failed to create waveform program");
            return false;
        }
        waveformRendering.uniforms.colorLoc = gl.getUniformLocation(
            waveformRendering.program,
            "u_waveform_color_with_alpha",
        );
        waveformRendering.uniforms.timeAtPlayheadLoc = gl.getUniformLocation(
            waveformRendering.program,
            "u_normalized_time_at_playhead",
        );
        waveformRendering.uniforms.zoomFactorLoc = gl.getUniformLocation(
            waveformRendering.program,
            "u_zoom_factor",
        );
        waveformRendering.low.vao = gl.createVertexArray();
        waveformRendering.low.vbo = gl.createBuffer();
        waveformRendering.mid.vao = gl.createVertexArray();
        waveformRendering.mid.vbo = gl.createBuffer();
        waveformRendering.high.vao = gl.createVertexArray();
        waveformRendering.high.vbo = gl.createBuffer();
        return true;
    }

    function initPlayheadResources(gl: WebGL2RenderingContext): boolean {
        const phVert = createShader(
            gl,
            gl.VERTEX_SHADER,
            playheadVertexShaderSource,
        );
        const phFrag = createShader(
            gl,
            gl.FRAGMENT_SHADER,
            playheadFragmentShaderSource,
        );
        if (!phVert || !phFrag) return false;

        playheadRendering.program = createProgram(gl, phVert, phFrag);
        gl.deleteShader(phVert);
        gl.deleteShader(phFrag);

        if (!playheadRendering.program) {
            console.error("Failed to create playhead program");
            return false;
        }
        playheadRendering.uniforms.colorLoc = gl.getUniformLocation(
            playheadRendering.program,
            "u_playhead_color",
        );
        playheadRendering.vao = gl.createVertexArray();
        playheadRendering.vbo = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, playheadRendering.vbo);
        const playheadVerts = new Float32Array([
            -PLAYHEAD_NDC_HALF_WIDTH,
            1.0, // Top-left
            PLAYHEAD_NDC_HALF_WIDTH,
            1.0, // Top-right
            -PLAYHEAD_NDC_HALF_WIDTH,
            -1.0, // Bottom-left
            PLAYHEAD_NDC_HALF_WIDTH,
            -1.0, // Bottom-right
        ]);
        gl.bufferData(gl.ARRAY_BUFFER, playheadVerts, gl.STATIC_DRAW);
        gl.bindVertexArray(playheadRendering.vao);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
        gl.bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
        return true;
    }

    function initCueLineResources(gl: WebGL2RenderingContext): boolean {
        const clVert = createShader(
            gl,
            gl.VERTEX_SHADER,
            cueLineVertexShaderSource,
        );
        const clFrag = createShader(
            gl,
            gl.FRAGMENT_SHADER,
            cueLineFragmentShaderSource,
        );
        if (!clVert || !clFrag) return false;

        cueLineRendering.program = createProgram(gl, clVert, clFrag);
        gl.deleteShader(clVert);
        gl.deleteShader(clFrag);

        if (!cueLineRendering.program) {
            console.error("Failed to create cue line program");
            return false;
        }
        cueLineRendering.uniforms.ndcXLoc = gl.getUniformLocation(
            cueLineRendering.program,
            "u_cue_line_ndc_x",
        );
        cueLineRendering.uniforms.colorLoc = gl.getUniformLocation(
            cueLineRendering.program,
            "u_cue_line_color",
        );
        cueLineRendering.vao = gl.createVertexArray();
        cueLineRendering.vbo = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, cueLineRendering.vbo);
        const cueLineVerts = new Float32Array([
            -CUE_LINE_NDC_HALF_WIDTH,
            1.0, // Top-left
            CUE_LINE_NDC_HALF_WIDTH,
            1.0, // Top-right
            -CUE_LINE_NDC_HALF_WIDTH,
            -1.0, // Bottom-left
            CUE_LINE_NDC_HALF_WIDTH,
            -1.0, // Bottom-right
        ]);
        gl.bufferData(gl.ARRAY_BUFFER, cueLineVerts, gl.STATIC_DRAW);
        gl.bindVertexArray(cueLineRendering.vao);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
        gl.bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
        return true;
    }

    // --- WebGL Initialization (Main Orchestrator) ---
    function initWebGL() {
        if (!glContext.canvas) return;
        const gl = setupWebGLContext(glContext.canvas);
        if (!gl) return;
        glContext.ctx = gl;

        if (!initWaveformResources(gl)) {
            console.error("Waveform resource initialization failed.");
        }
        if (!initPlayheadResources(gl)) {
            console.error("Playhead resource initialization failed.");
        }
        if (!initCueLineResources(gl)) {
            console.error("Cue line resource initialization failed.");
        }
        handleResize();
    }

    function resizeCanvas() {
        if (!glContext.ctx || !glContext.canvas) return;
        const gl = glContext.ctx;
        const canvas = glContext.canvas;

        const newWidth = canvas.clientWidth;
        const newHeight = canvas.clientHeight;

        if (canvas.width !== newWidth || canvas.height !== newHeight) {
            canvas.width = newWidth;
            canvas.height = newHeight;
            gl.viewport(0, 0, canvas.width, canvas.height);
            console.log(
                `[WebGLWaveform] Canvas resized to: ${canvas.width}x${canvas.height}.`,
            );
        }
        if (newWidth > 0 && newHeight > 0 && !initialDimensionsSet) {
            initialDimensionsSet = true;
        }
        // render() is now primarily driven by continuousRender or geometry updates
    }

    // --- Data Preparation for Waveform (uses audioDuration from prop) ---
    function updateWaveformGeometry() {
        const currentAudioDuration = audioDuration;
        const currentActiveMip = activeMipLevel();
        const gl = glContext.ctx;

        if (
            !gl ||
            !waveformRendering.low.vao ||
            !waveformRendering.low.vbo ||
            !waveformRendering.mid.vao ||
            !waveformRendering.mid.vbo ||
            !waveformRendering.high.vao ||
            !waveformRendering.high.vbo ||
            !currentActiveMip ||
            !volumeAnalysis ||
            currentAudioDuration <= 0
        ) {
            waveformRendering.low.vertexCount = 0;
            waveformRendering.mid.vertexCount = 0;
            waveformRendering.high.vertexCount = 0;
            return;
        }

        // Calculate gain multipliers from dB
        const lowMultiplier = Math.pow(10, eqParams.lowGainDb / 20);
        const midMultiplier = Math.pow(10, eqParams.midGainDb / 20);
        const highMultiplier = Math.pow(10, eqParams.highGainDb / 20);

        const bins = currentActiveMip;
        if (!bins || bins.length === 0) {
            waveformRendering.low.vertexCount = 0;
            waveformRendering.mid.vertexCount = 0;
            waveformRendering.high.vertexCount = 0;
            return;
        }

        const maxRms =
            volumeAnalysis.maxBandEnergy > 0
                ? volumeAnalysis.maxBandEnergy
                : 0.0001;

        const vertexDataLow: number[] = [];
        const vertexDataMid: number[] = [];
        const vertexDataHigh: number[] = [];

        bins.forEach((bin: WaveBin, index: number) => {
            const binDurationForThisMip = currentAudioDuration / bins.length; // Duration of one bin at this MIP level
            const timeSec = index * binDurationForThisMip;
            const normalizedTimeX =
                currentAudioDuration > 0 ? timeSec / currentAudioDuration : 0;

            // Apply height gain factor here
            const yTopLow = Math.min(
                1.0,
                ((bin.low * lowMultiplier) / maxRms) * HEIGHT_GAIN_FACTOR, // Applied lowMultiplier
            );
            const yBottomLow = -yTopLow;
            vertexDataLow.push(normalizedTimeX, yBottomLow);
            vertexDataLow.push(normalizedTimeX, yTopLow);

            const yTopMid = Math.min(
                1.0,
                ((bin.mid * midMultiplier) / maxRms) * HEIGHT_GAIN_FACTOR, // Applied midMultiplier
            );
            const yBottomMid = -yTopMid;
            vertexDataMid.push(normalizedTimeX, yBottomMid);
            vertexDataMid.push(normalizedTimeX, yTopMid);

            const yTopHigh = Math.min(
                1.0,
                ((bin.high * highMultiplier) / maxRms) * HEIGHT_GAIN_FACTOR, // Applied highMultiplier
            );
            const yBottomHigh = -yTopHigh;
            vertexDataHigh.push(normalizedTimeX, yBottomHigh);
            vertexDataHigh.push(normalizedTimeX, yTopHigh);
        });

        waveformRendering.low.vertexCount = vertexDataLow.length / 2;
        waveformRendering.mid.vertexCount = vertexDataMid.length / 2;
        waveformRendering.high.vertexCount = vertexDataHigh.length / 2;

        setupBandGeometry(
            gl,
            waveformRendering.low.vao,
            waveformRendering.low.vbo,
            vertexDataLow,
        );
        setupBandGeometry(
            gl,
            waveformRendering.mid.vao,
            waveformRendering.mid.vbo,
            vertexDataMid,
        );
        setupBandGeometry(
            gl,
            waveformRendering.high.vao,
            waveformRendering.high.vbo,
            vertexDataHigh,
        );
    }

    function setupBandGeometry(
        ctx: WebGL2RenderingContext,
        vao: WebGLVertexArrayObject,
        vbo: WebGLBuffer,
        vertexData: number[],
    ) {
        ctx.bindBuffer(ctx.ARRAY_BUFFER, vbo);
        ctx.bufferData(
            ctx.ARRAY_BUFFER,
            new Float32Array(vertexData),
            ctx.STATIC_DRAW,
        );

        ctx.bindVertexArray(vao);
        // Attribute 0: Normalized Time X (a_normalized_time_x)
        ctx.enableVertexAttribArray(0);
        ctx.vertexAttribPointer(
            0, // Attribute location
            1, // Number of elements per attribute (just x)
            ctx.FLOAT,
            false, // Normalized
            2 * Float32Array.BYTES_PER_ELEMENT, // Stride: 2 floats per vertex (x, y)
            0, // Offset
        );
        // Attribute 1: Normalized Y Value (a_normalized_y_value)
        ctx.enableVertexAttribArray(1);
        ctx.vertexAttribPointer(
            1, // Attribute location
            1, // Number of elements per attribute (just y)
            ctx.FLOAT,
            false, // Normalized
            2 * Float32Array.BYTES_PER_ELEMENT, // Stride: 2 floats per vertex (x, y)
            1 * Float32Array.BYTES_PER_ELEMENT, // Offset: skip x (1 float)
        );

        // Unbind after setup is good practice, though render will rebind
        ctx.bindVertexArray(null);
        ctx.bindBuffer(ctx.ARRAY_BUFFER, null);
    }

    // --- Main Render Function ---
    function render() {
        if (!glContext.ctx || !glContext.canvas || !initialDimensionsSet)
            return;
        const gl = glContext.ctx; // local alias
        if (glContext.canvas.width === 0 || glContext.canvas.height === 0)
            return;

        gl.clearColor(0.0, 0.0, 0.0, 0.0);
        gl.clear(gl.COLOR_BUFFER_BIT);

        // Draw Waveform
        if (
            waveformRendering.program &&
            waveformRendering.low.vao &&
            waveformRendering.mid.vao &&
            waveformRendering.high.vao &&
            waveformRendering.low.vertexCount > 0 &&
            waveformRendering.mid.vertexCount > 0 &&
            waveformRendering.high.vertexCount > 0 &&
            waveformRendering.uniforms.colorLoc &&
            waveformRendering.uniforms.timeAtPlayheadLoc &&
            waveformRendering.uniforms.zoomFactorLoc
        ) {
            gl.useProgram(waveformRendering.program);

            // Set shared uniforms once
            const normalizedTimeAtPlayhead =
                audioDuration > 0
                    ? interpolationCtx.internalDisplayTime / audioDuration
                    : 0;
            gl.uniform1f(
                waveformRendering.uniforms.timeAtPlayheadLoc,
                normalizedTimeAtPlayhead,
            );
            gl.uniform1f(
                waveformRendering.uniforms.zoomFactorLoc,
                effectiveZoomFactor(),
            );

            const faderAlpha = Math.max(0, Math.min(1, faderLevel)); // Clamp fader level to [0,1]

            // Draw Low Band
            gl.uniform4f(
                waveformRendering.uniforms.colorLoc,
                lowBandColor[0],
                lowBandColor[1],
                lowBandColor[2],
                faderAlpha,
            );
            gl.bindVertexArray(waveformRendering.low.vao);
            gl.drawArrays(
                gl.TRIANGLE_STRIP,
                0,
                waveformRendering.low.vertexCount,
            );

            // Draw Mid Band
            gl.uniform4f(
                waveformRendering.uniforms.colorLoc,
                midBandColor[0],
                midBandColor[1],
                midBandColor[2],
                faderAlpha,
            );
            gl.bindVertexArray(waveformRendering.mid.vao);
            gl.drawArrays(
                gl.TRIANGLE_STRIP,
                0,
                waveformRendering.mid.vertexCount,
            );

            // Draw High Band
            gl.uniform4f(
                waveformRendering.uniforms.colorLoc,
                highBandColor[0],
                highBandColor[1],
                highBandColor[2],
                faderAlpha,
            );
            gl.bindVertexArray(waveformRendering.high.vao);
            gl.drawArrays(
                gl.TRIANGLE_STRIP,
                0,
                waveformRendering.high.vertexCount,
            );

            gl.bindVertexArray(null); // Unbind after all bands
        }

        // Draw Playhead
        if (
            playheadRendering.program &&
            playheadRendering.vao &&
            playheadRendering.vbo && // vbo check added for completeness, though not strictly needed if vao is good
            playheadRendering.uniforms.colorLoc
        ) {
            gl.useProgram(playheadRendering.program);
            gl.uniform3fv(playheadRendering.uniforms.colorLoc, PLAYHEAD_COLOR);
            gl.bindVertexArray(playheadRendering.vao);
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
            gl.bindVertexArray(null);
        }

        // Draw Cue Line (if active)
        if (
            cuePointTime !== null &&
            cueLineRendering.program &&
            cueLineRendering.vao &&
            cueLineRendering.uniforms.ndcXLoc &&
            cueLineRendering.uniforms.colorLoc &&
            audioDuration > 0
        ) {
            gl.useProgram(cueLineRendering.program);

            const normalizedCueTime = cuePointTime / audioDuration;
            const normalizedPlayheadCenterTime =
                interpolationCtx.internalDisplayTime / audioDuration;

            const cueNdcX =
                (normalizedCueTime - normalizedPlayheadCenterTime) *
                effectiveZoomFactor();

            // Only draw if within visible NDC range (plus a small buffer)
            if (cueNdcX >= -1.1 && cueNdcX <= 1.1) {
                gl.uniform1f(cueLineRendering.uniforms.ndcXLoc, cueNdcX);
                gl.uniform3fv(
                    cueLineRendering.uniforms.colorLoc,
                    CUE_LINE_COLOR,
                );

                gl.bindVertexArray(cueLineRendering.vao);
                gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
                gl.bindVertexArray(null);
            }
        }
    }

    // --- Continuous Render Loop Function ---
    function continuousRender() {
        if (!isTrackLoaded || !glContext.ctx || !initialDimensionsSet) {
            animationFrameId = null;
            console.log(
                "[WebGLWaveform] continuousRender: stopping due to unmet conditions.",
            );
            return;
        }

        if (seekAnimationCtx.isActive) {
            const elapsedAnimationTime =
                performance.now() - seekAnimationCtx.startTime;
            if (elapsedAnimationTime < SEEK_ANIMATION_DURATION_MS) {
                const animationProgress =
                    elapsedAnimationTime / SEEK_ANIMATION_DURATION_MS;
                // Simple linear interpolation, can be replaced with easing function
                interpolationCtx.internalDisplayTime =
                    seekAnimationCtx.startDisplayTime +
                    (seekAnimationCtx.targetDisplayTime -
                        seekAnimationCtx.startDisplayTime) *
                        animationProgress;
            } else {
                interpolationCtx.internalDisplayTime =
                    seekAnimationCtx.targetDisplayTime;
                seekAnimationCtx.isActive = false;
                // Sync up the host time references after animation completes
                interpolationCtx.lastHostTimeValue =
                    interpolationCtx.internalDisplayTime;
                interpolationCtx.lastHostUpdateTime = performance.now();
                console.log(
                    `[WebGLWaveform] Animation finished at: ${interpolationCtx.internalDisplayTime.toFixed(3)}s`,
                );
            }
        } else if (interpolationCtx.hostIsPlaying) {
            const now = performance.now();
            const elapsedTimeMs = now - interpolationCtx.lastHostUpdateTime;
            let newCalculatedTime =
                interpolationCtx.lastHostTimeValue + elapsedTimeMs / 1000.0;
            newCalculatedTime = Math.max(
                0,
                Math.min(newCalculatedTime, audioDuration),
            );
            interpolationCtx.internalDisplayTime = newCalculatedTime;
        } else {
            // Not playing and not animating seek, internalDisplayTime should be stable from the $effect
            if (!seekAnimationCtx.isActive) {
                interpolationCtx.internalDisplayTime =
                    interpolationCtx.lastHostTimeValue;
            }
        }

        render();
        animationFrameId = requestAnimationFrame(continuousRender);
    }

    let resizeObserver: ResizeObserver | null = null;
    function handleResize() {
        resizeCanvas();
        if (
            glContext.ctx &&
            initialDimensionsSet &&
            animationFrameId === null
        ) {
            render();
        }
    }

    onMount(() => {
        if (glContext.canvas) {
            initWebGL();
            resizeObserver = new ResizeObserver(handleResize);
            resizeObserver.observe(glContext.canvas);
        } else {
            console.error(
                "onMount: glContext.canvas is null. This should not happen if bind:this is working.",
            );
        }
    });

    onDestroy(() => {
        if (animationFrameId !== null) {
            cancelAnimationFrame(animationFrameId);
            animationFrameId = null;
        }
        if (resizeObserver && glContext.canvas)
            resizeObserver.unobserve(glContext.canvas);

        const gl = glContext.ctx;
        if (gl) {
            gl.disable(gl.BLEND); // Disable blending on cleanup
            if (waveformRendering.program)
                gl.deleteProgram(waveformRendering.program);
            if (waveformRendering.low.vao)
                gl.deleteVertexArray(waveformRendering.low.vao);
            if (waveformRendering.low.vbo)
                gl.deleteBuffer(waveformRendering.low.vbo);
            if (waveformRendering.mid.vao)
                gl.deleteVertexArray(waveformRendering.mid.vao);
            if (waveformRendering.mid.vbo)
                gl.deleteBuffer(waveformRendering.mid.vbo);
            if (waveformRendering.high.vao)
                gl.deleteVertexArray(waveformRendering.high.vao);
            if (waveformRendering.high.vbo)
                gl.deleteBuffer(waveformRendering.high.vbo);

            if (playheadRendering.program)
                gl.deleteProgram(playheadRendering.program);
            if (playheadRendering.vao)
                gl.deleteVertexArray(playheadRendering.vao);
            if (playheadRendering.vbo) gl.deleteBuffer(playheadRendering.vbo);

            if (cueLineRendering.program)
                gl.deleteProgram(cueLineRendering.program);
            if (cueLineRendering.vao)
                gl.deleteVertexArray(cueLineRendering.vao);
            if (cueLineRendering.vbo) gl.deleteBuffer(cueLineRendering.vbo);
        }
    });
</script>

<div class="webgl-waveform-container" style="width: 100%; height: 100%;">
    <canvas
        bind:this={glContext.canvas}
        style="display: block; width: 100%; height: 100%;"
        onclick={handleWaveformClick}
    ></canvas>

    {#if !(isTrackLoaded && activeMipLevel() && glContext.ctx && (waveformRendering.low.vertexCount > 0 || waveformRendering.mid.vertexCount > 0 || waveformRendering.high.vertexCount > 0)) && !isAnalysisPending}
        <div class="status-message placeholder">
            {#if !isTrackLoaded}
                Load audio to see waveform
            {:else if isAnalysisPending}
                Analyzing audio...
            {:else if !activeMipLevel() || (waveformRendering.low.vertexCount === 0 && waveformRendering.mid.vertexCount === 0 && waveformRendering.high.vertexCount === 0)}
                Waveform data not available or empty
            {:else}
                Preparing waveform display...
            {/if}
        </div>
    {/if}
    {#if isAnalysisPending && isTrackLoaded && !activeMipLevel()}
        <div class="status-message loading-message">
            Analyzing audio... (MIP level pending)
        </div>
    {/if}
</div>

<style>
    .webgl-waveform-container {
        position: relative;
        min-height: 80px;
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
    .status-message.placeholder {
        color: #888;
    }
    .status-message.loading-message {
        color: #ccc;
    }

    @media (prefers-color-scheme: light) {
        .status-message {
            color: #555;
            background-color: rgba(233, 233, 233, 0.8);
        }
        .status-message.placeholder {
            color: #888;
        }
        .status-message.loading-message {
            color: #333;
        }
    }
</style>
