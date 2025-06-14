<script lang="ts">
    import type { AudioDeviceState, AudioDevice } from "$lib/types";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    
    let { 
        title = "Cue Output",
        disabled = false 
    } = $props<{
        title?: string;
        disabled?: boolean;
    }>();
    
    let deviceState = $state<AudioDeviceState | null>(null);
    let isLoading = $state(true);
    let error = $state<string | null>(null);
    
    const selectedDevice = $derived.by(() => {
        if (!deviceState) return null;
        return deviceState.selection.cueOutput;
    });
    
    let outputDevices = $derived.by(() => {
        if (!deviceState?.devices?.outputDevices) return [];
        return deviceState.devices.outputDevices;
    });
    
    onMount(async () => {
        await loadDevices();
    });
    
    async function loadDevices() {
        try {
            isLoading = true;
            error = null;
            const result = await invoke<AudioDeviceState>("get_audio_devices");
            deviceState = result;
        } catch (err) {
            error = `Failed to load audio devices: ${err}`;
            console.error("AudioDeviceSelector - error loading devices:", error, err);
        } finally {
            isLoading = false;
        }
    }
    
    async function selectDevice(deviceName: string | null) {
        if (disabled) return;
        
        try {
            error = null;
            const command = "set_cue_output_device";
            
            // Optimistically update local state first
            if (deviceState) {
                // Create a new state object to trigger reactivity
                deviceState = {
                    ...deviceState,
                    selection: {
                        ...deviceState.selection,
                        cueOutput: deviceName
                    }
                };
            }
            
            await invoke(command, { deviceName });
            
        } catch (err) {
            error = `Failed to select device: ${err}`;
            console.error(error);
            // On error, reload to get the correct state
            await loadDevices();
        }
    }
    
    async function refreshDevices() {
        if (disabled) return;
        
        try {
            error = null;
            const result = await invoke<AudioDeviceState>("refresh_audio_devices");
            deviceState = result;
        } catch (err) {
            error = `Failed to refresh devices: ${err}`;
            console.error(error);
        }
    }
</script>

<div class="device-selector" class:disabled>
    <div class="header">
        <h3>{title}</h3>
        <button 
            class="refresh-btn"
            onclick={refreshDevices}
            disabled={disabled || isLoading}
            title="Refresh device list"
        >
            ↻
        </button>
    </div>
    
    {#if isLoading}
        <div class="loading">Loading devices...</div>
    {:else if error}
        <div class="error">{error}</div>
    {:else}
        <div class="device-list">
            <!-- Available devices -->
            {#each outputDevices as device (device.name)}
                <button 
                    class="device-option"
                    class:selected={selectedDevice === device.name}
                    class:default-device={device.isDefault}
                    onclick={() => selectDevice(device.name)}
                    disabled={disabled}
                >
                    <span class="device-name">{device.name}</span>
                    {#if device.isDefault}
                        <span class="default-badge">Default</span>
                    {/if}
                </button>
            {/each}
            
            {#if outputDevices.length === 0}
                <div class="no-devices">No output devices found</div>
            {/if}
        </div>
    {/if}
</div>

<style>
    .device-selector {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 1rem;
        border: 1px solid var(--section-border, #ddd);
        border-radius: 8px;
        background-color: var(--section-bg, #fff);
        width: 100%;
        max-width: 240px;
        color: var(--text-color, #333);
        box-sizing: border-box;
        overflow: hidden;
    }
    
    .device-selector.disabled {
        opacity: 0.6;
        pointer-events: none;
    }
    
    .header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        margin-bottom: 0.5rem;
    }
    
    .header h3 {
        margin: 0;
        font-size: 0.9rem;
        font-weight: 600;
        color: var(--text-color, #333);
    }
    
    .refresh-btn {
        background: none;
        border: 1px solid var(--section-border, #ddd);
        border-radius: 4px;
        padding: 0.25rem 0.5rem;
        cursor: pointer;
        font-size: 0.8rem;
        color: var(--text-color, #666);
        transition: background-color 0.2s;
    }
    
    .refresh-btn:hover:not(:disabled) {
        background-color: var(--waveform-area-bg, #f0f0f0);
    }
    
    .refresh-btn:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    
    .loading, .error, .no-devices {
        padding: 0.75rem;
        text-align: center;
        font-size: 0.85rem;
        border-radius: 4px;
    }
    
    .loading {
        color: var(--text-color, #666);
        background-color: var(--waveform-area-bg, #f0f0f0);
    }
    
    .error {
        color: #d32f2f;
        background-color: #ffebee;
        border: 1px solid #ffcdd2;
    }
    
    .no-devices {
        color: var(--text-color, #888);
        background-color: var(--waveform-area-bg, #f0f0f0);
    }
    
    .device-list {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }
    
    .device-option {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 0.5rem 0.75rem;
        background: var(--section-bg, #fff);
        border: 1px solid var(--section-border-light, #eee);
        border-radius: 4px;
        cursor: pointer;
        transition: all 0.2s;
        text-align: left;
        min-height: 2.5rem;
        color: var(--text-color, #333);
        width: 100%;
        box-sizing: border-box;
        overflow: hidden;
    }
    
    .device-option:hover:not(:disabled) {
        background-color: var(--waveform-area-bg, #f0f0f0);
        border-color: var(--section-border, #ddd);
    }
    
    .device-option.selected {
        background-color: #e0e0e0;
        color: #333;
        border-color: #bbb;
    }
    
    .device-option:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    
    .device-name {
        font-size: 0.8rem;
        font-weight: 500;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: inherit;
        min-width: 0;
    }
    
    .default-info, .default-badge {
        font-size: 0.75rem;
        opacity: 0.8;
        margin-left: 0.5rem;
        flex-shrink: 0;
    }
    
    .default-badge {
        background-color: var(--deck-b-button-bg-light, #90a4ae);
        color: var(--deck-b-button-text-light, white);
        padding: 0.125rem 0.375rem;
        border-radius: 10px;
        font-weight: 500;
    }
    
    .device-option.selected .default-badge {
        background-color: rgba(255, 255, 255, 0.2);
        color: inherit;
    }
    
    @media (prefers-color-scheme: dark) {
        .error {
            color: #f48fb1;
            background-color: #3c1f28;
            border-color: #5d2f3a;
        }
        
        .device-option.selected {
            background-color: #555;
            color: #ddd;
            border-color: #777;
        }
        
        .default-badge {
            background-color: var(--deck-b-button-bg-dark, #455a64);
            color: var(--deck-b-button-text-dark, #cfd8dc);
        }
    }
</style>