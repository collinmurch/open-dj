<script lang="ts">
    import { onDestroy } from "svelte";

    let {
        id,
        label,
        min = 0,
        max = 100,
        step = 1,
        value = $bindable(), // Use bindable prop for two-way binding
        debounceMs = 50, // Debounce API calls
    }: {
        id: string;
        label: string;
        min?: number;
        max?: number;
        step?: number;
        value?: number;
        debounceMs?: number;
    } = $props();

    // Local state to reflect immediate input changes before debouncing
    let internalValue = $state(value ?? min);
    let debounceTimeoutId: number | undefined = $state(undefined);

    // Sync internalValue when the bound external value changes
    $effect(() => {
        internalValue = value ?? min;
    });

    // Effect to handle debounced updates to the bound value
    $effect(() => {
        // Capture the current internal value for the debounce function
        const valueToEmit = internalValue;

        // Clear any existing timeout
        if (debounceTimeoutId !== undefined) clearTimeout(debounceTimeoutId);

        // Set a new timeout
        debounceTimeoutId = setTimeout(() => {
            // Update the externally bound value after debounce
            if (value !== valueToEmit) {
                value = valueToEmit;
            }
        }, debounceMs);
    });

    // Cleanup timeout on component destroy
    onDestroy(() => {
        if (debounceTimeoutId !== undefined) clearTimeout(debounceTimeoutId);
    });
</script>

<div class="vertical-slider-wrapper">
    <label for={id} class="slider-label">{label}</label>
    <div class="slider-container">
        <input
            type="range"
            {id}
            {min}
            {max}
            {step}
            bind:value={internalValue}
            aria-label={label}
            class="vertical-slider"
        />
        <span class="slider-value"
            >{internalValue.toFixed(step < 1 ? 2 : 0)}</span
        >
    </div>
</div>

<style>
    .vertical-slider-wrapper {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 1rem; /* Increased gap for more space */
        padding: 0.5rem 0.5rem 0.25rem;
        min-width: 60px;
        height: 100%;
    }

    .slider-label {
        font-size: 0.8em;
        color: var(--muted-foreground, #555);
        text-align: center;
        flex-shrink: 0;
    }

    .slider-container {
        display: flex;
        flex-direction: column;
        align-items: center;
        flex-grow: 1;
        width: 100%;
        position: relative;
    }

    .vertical-slider {
        appearance: slider-vertical;
        writing-mode: bt-lr;
        width: 24px; /* Increase clickable width slightly */
        height: 100%;
        cursor: pointer;
        margin: 0;
        background: transparent; /* Make slider itself transparent */
        padding: 0; /* Remove padding */
    }

    /* --- Track Styling --- */
    .vertical-slider::-webkit-slider-runnable-track {
        width: 6px; /* Thinner track */
        background: var(--border, #ddd);
        border-radius: 3px;
        margin-left: 9px; /* Center track ( (24px - 6px) / 2 ) */
    }
    .vertical-slider::-moz-range-track {
        width: 6px; /* Thinner track */
        height: 100%;
        background: var(--border, #ddd);
        border-radius: 3px;
    }

    /* --- Thumb Styling --- */
    .vertical-slider::-webkit-slider-thumb {
        appearance: none; /* Force appearance reset */
        width: 16px;
        height: 16px;
        background: var(--button-bg, #eee); /* Match button background */
        border-radius: 50%; /* Make it a circle */
        border: 1px solid var(--button-border, #ccc); /* Add subtle border like buttons */
        box-shadow: none;
        cursor: pointer;
        margin-left: -5px;
    }
    .vertical-slider::-moz-range-thumb {
        appearance: none; /* Force appearance reset */
        width: 16px;
        height: 16px;
        background: var(--button-bg, #eee); /* Match button background */
        border: 1px solid var(--button-border, #ccc); /* Add subtle border like buttons */
        box-shadow: none;
        border-radius: 50%; /* Make it a circle */
        cursor: pointer;
    }

    .slider-value {
        font-family: monospace;
        font-size: 0.75em;
        margin-top: 1rem; /* Add margin above the value */
        background-color: var(--muted, #eee);
        color: var(--muted-foreground, #333);
        padding: 0.2em 0.4em;
        border-radius: 4px;
        min-width: 2.8em;
        text-align: center;
        flex-shrink: 0; /* Prevent value from shrinking */
    }

    @media (prefers-color-scheme: dark) {
        .vertical-slider-wrapper {
            --muted-foreground: #aaa;
            --border: #444; /* Example dark border color */
            --primary: #8ab4f8; /* Example dark primary */
            --muted: #444; /* Example dark muted bg */
            --button-bg: #555; /* Define button bg for dark */
            --button-border: #777; /* Define button border for dark */
        }
        /* Track and thumb colors inherit via CSS vars */
        .slider-value {
            color: var(--foreground, #eee);
        }
    }
</style>
