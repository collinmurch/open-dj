<script lang="ts">
    import Slider from "./Slider.svelte";
    import { syncStore } from "$lib/stores/syncStore";

    let {
        value = $bindable(0.5),
    }: {
        value?: number;
    } = $props();

    const CROSSFADER_TOLERANCE = 1e-5;
    
    // Prevent circular updates by tracking last synced value
    let lastSyncedValue = $state<number | null>(null);
    $effect(() => {
        const localValue = value;
        
        // Only sync if value actually changed from our side (not from store updates)
        if (lastSyncedValue !== localValue && Math.abs(localValue - ($syncStore.crossfaderValue ?? 0.5)) > CROSSFADER_TOLERANCE) {
            lastSyncedValue = localValue;
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