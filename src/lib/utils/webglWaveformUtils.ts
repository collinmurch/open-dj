export const waveformVertexShaderSource = `#version 300 es
    layout(location = 0) in float a_normalized_time_x; // Song's normalized time [0, 1]
    layout(location = 1) in float a_normalized_y_value;
    layout(location = 2) in float a_band_type; // 0=low, 1=mid, 2=high

    uniform float u_normalized_time_at_playhead;
    uniform float u_zoom_factor;
    uniform vec3 u_eq_multipliers; // [low, mid, high] linear gain multipliers

    void main() {
        float x_ndc = (a_normalized_time_x - u_normalized_time_at_playhead) * u_zoom_factor;
        
        // Apply EQ gain based on band type
        float eq_multiplier;
        if (a_band_type < 0.5) {
            eq_multiplier = u_eq_multipliers.x; // low
        } else if (a_band_type < 1.5) {
            eq_multiplier = u_eq_multipliers.y; // mid
        } else {
            eq_multiplier = u_eq_multipliers.z; // high
        }
        
        float adjusted_y = a_normalized_y_value * eq_multiplier;
        gl_Position = vec4(x_ndc, adjusted_y, 0.0, 1.0);
    }
`.trim();

export const waveformFragmentShaderSource = `#version 300 es
    precision mediump float;
    uniform vec4 u_waveform_color_with_alpha; // Changed to vec4
    out vec4 fragColor;
    void main() {
        fragColor = u_waveform_color_with_alpha; // Use directly
    }
`.trim();

export const playheadVertexShaderSource = `#version 300 es
    layout(location = 0) in vec2 a_pos;
    void main() {
        // Playhead is centered at NDC x = 0, a_pos.x provides the half-width offset
        gl_Position = vec4(a_pos.x, a_pos.y, 0.0, 1.0);
    }
`.trim();

export const playheadFragmentShaderSource = `#version 300 es
    precision mediump float;
    uniform vec3 u_playhead_color;
    out vec4 fragColor;
    void main() {
        fragColor = vec4(u_playhead_color, 1.0);
    }
`.trim();

export const cueLineVertexShaderSource = `#version 300 es
    layout(location = 0) in vec2 a_line_pos; // a_line_pos.x is -half_width or +half_width
    uniform float u_cue_line_ndc_x; // The NDC x-coordinate for the cue line center

    void main() {
        // Offset the line's local x-coordinate (half-width) by the uniform's global x-position
        gl_Position = vec4(u_cue_line_ndc_x + a_line_pos.x, a_line_pos.y, 0.0, 1.0);
    }
`.trim();

export const cueLineFragmentShaderSource = `#version 300 es
    precision mediump float;
    uniform vec3 u_cue_line_color;
    out vec4 fragColor;

    void main() {
        fragColor = vec4(u_cue_line_color, 1.0);
    }
`.trim();

export function createShader(
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

export function createProgram(
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