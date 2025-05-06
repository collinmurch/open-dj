<script lang="ts">
    import MusicLibrary from "$lib/components/MusicLibrary.svelte";
    import TrackPlayer from "$lib/components/TrackPlayer.svelte";
    import { libraryStore } from "$lib/stores/libraryStore";

    let deckAFilePath = $state<string | null>(null);
    let deckBFilePath = $state<string | null>(null);

    const selectedTrack = $derived($libraryStore.selectedTrack);
    const isFolderSelected = $derived(!!$libraryStore.selectedFolder);

    // Explicitly type the deck IDs for clarity
    const deckAId: string = "A";
    const deckBId: string = "B";

    function loadToDeckA() {
        if (selectedTrack) deckAFilePath = selectedTrack.path;
    }

    function loadToDeckB() {
        if (selectedTrack) deckBFilePath = selectedTrack.path;
    }
</script>

<main class="container">
    {#if !isFolderSelected}
        <!-- Initial State: Show only the Music Library selection button -->
        <section class="library-section library-section-initial">
            <h2>Music Library</h2>
            <MusicLibrary />
        </section>
    {/if}

    {#if isFolderSelected}
        <!-- After Folder Selection: Show Decks and expanded Library -->
        <section class="decks-section">
            <h2>Decks</h2>
            <div class="decks-container">
                <div class="deck">
                    <h3>Deck A</h3>
                    <TrackPlayer filePath={deckAFilePath} deckId={deckAId} />
                </div>
                <div class="deck">
                    <h3>Deck B</h3>
                    <TrackPlayer filePath={deckBFilePath} deckId={deckBId} />
                </div>
            </div>
        </section>

        <section class="library-section">
            <h2>Music Library</h2>
            <div class="load-controls">
                <button onclick={loadToDeckA} disabled={!selectedTrack}
                    >Load Selected to Deck A</button
                >
                <button onclick={loadToDeckB} disabled={!selectedTrack}
                    >Load Selected to Deck B</button
                >
            </div>

            <MusicLibrary />
        </section>
    {/if}
</main>

<style>
    .library-section-initial {
        align-items: center;
        flex-grow: 1;
    }

    main.container {
        margin: 0 auto;
        padding: 2rem;
        padding-top: 3vh;
        display: flex;
        flex-direction: column;
        gap: 2rem;
        min-height: 90vh;
    }

    .decks-section {
        flex-grow: 1;
        display: flex;
        flex-direction: column;
    }

    .library-section {
        flex-grow: 0;
        flex-shrink: 0;
        max-height: 40vh;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        width: 100%;
        border: 1px solid var(--section-border, #ddd);
        border-radius: 8px;
        padding: 1.5rem;
        background-color: var(--section-bg, #fff);
        gap: 1rem;
    }
    h2 {
        margin-top: 0;
        margin-bottom: 1rem;
        border-bottom: 1px solid var(--section-border, #ddd);
        padding-bottom: 0.5rem;
        text-align: center;
    }

    .load-controls {
        display: flex;
        flex-wrap: wrap;
        justify-content: center;
        align-items: center;
        gap: 1rem;
        margin-bottom: 1rem;
    }
    .load-controls button {
        padding: 0.5em 1em;
        font-size: 0.9em;
        background-color: #e0e0e0;
        border: 1px solid #ccc;
        border-radius: 4px;
        cursor: pointer;
        transition: background-color 0.2s;
    }
    .load-controls button:hover:not(:disabled) {
        background-color: #d0d0d0;
    }
    .load-controls button:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .decks-container {
        width: 100%;
        display: flex;
        flex-direction: row;
        justify-content: space-around;
        align-items: stretch;
        gap: 3rem;
        flex-wrap: wrap;
    }
    .deck {
        flex: 1;
        min-width: 300px;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.5rem;
    }
    .deck h3 {
        margin-bottom: 0.5rem;
        font-size: 1.1em;
    }

    @media (prefers-color-scheme: dark) {
        :root {
            --text-color: #f6f6f6;
            --bg-color: #2f2f2f;
            --section-border: #555;
            --section-border-light: #444;
            --section-bg: #3a3a3a;
            --text-muted: #bbb;
        }
        .load-controls button {
            background-color: #555;
            border-color: #777;
            color: #eee;
        }
        .load-controls button:hover:not(:disabled) {
            background-color: #666;
        }
    }

    :root {
        *,
        *::before,
        *::after {
            box-sizing: border-box;
        }
        font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
        font-size: 16px;
        line-height: 24px;
        font-weight: 400;
        color: var(--text-color, #0f0f0f);
        background-color: var(--bg-color, #f6f6f6);
        font-synthesis: none;
        text-rendering: optimizeLegibility;
        -webkit-font-smoothing: antialiased;
        -moz-osx-font-smoothing: grayscale;
        -webkit-text-size-adjust: 100%;
    }
</style>
