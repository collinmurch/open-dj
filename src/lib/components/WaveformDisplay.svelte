<script lang="ts">
    import type { VolumeAnalysis, WaveBin } from "$lib/types";
    import { onDestroy, onMount } from "svelte";

    // --- Component Props ---
    let {
        volumeAnalysis = null as VolumeAnalysis | null,
        audioDuration = 0,
        currentTime = 0,
        isPlaying = false,
        isAnalysisPending = false,
        isTrackLoaded = false,
        seekAudio = (time: number) => {},
        cuePointTime = null as number | null,
        waveformColor = [0.3, 0.6, 0.8] as [number, number, number],
    }: {
        volumeAnalysis: VolumeAnalysis | null;
        audioDuration: number;
        currentTime?: number;
        isPlaying?: boolean;
        isAnalysisPending?: boolean;
        isTrackLoaded?: boolean;
        seekAudio?: (time: number) => void;
        cuePointTime?: number | null;
        waveformColor?: [number, number, number];
    } = $props();

    // --- WebGL State & Refs ---
    let canvasElement: HTMLCanvasElement | null = $state(null);
    let gl: WebGL2RenderingContext | null = $state(null);

    // Waveform resources
    let waveformProgram: WebGLProgram | null = $state(null);
    let waveformVao: WebGLVertexArrayObject | null = $state(null);
    let waveformVbo: WebGLBuffer | null = $state(null);
    let waveformVertexCount = $state(0);

    // Playhead resources
    let playheadProgram: WebGLProgram | null = $state(null);
    let playheadVao: WebGLVertexArrayObject | null = $state(null);
    let playheadVbo: WebGLBuffer | null = $state(null);

    // Cue Line resources
    let cueLineProgram: WebGLProgram | null = $state(null);
    let cueLineVao: WebGLVertexArrayObject | null = $state(null);
    let cueLineVbo: WebGLBuffer | null = $state(null);
    let uCueLineNdcXLoc: WebGLUniformLocation | null = $state(null);
    let uCueLineColorLoc: WebGLUniformLocation | null = $state(null);

    // --- Uniform Locations ---
    let uWaveformColorLoc: WebGLUniformLocation | null = $state(null);
    let uNormalizedTimeAtPlayheadLoc: WebGLUniformLocation | null =
        $state(null);
    let uZoomFactorLoc: WebGLUniformLocation | null = $state(null);
    let uPlayheadColorLoc: WebGLUniformLocation | null = $state(null);

    let initialDimensionsSet = $state(false);

    // --- Interpolation State ---
    let internalDisplayTime = $state(0);
    let lastHostUpdateTime = $state(performance.now());
    let lastHostTimeValue = $state(0);
    let hostIsPlaying = $state(false);

    // --- Animation State ---
    let isSeekingAnimationActive = $state(false);
    let seekAnimationStartTime = $state(0);
    let seekAnimationStartDisplayTime = $state(0);
    let seekAnimationTargetDisplayTime = $state(0);
    const SEEK_ANIMATION_DURATION_MS = 80;
    const SEEK_TRIGGER_THRESHOLD_SECONDS = 0.2; // If currentTime jumps by more than this, animate

    // --- Constants ---
    const PLAYHEAD_COLOR = [1.0, 0.2, 0.2]; // A visible red
    const CUE_LINE_COLOR = [0.14, 0.55, 0.96]; // Changed to selection blue (approx hsl(210, 90%, 55%))
    const INITIAL_ZOOM_FACTOR = 75.0;
    const HEIGHT_GAIN_FACTOR = 2.0;

    // --- State for controlling geometry updates ---
    let lastProcessedVolumeAnalysis: VolumeAnalysis | null | undefined =
        $state(undefined);
    let lastProcessedAudioDuration: number = $state(NaN);
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

    // Effect to update geometry ONLY when the track/analysis actually changes
    $effect(() => {
        const newVolumeAnalysis = volumeAnalysis;
        const newAudioDuration = audioDuration;

        const analysisObjectChanged =
            newVolumeAnalysis !== lastProcessedVolumeAnalysis;
        const durationChangedSignificantly =
            newAudioDuration !== lastProcessedAudioDuration &&
            newAudioDuration > 0;

        if (
            analysisObjectChanged ||
            (newVolumeAnalysis && durationChangedSignificantly)
        ) {
            console.log(
                `[WebGLWaveform] Geometry update criteria met. analysisObjectChanged: ${analysisObjectChanged}, durationChangedSignificantly: ${durationChangedSignificantly}. New duration: ${newAudioDuration}`,
            );

            lastProcessedVolumeAnalysis = newVolumeAnalysis;
            lastProcessedAudioDuration = newAudioDuration;

            if (activeMipLevel() && gl && newAudioDuration > 0) {
                console.log("[WebGLWaveform] Updating waveform geometry.");
                updateWaveformGeometry(); // Uses activeMipLevel() and newAudioDuration implicitly
                // render() will be called by the continuousRender loop if active
            } else {
                console.log(
                    "[WebGLWaveform] Clearing waveform geometry due to invalid analysis/duration.",
                );
                waveformVertexCount = 0;
            }
        } else if (!newVolumeAnalysis && lastProcessedVolumeAnalysis !== null) {
            // Explicitly clear if volumeAnalysis becomes null (track unloaded)
            console.log(
                "[WebGLWaveform] Track unloaded (volumeAnalysis is null), clearing geometry.",
            );
            waveformVertexCount = 0;
            lastProcessedVolumeAnalysis = null;
            lastProcessedAudioDuration = NaN;
        }
        // If only currentTime changes, this effect should NOT re-run updateWaveformGeometry.
        // The render() call is handled by the continuousRender loop.
    });

    // Effect to sync with host state for interpolation
    $effect(() => {
        const hostTimeProp = currentTime; // Prop value from playerStore
        const hostPlayingStatusProp = isPlaying; // Prop value

        const timeDelta = Math.abs(hostTimeProp - lastHostTimeValue);

        if (
            timeDelta > SEEK_TRIGGER_THRESHOLD_SECONDS &&
            !isSeekingAnimationActive
        ) {
            // Likely a seek or significant jump, trigger animation
            console.log(
                `[WebGLWaveform] Animation triggered. From: ${internalDisplayTime.toFixed(3)}s to: ${hostTimeProp.toFixed(3)}s`,
            );
            isSeekingAnimationActive = true;
            seekAnimationStartTime = performance.now();
            seekAnimationStartDisplayTime = internalDisplayTime; // Current visual position
            seekAnimationTargetDisplayTime = hostTimeProp; // Target from prop
        } else if (!isSeekingAnimationActive) {
            // Not animating, so update baseline for normal interpolation or static display
            internalDisplayTime = hostTimeProp;
        }
        // Always update these for the next frame's interpolation logic or delta check
        lastHostTimeValue = hostTimeProp;
        lastHostUpdateTime = performance.now();
        hostIsPlaying = hostPlayingStatusProp;
    });

    // Effect to manage the continuous render loop
    $effect(() => {
        if (isTrackLoaded && initialDimensionsSet && gl) {
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
        if (!gl || !canvasElement || audioDuration <= 0 || !isTrackLoaded) {
            return;
        }

        const rect = canvasElement.getBoundingClientRect();
        const clickXInCanvas = event.clientX - rect.left;
        const canvasWidth = canvasElement.clientWidth;

        // Convert click X to NDC space (-1 to 1)
        const clickedNdcX = (clickXInCanvas / canvasWidth) * 2.0 - 1.0;

        // Current normalized time at the center of the view (playhead position)
        const normalizedCenterTime =
            audioDuration > 0 ? internalDisplayTime / audioDuration : 0;

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

    // --- Shader Definitions (waveformVertexShaderSource includes u_normalized_time_at_playhead and u_zoom_factor) ---
    const waveformVertexShaderSource = `#version 300 es
        layout(location = 0) in float a_normalized_time_x; // Song's normalized time [0, 1]
        layout(location = 1) in float a_normalized_y_value;

        uniform float u_normalized_time_at_playhead;
        uniform float u_zoom_factor;

        void main() {
            float x_ndc = (a_normalized_time_x - u_normalized_time_at_playhead) * u_zoom_factor;
            gl_Position = vec4(x_ndc, a_normalized_y_value, 0.0, 1.0);
        }
    `.trim();

    const waveformFragmentShaderSource = `#version 300 es
        precision mediump float;
        uniform vec3 u_waveform_color;
        out vec4 fragColor;
        void main() {
            fragColor = vec4(u_waveform_color, 1.0);
        }
    `.trim();

    const playheadVertexShaderSource = `#version 300 es
        layout(location = 0) in vec2 a_pos;
        void main() {
            gl_Position = vec4(0.0, a_pos.y, 0.0, 1.0);
        }
    `.trim();

    const playheadFragmentShaderSource = `#version 300 es
        precision mediump float;
        uniform vec3 u_playhead_color;
        out vec4 fragColor;
        void main() {
            fragColor = vec4(u_playhead_color, 1.0);
        }
    `.trim();

    // --- NEW: Cue Line Shaders ---
    const cueLineVertexShaderSource = `#version 300 es
        layout(location = 0) in vec2 a_line_pos; // Simple line vertices (-1 to 1 for y)
        uniform float u_cue_line_ndc_x; // The NDC x-coordinate for the cue line

        void main() {
            gl_Position = vec4(u_cue_line_ndc_x, a_line_pos.y, 0.0, 1.0);
        }
    `.trim();

    const cueLineFragmentShaderSource = `#version 300 es
        precision mediump float;
        uniform vec3 u_cue_line_color;
        out vec4 fragColor;

        void main() {
            fragColor = vec4(u_cue_line_color, 1.0);
        }
    `.trim();

    // --- WebGL Helper Functions (createShader, createProgram - unchanged) ---
    function createShader(
        ctx: WebGL2RenderingContext,
        type: number,
        source: string,
    ): WebGLShader | null {
        const shader = ctx.createShader(type);
        if (!shader) return null;
        ctx.shaderSource(shader, source);
        ctx.compileShader(shader);
        if (!ctx.getShaderParameter(shader, ctx.COMPILE_STATUS)) {
            console.error(
                `Error compiling shader (${type === ctx.VERTEX_SHADER ? "Vertex" : "Fragment"}):`,
                ctx.getShaderInfoLog(shader),
            );
            ctx.deleteShader(shader);
            return null;
        }
        return shader;
    }

    function createProgram(
        ctx: WebGL2RenderingContext,
        vertexShader: WebGLShader,
        fragmentShader: WebGLShader,
    ): WebGLProgram | null {
        const program = ctx.createProgram();
        if (!program) return null;
        ctx.attachShader(program, vertexShader);
        ctx.attachShader(program, fragmentShader);
        ctx.linkProgram(program);
        if (!ctx.getProgramParameter(program, ctx.LINK_STATUS)) {
            console.error(
                "Error linking program:",
                ctx.getProgramInfoLog(program),
            );
            ctx.deleteProgram(program);
            return null;
        }
        return program;
    }

    // --- WebGL Initialization (includes playhead setup) ---
    function initWebGL() {
        if (!canvasElement) return;
        const context = canvasElement.getContext("webgl2");
        if (!context) {
            console.error("WebGL2 not supported or context creation failed");
            return;
        }
        gl = context;

        // Waveform program
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
        if (wfVert && wfFrag) {
            waveformProgram = createProgram(gl, wfVert, wfFrag);
            gl.deleteShader(wfVert);
            gl.deleteShader(wfFrag);
        }

        if (waveformProgram) {
            uWaveformColorLoc = gl.getUniformLocation(
                waveformProgram,
                "u_waveform_color",
            );
            uNormalizedTimeAtPlayheadLoc = gl.getUniformLocation(
                waveformProgram,
                "u_normalized_time_at_playhead",
            );
            uZoomFactorLoc = gl.getUniformLocation(
                waveformProgram,
                "u_zoom_factor",
            );
            waveformVao = gl.createVertexArray();
            waveformVbo = gl.createBuffer();
        } else {
            console.error("Failed to create waveform program");
            return;
        }

        // Playhead program
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
        if (phVert && phFrag) {
            playheadProgram = createProgram(gl, phVert, phFrag);
            gl.deleteShader(phVert);
            gl.deleteShader(phFrag);
        }

        if (playheadProgram) {
            uPlayheadColorLoc = gl.getUniformLocation(
                playheadProgram,
                "u_playhead_color",
            );
            playheadVao = gl.createVertexArray();
            playheadVbo = gl.createBuffer();
            gl.bindBuffer(gl.ARRAY_BUFFER, playheadVbo);
            const lineVerts = new Float32Array([0.0, -1.0, 0.0, 1.0]);
            gl.bufferData(gl.ARRAY_BUFFER, lineVerts, gl.STATIC_DRAW);
            gl.bindVertexArray(playheadVao);
            gl.enableVertexAttribArray(0);
            gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
            gl.bindVertexArray(null);
            gl.bindBuffer(gl.ARRAY_BUFFER, null);
        } else {
            console.error("Failed to create playhead program");
            return;
        }

        // --- Cue Line Program Setup ---
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
        if (clVert && clFrag) {
            cueLineProgram = createProgram(gl, clVert, clFrag);
            gl.deleteShader(clVert);
            gl.deleteShader(clFrag);
        }

        if (cueLineProgram) {
            uCueLineNdcXLoc = gl.getUniformLocation(
                cueLineProgram,
                "u_cue_line_ndc_x",
            );
            uCueLineColorLoc = gl.getUniformLocation(
                cueLineProgram,
                "u_cue_line_color",
            );

            cueLineVao = gl.createVertexArray();
            cueLineVbo = gl.createBuffer();
            gl.bindBuffer(gl.ARRAY_BUFFER, cueLineVbo);
            const cueLineVerts = new Float32Array([0.0, -1.0, 0.0, 1.0]); // X is unused here, will be set by uniform
            gl.bufferData(gl.ARRAY_BUFFER, cueLineVerts, gl.STATIC_DRAW);

            gl.bindVertexArray(cueLineVao);
            gl.enableVertexAttribArray(0); // a_line_pos
            // For a_line_pos, we only need the Y component from the VBO for this shader logic.
            // The X component in the VBO is effectively ignored by the cueLineVertexShader
            // as gl_Position.x is directly set by u_cue_line_ndc_x.
            // However, we still define the attribute as vec2 a_line_pos, so we pass 2 components.
            gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

            gl.bindVertexArray(null);
            gl.bindBuffer(gl.ARRAY_BUFFER, null);
        } else {
            console.error("Failed to create cue line program");
            // No need to return early from entire initWebGL if only cue line fails, rest might work.
        }

        handleResize(); // Sets initial dimensions and calls render if ready
    }

    function resizeCanvas() {
        if (!gl || !canvasElement) return;
        const newWidth = canvasElement.clientWidth;
        const newHeight = canvasElement.clientHeight;

        if (
            canvasElement.width !== newWidth ||
            canvasElement.height !== newHeight
        ) {
            canvasElement.width = newWidth;
            canvasElement.height = newHeight;
            gl.viewport(0, 0, canvasElement.width, canvasElement.height);
            console.log(
                `[WebGLWaveform] Canvas resized to: ${canvasElement.width}x${canvasElement.height}.`,
            );
        }
        if (newWidth > 0 && newHeight > 0 && !initialDimensionsSet) {
            initialDimensionsSet = true;
        }
        // render() is now primarily driven by continuousRender or geometry updates
    }

    // --- Data Preparation for Waveform (unchanged from previous step, uses audioDuration from prop) ---
    function updateWaveformGeometry() {
        const currentAudioDuration = audioDuration; // Use prop directly
        const currentActiveMip = activeMipLevel(); // Use derived value

        if (
            !gl ||
            !waveformVao ||
            !waveformVbo ||
            !currentActiveMip ||
            !volumeAnalysis ||
            currentAudioDuration <= 0
        ) {
            waveformVertexCount = 0;
            return;
        }

        const bins = currentActiveMip;
        if (!bins || bins.length === 0) {
            waveformVertexCount = 0;
            return;
        }

        const maxRms =
            volumeAnalysis.max_rms_amplitude > 0
                ? volumeAnalysis.max_rms_amplitude
                : 0.0001;

        const vertexData: number[] = [];
        bins.forEach((bin: WaveBin, index: number) => {
            const binDurationForThisMip = currentAudioDuration / bins.length; // Duration of one bin at this MIP level
            const timeSec = index * binDurationForThisMip;
            const normalizedTimeX =
                currentAudioDuration > 0 ? timeSec / currentAudioDuration : 0;

            // Apply height gain factor here
            const baseNormalizedAmplitude = Math.min(1.0, bin.mid / maxRms);
            const amplifiedNormalizedAmplitude = Math.min(
                1.0,
                baseNormalizedAmplitude * HEIGHT_GAIN_FACTOR,
            );

            const yTopNdc = amplifiedNormalizedAmplitude;
            const yBottomNdc = -amplifiedNormalizedAmplitude;

            vertexData.push(normalizedTimeX, yBottomNdc);
            vertexData.push(normalizedTimeX, yTopNdc);
        });

        waveformVertexCount = vertexData.length / 2;

        gl.bindBuffer(gl.ARRAY_BUFFER, waveformVbo);
        gl.bufferData(
            gl.ARRAY_BUFFER,
            new Float32Array(vertexData),
            gl.STATIC_DRAW,
        );

        gl.bindVertexArray(waveformVao);
        gl.enableVertexAttribArray(0);
        gl.vertexAttribPointer(
            0,
            1,
            gl.FLOAT,
            false,
            2 * Float32Array.BYTES_PER_ELEMENT,
            0,
        );
        gl.enableVertexAttribArray(1);
        gl.vertexAttribPointer(
            1,
            1,
            gl.FLOAT,
            false,
            2 * Float32Array.BYTES_PER_ELEMENT,
            1 * Float32Array.BYTES_PER_ELEMENT,
        );

        gl.bindVertexArray(null);
        gl.bindBuffer(gl.ARRAY_BUFFER, null);
    }

    // --- Main Render Function ---
    function render() {
        if (!gl || !canvasElement || !initialDimensionsSet) return;
        if (canvasElement.width === 0 || canvasElement.height === 0) return;

        gl.clearColor(0.0, 0.0, 0.0, 0.0);
        gl.clear(gl.COLOR_BUFFER_BIT);

        // Draw Waveform
        if (
            waveformProgram &&
            waveformVao &&
            waveformVertexCount > 0 &&
            uWaveformColorLoc &&
            uNormalizedTimeAtPlayheadLoc &&
            uZoomFactorLoc
        ) {
            gl.useProgram(waveformProgram);
            gl.uniform3fv(uWaveformColorLoc, waveformColor);

            const currentTrackTimeToRender = internalDisplayTime;
            const currentTrackDuration = audioDuration;
            const normalizedCurrentTime =
                currentTrackDuration > 0 && currentTrackTimeToRender >= 0
                    ? currentTrackTimeToRender / currentTrackDuration
                    : 0.0;

            gl.uniform1f(uNormalizedTimeAtPlayheadLoc, normalizedCurrentTime);
            gl.uniform1f(uZoomFactorLoc, INITIAL_ZOOM_FACTOR);

            gl.bindVertexArray(waveformVao);
            gl.drawArrays(gl.TRIANGLE_STRIP, 0, waveformVertexCount);
            gl.bindVertexArray(null);
        }

        // Draw Playhead
        if (
            playheadProgram &&
            playheadVao &&
            playheadVbo &&
            uPlayheadColorLoc
        ) {
            gl.useProgram(playheadProgram);
            gl.uniform3fv(uPlayheadColorLoc, PLAYHEAD_COLOR);
            gl.bindVertexArray(playheadVao);
            gl.drawArrays(gl.LINES, 0, 2);
            gl.bindVertexArray(null);
        }

        // Draw Cue Line (if active)
        if (
            cuePointTime !== null &&
            cueLineProgram &&
            cueLineVao &&
            uCueLineNdcXLoc &&
            uCueLineColorLoc &&
            audioDuration > 0
        ) {
            gl.useProgram(cueLineProgram);

            const normalizedCueTime = cuePointTime / audioDuration;
            const normalizedPlayheadCenterTime =
                internalDisplayTime / audioDuration;

            const cueNdcX =
                (normalizedCueTime - normalizedPlayheadCenterTime) *
                INITIAL_ZOOM_FACTOR;

            // Only draw if within visible NDC range (plus a small buffer)
            if (cueNdcX >= -1.1 && cueNdcX <= 1.1) {
                gl.uniform1f(uCueLineNdcXLoc, cueNdcX);
                gl.uniform3fv(uCueLineColorLoc, CUE_LINE_COLOR);

                gl.bindVertexArray(cueLineVao);
                gl.drawArrays(gl.LINES, 0, 2);
                gl.bindVertexArray(null);
            }
        }
    }

    // --- Continuous Render Loop Function ---
    function continuousRender() {
        if (!isTrackLoaded || !gl || !initialDimensionsSet) {
            animationFrameId = null;
            console.log(
                "[WebGLWaveform] continuousRender: stopping due to unmet conditions.",
            );
            return;
        }

        if (isSeekingAnimationActive) {
            const elapsedAnimationTime =
                performance.now() - seekAnimationStartTime;
            if (elapsedAnimationTime < SEEK_ANIMATION_DURATION_MS) {
                const animationProgress =
                    elapsedAnimationTime / SEEK_ANIMATION_DURATION_MS;
                // Simple linear interpolation, can be replaced with easing function
                internalDisplayTime =
                    seekAnimationStartDisplayTime +
                    (seekAnimationTargetDisplayTime -
                        seekAnimationStartDisplayTime) *
                        animationProgress;
            } else {
                internalDisplayTime = seekAnimationTargetDisplayTime;
                isSeekingAnimationActive = false;
                // Sync up the host time references after animation completes
                lastHostTimeValue = internalDisplayTime;
                lastHostUpdateTime = performance.now();
                console.log(
                    `[WebGLWaveform] Animation finished at: ${internalDisplayTime.toFixed(3)}s`,
                );
            }
        } else if (hostIsPlaying) {
            const now = performance.now();
            const elapsedTimeMs = now - lastHostUpdateTime;
            let newCalculatedTime = lastHostTimeValue + elapsedTimeMs / 1000.0;
            newCalculatedTime = Math.max(
                0,
                Math.min(newCalculatedTime, audioDuration),
            );
            internalDisplayTime = newCalculatedTime;
        } else {
            // Not playing and not animating seek, internalDisplayTime should be stable from the $effect
            // For safety, ensure it reflects the last known host time if no animation is active.
            if (!isSeekingAnimationActive) {
                // Double check to avoid conflict if an animation just finished
                internalDisplayTime = lastHostTimeValue;
            }
        }

        render();
        animationFrameId = requestAnimationFrame(continuousRender);
    }

    let resizeObserver: ResizeObserver | null = null;
    function handleResize() {
        resizeCanvas();
        if (gl && initialDimensionsSet && animationFrameId === null) {
            render();
        }
    }

    onMount(() => {
        if (canvasElement) {
            initWebGL();
            resizeObserver = new ResizeObserver(handleResize);
            resizeObserver.observe(canvasElement);
        } else {
            console.error("onMount: canvasElement is null.");
        }
    });

    onDestroy(() => {
        if (animationFrameId !== null) {
            cancelAnimationFrame(animationFrameId);
            animationFrameId = null;
        }
        if (resizeObserver && canvasElement)
            resizeObserver.unobserve(canvasElement);
        if (gl) {
            if (waveformProgram) gl.deleteProgram(waveformProgram);
            if (waveformVao) gl.deleteVertexArray(waveformVao);
            if (waveformVbo) gl.deleteBuffer(waveformVbo);
            if (playheadProgram) gl.deleteProgram(playheadProgram);
            if (playheadVao) gl.deleteVertexArray(playheadVao);
            if (playheadVbo) gl.deleteBuffer(playheadVbo);

            if (cueLineProgram) gl.deleteProgram(cueLineProgram);
            if (cueLineVao) gl.deleteVertexArray(cueLineVao);
            if (cueLineVbo) gl.deleteBuffer(cueLineVbo);
        }
    });
</script>

<div class="webgl-waveform-container" style="width: 100%; height: 100%;">
    <canvas
        bind:this={canvasElement}
        style="display: block; width: 100%; height: 100%;"
        onclick={handleWaveformClick}
    ></canvas>

    {#if !(isTrackLoaded && activeMipLevel() && gl && waveformVertexCount > 0) && !isAnalysisPending}
        <div class="status-message placeholder">
            {#if !isTrackLoaded}
                Load audio to see waveform
            {:else if isAnalysisPending}
                Analyzing audio...
            {:else if !activeMipLevel() || waveformVertexCount === 0}
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
        background-color: #1a1a1a;
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
        .webgl-waveform-container {
            background-color: #f0f0f0;
        }
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
