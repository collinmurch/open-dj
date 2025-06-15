<script lang="ts">
    import Slider from "./Slider.svelte";
    import { syncStore } from "$lib/stores/syncStore";

    let {
        value = $bindable(0.5),
    }: {
        value?: number;
    } = $props();

    const CROSSFADER_TOLERANCE = 1e-5;
    
    // Simplified sync - only sync from local to store, no bidirectional sync
    $effect(() => {
        const localValue = value;
        const storeValue = $syncStore.crossfaderValue;
        
        // Only update store if local value differs significantly
        if (Math.abs(localValue - storeValue) > CROSSFADER_TOLERANCE) {
            syncStore.setCrossfader(localValue);
        }
    });
</script>

<div class="crossfader-container">
    <Slider
        id="crossfader"
        label="Crossfader"
        orientation="horizontal"
        outputMin={0}
        outputMax={1}
        centerValue={0.5}
        step={0.01}
        bind:value
    />
</div>

<style>
    .crossfader-container {
        width: 100%;
        max-width: 50%;
        margin-left: auto;
        margin-right: auto;
        padding: 0;
        margin-top: 0;
    }
</style>