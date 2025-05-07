<script lang="ts">
    import { onDestroy } from "svelte";

    // --- Props --- A versatile Slider component
    let {
        id,
        label,
        orientation = "vertical" as "vertical" | "horizontal",
        outputMin = 0,
        outputMax = 100,
        // If undefined or same as mathematical midpoint, linear mapping is used.
        centerValue = undefined as number | undefined,
        step = 1,
        value = $bindable(),
        debounceMs = 50,
    }: {
        id: string;
        label: string;
        orientation?: "vertical" | "horizontal";
        outputMin?: number;
        outputMax?: number;
        centerValue?: number;
        step?: number;
        value?: number;
        debounceMs?: number;
    } = $props();

    // --- Internal State --- The raw slider works on 0-100 range
    const SLIDER_MIN = 0;
    const SLIDER_MAX = 100;
    const SLIDER_CENTER = 50;

    // Determine if we should use the special center-point mapping
    const useCenterMapping = $derived(
        centerValue !== undefined &&
            // Check if centerValue is significantly different from the mathematical midpoint
            Math.abs(centerValue - (outputMin + outputMax) / 2) > 1e-6,
    );

    // Initial internal value calculation needs to respect the mapping type
    let internalRawValue = $state(getInitialRawValue());
    let debounceTimeoutId: number | undefined = undefined;

    function getInitialRawValue(): number {
        // If value is defined, map it. Otherwise, default based on mapping type.
        const initialOutput =
            value ?? (useCenterMapping ? centerValue : outputMin);
        return outputToRaw(initialOutput);
    }

    // --- Mapping Functions ---

    // Convert actual output value to raw slider position (0-100)
    function outputToRaw(outputVal: number | undefined): number {
        // Use centerValue only if mapping enabled AND value is undefined, otherwise use outputMin as fallback
        const val =
            outputVal ??
            (useCenterMapping && centerValue !== undefined
                ? centerValue
                : outputMin);

        if (useCenterMapping && centerValue !== undefined) {
            // Use the existing 3-point mapping logic centered around centerValue
            if (val === centerValue) return SLIDER_CENTER;
            if (val <= outputMin) return SLIDER_MIN;
            if (val >= outputMax) return SLIDER_MAX;
            if (val < centerValue) {
                if (outputMin === centerValue) return SLIDER_CENTER;
                const ratio = (val - centerValue) / (outputMin - centerValue);
                return SLIDER_CENTER + ratio * (SLIDER_MIN - SLIDER_CENTER);
            } else {
                // val > centerValue
                if (outputMax === centerValue) return SLIDER_CENTER;
                const ratio = (val - centerValue) / (outputMax - centerValue);
                return SLIDER_CENTER + ratio * (SLIDER_MAX - SLIDER_CENTER);
            }
        } else {
            // Simple linear mapping: outputMin..outputMax to SLIDER_MIN..SLIDER_MAX
            // Ensure outputMax !== outputMin before division
            const range = outputMax - outputMin;
            if (Math.abs(range) < 1e-9) return SLIDER_CENTER; // Avoid division by zero/small numbers

            const clampedVal = Math.max(outputMin, Math.min(outputMax, val));
            const ratio = (clampedVal - outputMin) / range; // Use calculated range
            return SLIDER_MIN + ratio * (SLIDER_MAX - SLIDER_MIN);
        }
    }

    // Convert raw slider position (0-100) to actual output value
    function rawToOutput(rawVal: number): number {
        if (useCenterMapping && centerValue !== undefined) {
            // Use the existing 3-point mapping logic
            if (rawVal === SLIDER_CENTER) return centerValue;
            // Clamp raw value before mapping to avoid exceeding output range due to float precision
            const clampedRaw = Math.max(
                SLIDER_MIN,
                Math.min(SLIDER_MAX, rawVal),
            );
            if (clampedRaw < SLIDER_CENTER) {
                const ratio =
                    (clampedRaw - SLIDER_CENTER) / (SLIDER_MIN - SLIDER_CENTER);
                // Add small epsilon to prevent floating point issues returning exactly centerValue sometimes
                return centerValue + ratio * (outputMin - centerValue);
            } else {
                // rawVal >= SLIDER_CENTER
                const ratio =
                    (clampedRaw - SLIDER_CENTER) / (SLIDER_MAX - SLIDER_CENTER);
                return centerValue + ratio * (outputMax - centerValue);
            }
        } else {
            // Simple linear mapping: SLIDER_MIN..SLIDER_MAX to outputMin..outputMax
            const clampedRaw = Math.max(
                SLIDER_MIN,
                Math.min(SLIDER_MAX, rawVal),
            );
            const ratio = (clampedRaw - SLIDER_MIN) / (SLIDER_MAX - SLIDER_MIN);
            return outputMin + ratio * (outputMax - outputMin);
        }
    }

    // --- Effects ---

    // Calculate the mapped output value based on internal raw slider value
    let actualOutputValue = $derived(rawToOutput(internalRawValue));

    // Effect to handle debounced updates to the bound value (outwards)
    $effect(() => {
        const valueToEmit = actualOutputValue;
        if (debounceTimeoutId !== undefined) clearTimeout(debounceTimeoutId);

        debounceTimeoutId = setTimeout(() => {
            // Update the externally bound value after debounce
            if (value !== valueToEmit) value = valueToEmit;
        }, debounceMs);
    });

    // --- Input Handler ---
    function handleInput(event: Event) {
        const target = event.currentTarget as HTMLInputElement;
        const newValue = parseFloat(target.value);
        internalRawValue = newValue;
    }

    // Cleanup timeout on component destroy
    onDestroy(() => {
        if (debounceTimeoutId !== undefined) clearTimeout(debounceTimeoutId);
    });

    const isVertical = $derived(orientation === "vertical");
</script>

<div
    class="slider-wrapper"
    class:vertical={isVertical}
    class:horizontal={!isVertical}
>
    <label for={id} class="slider-label">{label}</label>
    <div
        class="slider-container"
        class:vertical={isVertical}
        class:horizontal={!isVertical}
    >
        <input
            type="range"
            {id}
            min={SLIDER_MIN}
            max={SLIDER_MAX}
            step={1}
            value={internalRawValue}
            oninput={handleInput}
            aria-label={label}
            class="slider"
            class:vertical-slider={isVertical}
            class:horizontal-slider={!isVertical}
            aria-valuemin={outputMin}
            aria-valuemax={outputMax}
            aria-valuenow={actualOutputValue}
            aria-orientation={orientation}
        />
        <!-- Display the actual output value -->
        <span class="slider-value"
            >{actualOutputValue.toFixed(step < 1 ? 2 : 0)}</span
        >
    </div>
</div>

<style>
    .slider-wrapper {
        display: flex;
        align-items: center;
        gap: 1rem;
        padding: 0.5rem 0 0.25rem;
        min-width: 60px; /* Default for vertical */
    }
    .slider-wrapper.vertical {
        flex-direction: column;
        height: 100%;
    }
    .slider-wrapper.horizontal {
        flex-direction: row;
        width: 100%;
        min-height: 60px; /* Default for horizontal */
    }

    .slider-label {
        font-size: 0.8em;
        color: var(--muted-foreground, #555);
        text-align: center;
        flex-shrink: 0;
    }
    .slider-wrapper.horizontal .slider-label {
        min-width: 80px; /* Give some space for horizontal label */
        text-align: left;
    }

    .slider-container {
        display: flex;
        align-items: center;
        flex-grow: 1;
        position: relative;
    }
    .slider-container.vertical {
        flex-direction: column;
        width: 100%;
    }
    .slider-container.horizontal {
        flex-direction: row;
        height: 100%;
    }

    .slider {
        cursor: pointer;
        margin: 0;
        background: transparent;
        padding: 0;
    }

    .vertical-slider {
        appearance: slider-vertical;
        writing-mode: bt-lr;
        width: 24px;
        height: 100%;
    }

    .horizontal-slider {
        appearance: auto;
        width: 100%;
        height: 24px;
    }

    /* --- Track Styling --- */
    .vertical-slider::-webkit-slider-runnable-track {
        width: 6px;
        background: var(--border, #ddd);
        border-radius: 3px;
        margin-left: 9px;
    }
    .vertical-slider::-moz-range-track {
        width: 6px;
        height: 100%;
        background: var(--border, #ddd);
        border-radius: 3px;
    }
    .horizontal-slider::-webkit-slider-runnable-track {
        height: 6px;
        background: var(--border, #ddd);
        border-radius: 3px;
        margin-top: 9px;
    }
    .horizontal-slider::-moz-range-track {
        height: 6px;
        width: 100%;
        background: var(--border, #ddd);
        border-radius: 3px;
    }

    /* --- Thumb Styling --- */
    .slider::-webkit-slider-thumb {
        appearance: none;
        width: 16px;
        height: 16px;
        background: var(--button-bg, #eee);
        border-radius: 50%;
        border: 1px solid var(--button-border, #ccc);
        box-shadow: none;
        cursor: pointer;
    }
    .vertical-slider::-webkit-slider-thumb {
        margin-left: -5px;
    }
    .horizontal-slider::-webkit-slider-thumb {
        margin-top: -5px;
    }

    .slider::-moz-range-thumb {
        appearance: none;
        width: 16px;
        height: 16px;
        background: var(--button-bg, #eee);
        border: 1px solid var(--button-border, #ccc);
        box-shadow: none;
        border-radius: 50%;
        cursor: pointer;
    }

    .slider-value {
        font-family: monospace;
        font-size: 0.75em;
        background-color: var(--muted, #eee);
        color: var(--muted-foreground, #333);
        padding: 0.2em 0.4em;
        border-radius: 4px;
        min-width: 2.8em;
        text-align: center;
        flex-shrink: 0;
    }
    .slider-container.vertical .slider-value {
        margin-top: 1rem;
    }
    .slider-container.horizontal .slider-value {
        margin-left: 1rem;
    }

    @media (prefers-color-scheme: dark) {
        .slider-wrapper {
            /* Changed from .vertical-slider-wrapper */
            --muted-foreground: #aaa;
            --border: #444;
            --primary: #8ab4f8;
            --muted: #444;
            --button-bg: #555;
            --button-border: #777;
        }
        .slider-value {
            color: var(--foreground, #eee);
        }
    }
</style>
