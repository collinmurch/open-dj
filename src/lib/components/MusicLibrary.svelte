<script lang="ts">
    import { libraryStore } from "$lib/stores/libraryStore";
    import type { TrackInfo } from "$lib/types";

    // Access store functions for event handlers
    const { selectLibraryFolder, setSelectedTrack } = libraryStore;

    // Access store state reactively using $ prefix
    // Note: $libraryStore gives the *value* of the store, which is our LibraryState object.

    function handleTrackClick(track: TrackInfo) {
        setSelectedTrack(track);
    }

    // Check if a track is currently selected using the reactive store value
    function isSelected(track: TrackInfo): boolean {
        return $libraryStore.selectedTrack?.path === track.path;
    }
</script>

<div class="music-library">
    <!-- Button is always visible -->
    <button onclick={selectLibraryFolder} disabled={$libraryStore.isLoading}>
        {#if $libraryStore.isLoading}
            Loading Folder...
        {:else if $libraryStore.selectedFolder}
            Change Music Folder
        {:else}
            Select Music Folder
        {/if}
    </button>

    <!-- Show general error -->
    {#if $libraryStore.error}
        <p class="error-message">Error: {$libraryStore.error}</p>
    {/if}

    <!-- Conditional section for folder info and track list -->
    {#if $libraryStore.selectedFolder && !$libraryStore.isLoading}
        <div class="library-content">
            <p class="folder-info">Library: {$libraryStore.selectedFolder}</p>
            {#if $libraryStore.audioFiles.length > 0}
                <ul class="track-list">
                    {#each $libraryStore.audioFiles as track (track.path)}
                        <li class:selected-li={isSelected(track)}>
                            <button
                                class:selected={isSelected(track)}
                                onclick={() => handleTrackClick(track)}
                                onkeydown={(e) =>
                                    e.key === "Enter" &&
                                    handleTrackClick(track)}
                                aria-pressed={isSelected(track)}
                                aria-label={`Select track ${track.name}`}
                            >
                                <span class="track-name">{track.name}</span>
                                {#if track.bpm === undefined}
                                    <span class="track-bpm track-bpm-loading"
                                        >Calculating...</span
                                    >
                                {:else if track.bpm === null}
                                    <span class="track-bpm track-bpm-error"
                                        >Error</span
                                    >
                                {:else if typeof track.bpm === "number"}
                                    <span class="track-bpm"
                                        >{track.bpm.toFixed(1)} BPM</span
                                    >
                                {/if}
                            </button>
                        </li>
                    {/each}
                </ul>
            {:else if !$libraryStore.error}
                <!-- Only show 'no tracks' if there wasn't a folder error -->
                <p class="no-tracks">
                    No compatible audio files found in this folder.
                </p>
            {/if}
        </div>
    {/if}
</div>

<style>
    .music-library {
        padding: 1rem;
        border: 1px solid var(--border-color, #ccc);
        border-radius: 8px;
        background-color: var(--library-bg, #f0f0f0);
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    .library-content {
        max-height: 35vh;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    button {
        /* Style for the main folder select button */
        padding: 0.6em 1.2em;
        font-size: 1em;
        font-weight: 500;
        font-family: inherit;
        background-color: var(--button-bg, #eee);
        color: var(--button-text, #333);
        cursor: pointer;
        border: 1px solid transparent;
        border-radius: 8px;
        transition:
            border-color 0.25s,
            background-color 0.25s;
        align-self: flex-start;
    }
    button:hover {
        border-color: #646cff;
        background-color: #f9f9f9;
    }
    button:focus,
    button:focus-visible {
        outline: 4px auto -webkit-focus-ring-color;
    }
    button:disabled {
        opacity: 0.6;
        cursor: not-allowed;
    }

    .folder-info {
        font-style: italic;
        font-size: 0.9em;
        color: var(--text-muted, #555);
        word-break: break-all;
        margin-top: 0;
    }

    .track-list {
        list-style: none;
        padding: 0;
        margin: 0;
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .track-list li {
        border-radius: 4px;
        position: relative;
    }

    .track-list li button {
        display: flex;
        justify-content: space-between;
        align-items: center;
        width: 100%;
        padding: 0.5rem 0.75rem;
        border: 1px solid var(--border-color, #ddd);
        border-radius: 4px;
        cursor: pointer;
        transition:
            background-color 0.2s,
            border-color 0.2s;
        background-color: var(--track-item-bg, #fff);
        color: var(--text-color, #333);
        text-align: left;
        font-family: inherit;
        font-size: inherit;
    }

    .track-name {
        flex-grow: 1;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        margin-right: 0.5rem;
    }

    .track-bpm {
        font-size: 0.85em;
        font-style: italic;
        color: var(--text-muted, #666);
        flex-shrink: 0;
    }

    .track-bpm-loading {
        color: var(--text-muted-light, #999);
    }

    .track-bpm-error {
        color: var(--error-text-light, #e74c3c);
        font-weight: bold;
    }

    .track-list li button:hover {
        background-color: var(--track-item-hover-bg, #eee);
        border-color: #bbb;
    }

    .track-list li button.selected {
        background-color: var(--track-item-selected-bg, #cfe2ff);
        border-color: #9ec5fe;
        font-weight: bold;
    }

    .track-list li button:focus {
        outline: none;
    }
    .track-list li button:focus-visible {
        outline: 2px solid #646cff;
        outline-offset: 1px;
    }

    .error-message {
        color: var(--error-text, #d9534f);
        font-size: 0.9em;
        text-align: center;
        padding: 0.5rem;
        background-color: #fff0f0;
        border: 1px solid #ffcccc;
        border-radius: 4px;
    }

    .no-tracks {
        color: var(--text-muted, #555);
        font-size: 0.9em;
        text-align: center;
    }

    @media (prefers-color-scheme: dark) {
        .music-library {
            --border-color: #444;
            --library-bg: #2a2a2a;
            --text-muted: #aaa;
            --text-color: #eee;
            --button-bg: #444;
            --button-text: #eee;
            --track-item-bg: #383838;
            --track-item-hover-bg: #484848;
            --track-item-selected-bg: #0a2d57;
            --error-text: #f48481;
            --error-border: #8b4b4b;
        }
        .error-message {
            background-color: #4d3333;
            border-color: #8b4b4b;
        }
        button:hover {
            border-color: #adbac7;
            background-color: #555;
        }
        .track-list li button {
            border-color: var(--border-color, #555);
            background-color: var(--track-item-bg, #383838);
            color: var(--text-color, #eee);
        }
        .track-list li button:hover {
            background-color: var(--track-item-hover-bg, #484848);
            border-color: #777;
        }
        .track-list li button.selected {
            background-color: var(--track-item-selected-bg, #0a2d57);
            border-color: #2a6bba;
        }
        .track-bpm {
            color: var(--text-muted, #aaa);
        }
        .track-bpm-loading {
            color: var(--text-muted-light, #777);
        }
        .track-bpm-error {
            color: var(--error-text-light, #ff7f7f);
        }
    }
</style>
