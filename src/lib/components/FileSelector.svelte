<script lang="ts">
    import {
        audioError,
        selectedFile,
        selectMp3File,
        audioUrl,
    } from "$lib/stores/audioStore";

    // Add debug logging
    $effect(() => {
        console.log("FileSelector - selectedFile:", $selectedFile);
        console.log("FileSelector - audioUrl:", $audioUrl);
    });

    async function handleSelectFile() {
        console.log("Select button clicked");
        await selectMp3File();
    }
</script>

<div class="file-selector">
    <button class="select-button" onclick={handleSelectFile}>
        Select MP3 File
    </button>

    {#if $selectedFile}
        <div class="file-info">
            <p>Selected file: {$selectedFile.name}</p>
        </div>
    {/if}

    {#if $audioError}
        <div class="error">
            <p>{$audioError}</p>
        </div>
    {/if}
</div>

<style>
    .file-selector {
        width: 100%;
        max-width: 600px;
        margin: 0 auto;
        padding: 1rem;
        background: var(--background-color);
        border-radius: 8px;
        box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
    }

    .select-button {
        background: var(--button-background);
        color: var(--button-text);
        border: none;
        padding: 0.5rem 1rem;
        border-radius: 4px;
        cursor: pointer;
        font-size: 1rem;
        transition: background-color 0.2s;
    }

    .select-button:hover {
        background: var(--button-hover);
    }

    .file-info {
        margin-top: 1rem;
        padding: 0.5rem;
        background: var(--info-background);
        border-radius: 4px;
    }

    .error {
        margin-top: 1rem;
        padding: 0.5rem;
        background: var(--error-background);
        color: var(--error-text);
        border-radius: 4px;
    }

    @media (prefers-color-scheme: dark) {
        .file-selector {
            --background-color: #2a2a2a;
            --button-background: #4a9eff;
            --button-text: #fff;
            --button-hover: #3a8eef;
            --info-background: rgba(255, 255, 255, 0.1);
            --error-background: rgba(255, 0, 0, 0.2);
            --error-text: #ff6b6b;
        }
    }

    @media (prefers-color-scheme: light) {
        .file-selector {
            --background-color: #fff;
            --button-background: #0066cc;
            --button-text: #fff;
            --button-hover: #0055bb;
            --info-background: rgba(0, 0, 0, 0.05);
            --error-background: rgba(255, 0, 0, 0.1);
            --error-text: #cc0000;
        }
    }
</style>
