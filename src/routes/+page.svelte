<script lang="ts">
    import AudioDeviceSelector from "$lib/components/AudioDeviceSelector.svelte";
    import Crossfader from "$lib/components/Crossfader.svelte";
    import DeckController from "$lib/components/DeckController.svelte";
    import MusicLibrary from "$lib/components/MusicLibrary.svelte";
    import WaveformDisplay from "$lib/components/WaveformDisplay.svelte";
    import { deckAStore, deckBStore } from "$lib/stores/deckStore";
    import { libraryStore } from "$lib/stores/libraryStore";
    import { createPlayerStore } from "$lib/stores/playerStore";
    import { syncStore } from "$lib/stores/syncStore";

    // --- Library State ---
    const selectedTrack = $derived($libraryStore.selectedTrack);
    const isFolderSelected = $derived(!!$libraryStore.selectedFolder);

    // --- Player Store Instances ---
    const playerStoreA = createPlayerStore("A");
    const playerStoreB = createPlayerStore("B");

    // --- Deck State ---
    const deckAState = $derived($deckAStore);
    const deckBState = $derived($deckBStore);
    const deckAPlayerState = $derived($playerStoreA);
    const deckBPlayerState = $derived($playerStoreB);

    // --- Cue Audio Control ---
    let cueAudioDeck = $state<"A" | "B" | null>(null);

    // --- Crossfader State ---
    let crossfaderValue = $state($syncStore.crossfaderValue);

    // --- Waveform Colors ---
    const deckAColors = {
        low: [0.3, 0.2, 0.6] as [number, number, number],
        mid: [0.48, 0.38, 0.72] as [number, number, number],
        high: [0.55, 0.55, 0.85] as [number, number, number],
    };

    const deckBColors = {
        low: [0.475, 0.5, 0.525] as [number, number, number],
        mid: [0.66, 0.7, 0.74] as [number, number, number],
        high: [0.88, 0.9, 0.92] as [number, number, number],
    };

    // Track info lookup using Svelte 5 runes
    const trackInfoA = $derived.by(() => {
        const deckState = $deckAStore;
        const libraryState = $libraryStore;
        if (!deckState.filePath) return undefined;
        return libraryState.audioFiles.find(track => track.path === deckState.filePath);
    });

    const trackInfoB = $derived.by(() => {
        const deckState = $deckBStore;
        const libraryState = $libraryStore;
        if (!deckState.filePath) return undefined;
        return libraryState.audioFiles.find(track => track.path === deckState.filePath);
    });






    // --- BPM Calculations (removed - now handled in DeckController) ---
    // BPM calculation moved to DeckController to avoid duplication and circular dependencies

    // --- Track Loading Functions ---
    async function loadTrackToDeck(deckId: "A" | "B") {
        if (!selectedTrack) return;

        const deckStore = deckId === "A" ? deckAStore : deckBStore;
        const playerStore = deckId === "A" ? playerStoreA : playerStoreB;

        // Skip if same track is already loaded
        if (deckStore.get().filePath === selectedTrack.path) {
            console.log(`[Page] Track ${selectedTrack.path} is already loaded on Deck ${deckId}. Skipping reload.`);
            return;
        }

        const bpm = selectedTrack.metadata?.bpm ?? null;
        const firstBeat = selectedTrack.metadata?.firstBeatSec ?? null;

        // Load track in both stores
        await deckStore.loadTrackFromLibrary(selectedTrack);
        playerStore.loadTrack(selectedTrack.path, bpm, firstBeat);
    }

    // --- Seek Functions ---
    function seekDeckA(time: number) {
        playerStoreA.seek(time);
    }

    function seekDeckB(time: number) {
        playerStoreB.seek(time);
    }
</script>

<main class="container">
    {#if !isFolderSelected}
        <section class="library-section library-section-initial">
            <h2>Music Library</h2>
            <MusicLibrary />
        </section>
    {:else}
        <!-- Mixer Section with Waveforms and Crossfader -->
        <section class="mixer-section">
            <!-- Deck A Waveform -->
            <div class="waveform-container deck-a-style">
                <WaveformDisplay
                    volumeAnalysis={deckAState.volumeAnalysis}
                    isAnalysisPending={deckAState.isWaveformLoading}
                    isTrackLoaded={!!deckAState.filePath}
                    audioDuration={deckAPlayerState.duration}
                    currentTime={deckAPlayerState.currentTime}
                    isPlaying={deckAPlayerState.isPlaying}
                    seekAudio={seekDeckA}
                    cuePointTime={deckAPlayerState.cuePointTime}
                    lowBandColor={deckAColors.low}
                    midBandColor={deckAColors.mid}
                    highBandColor={deckAColors.high}
                    eqParams={deckAState.eqParams}
                    faderLevel={deckAState.faderLevel}
                    pitchRate={deckAPlayerState.pitchRate ?? 1.0}
                    firstBeatSec={trackInfoA?.metadata?.firstBeatSec}
                    bpm={trackInfoA?.metadata?.bpm}
                />
            </div>

            <!-- Deck B Waveform -->
            <div class="waveform-container deck-b-style">
                <WaveformDisplay
                    volumeAnalysis={deckBState.volumeAnalysis}
                    isAnalysisPending={deckBState.isWaveformLoading}
                    isTrackLoaded={!!deckBState.filePath}
                    audioDuration={deckBPlayerState.duration}
                    currentTime={deckBPlayerState.currentTime}
                    isPlaying={deckBPlayerState.isPlaying}
                    seekAudio={seekDeckB}
                    cuePointTime={deckBPlayerState.cuePointTime}
                    lowBandColor={deckBColors.low}
                    midBandColor={deckBColors.mid}
                    highBandColor={deckBColors.high}
                    eqParams={deckBState.eqParams}
                    faderLevel={deckBState.faderLevel}
                    pitchRate={deckBPlayerState.pitchRate ?? 1.0}
                    firstBeatSec={trackInfoB?.metadata?.firstBeatSec}
                    bpm={trackInfoB?.metadata?.bpm}
                />
            </div>

            <!-- Crossfader -->
            <Crossfader bind:value={crossfaderValue} />
        </section>

        <!-- Deck Controls Section -->
        <section class="decks-section-horizontal">
            <DeckController
                deckId="A"
                bind:cueAudioDeck
                {crossfaderValue}
                playerStore={playerStoreA}
                onLoadTrack={loadTrackToDeck}
            />
            <DeckController
                deckId="B"
                bind:cueAudioDeck
                {crossfaderValue}
                playerStore={playerStoreB}
                onLoadTrack={loadTrackToDeck}
            />
        </section>

        <!-- Library Section -->
        <section class="library-section library-section-expanded">
            <h2>Music Library</h2>
            <div class="library-content">
                <div class="music-library-column">
                    <div class="load-controls">
                        <button
                            class="load-deck-a-button"
                            onclick={() => loadTrackToDeck("A")}
                            disabled={!selectedTrack}
                        >
                            Load Selected to Deck A
                        </button>
                        <button
                            class="load-deck-b-button"
                            onclick={() => loadTrackToDeck("B")}
                            disabled={!selectedTrack}
                        >
                            Load Selected to Deck B
                        </button>
                    </div>
                    <MusicLibrary />
                </div>
                <div class="audio-device-column">
                    <AudioDeviceSelector title="Cue Output" />
                </div>
            </div>
        </section>
    {/if}
</main>

<style>
    :root {
        --section-border: #ddd;
        --section-bg: #fff;
        --waveform-area-bg: #e9e9e9;
        --deck-bg: transparent;
        --section-border-light: #eee;

        /* Deck A - Light Theme */
        --deck-a-waveform-bg-light: hsl(255, 45%, 75%);
        --deck-a-border-light: hsl(255, 40%, 60%);
        --deck-a-deck-bg-light: hsl(255, 50%, 80%);
        --deck-a-waveform-fill-light: hsl(255, 60%, 90%);
        --deck-a-button-bg-light: hsl(255, 50%, 65%);
        --deck-a-button-text-light: hsl(255, 100%, 98%);
        --deck-a-button-hover-bg-light: hsl(255, 55%, 75%);

        /* Deck B - Light Theme */
        --deck-b-waveform-bg-light: hsl(210, 25%, 97%);
        --deck-b-border-light: hsl(210, 15%, 88%);
        --deck-b-deck-bg-light: hsl(210, 30%, 99%);
        --deck-b-waveform-fill-light: hsl(210, 15%, 75%);
        --deck-b-button-bg-light: hsl(210, 20%, 85%);
        --deck-b-button-text-light: hsl(210, 20%, 25%);
        --deck-b-button-hover-bg-light: hsl(210, 25%, 90%);
    }

    .library-section-initial {
        align-items: center;
        flex-grow: 1;
        display: flex;
        flex-direction: column;
        justify-content: center;
        min-height: 80vh;
    }

    main.container {
        margin: 0 auto;
        padding: 2rem;
        padding-top: 1.5rem;
        display: flex;
        flex-direction: column;
        gap: 1.5rem;
        min-height: 95vh;
        width: 100%;
        max-width: 1800px;
    }

    .mixer-section {
        width: 100%;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        padding: 1rem;
        border: 1px solid var(--section-border);
        border-radius: 8px;
        background-color: var(--section-bg);
    }

    .waveform-container {
        width: 100%;
        height: 80px;
        border-radius: 4px;
        overflow: hidden;
        background-color: var(--waveform-area-bg);
        transition: background-color 0.3s ease;
        margin-bottom: 0.1rem;
    }

    .waveform-container.deck-a-style {
        background-color: var(--deck-a-waveform-bg-light);
    }

    .waveform-container.deck-b-style {
        background-color: var(--deck-b-waveform-bg-light);
    }

    .decks-section-horizontal {
        display: flex;
        flex-direction: row;
        justify-content: center;
        align-items: flex-start;
        gap: 1.5rem;
        width: 100%;
    }


    .library-section {
        flex-grow: 0;
        flex-shrink: 0;
        display: flex;
        flex-direction: column;
        width: 100%;
        border: 1px solid var(--section-border);
        border-radius: 8px;
        padding: 1.5rem;
        background-color: var(--section-bg);
        gap: 1rem;
    }

    .library-section-expanded {
        max-height: none;
        overflow: visible;
    }

    .library-content {
        display: grid;
        grid-template-columns: 1fr 240px;
        gap: 1.5rem;
        align-items: start;
        width: 100%;
    }

    .music-library-column {
        display: flex;
        flex-direction: column;
        gap: 1rem;
        min-width: 0;
    }

    h2 {
        margin-top: 0;
        margin-bottom: 1rem;
        border-bottom: 1px solid var(--section-border);
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
        color: #333;
        border: 1px solid #ccc;
        border-radius: 4px;
        cursor: pointer;
        transition: background-color 0.2s;
    }

    .load-controls button:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .load-controls .load-deck-a-button {
        background-color: var(--deck-a-button-bg-light);
        color: var(--deck-a-button-text-light);
        border-color: var(--deck-a-border-light);
    }

    .load-controls .load-deck-a-button:hover:not(:disabled) {
        background-color: limegreen;
    }

    .load-controls .load-deck-b-button {
        background-color: var(--deck-b-button-bg-light);
        color: var(--deck-b-button-text-light);
        border-color: var(--deck-b-border-light);
    }

    .load-controls .load-deck-b-button:hover:not(:disabled) {
        background-color: var(--deck-b-button-hover-bg-light);
    }

    @media (prefers-color-scheme: dark) {
        :root {
            --section-border: #555;
            --section-bg: #3a3a3a;
            --waveform-area-bg: #303030;
            --deck-bg: #333;
            --section-border-light: #444;

            /* Deck A - Dark Theme */
            --deck-a-waveform-bg-dark: hsl(260, 35%, 12%);
            --deck-a-border-dark: hsl(260, 40%, 20%);
            --deck-a-deck-bg-dark: hsl(260, 30%, 10%);
            --deck-a-waveform-fill-light: hsl(260, 45%, 35%);
            --deck-a-button-bg-dark: hsl(260, 40%, 25%);
            --deck-a-button-text-dark: hsl(260, 80%, 90%);
            --deck-a-button-hover-bg-light: hsl(260, 45%, 35%);

            /* Deck B - Dark Theme */
            --deck-b-waveform-bg-dark: hsl(210, 12%, 20%);
            --deck-b-border-dark: hsl(210, 15%, 30%);
            --deck-b-deck-bg-dark: hsl(210, 10%, 15%);
            --deck-b-waveform-fill-light: hsl(210, 15%, 45%);
            --deck-b-button-bg-dark: hsl(210, 15%, 25%);
            --deck-b-button-text-dark: hsl(210, 25%, 85%);
            --deck-b-button-hover-bg-light: hsl(210, 20%, 35%);
        }

        .load-controls button {
            background-color: #555;
            border-color: #777;
            color: #eee;
        }

        .load-controls button:hover:not(:disabled) {
            background-color: #666;
        }

        .load-controls .load-deck-a-button {
            background-color: var(--deck-a-button-bg-dark);
            color: var(--deck-a-button-text-dark);
            border-color: var(--deck-a-border-dark);
        }

        .load-controls .load-deck-a-button:hover:not(:disabled) {
            background-color: var(--deck-a-button-hover-bg-light);
        }

        .load-controls .load-deck-b-button {
            background-color: var(--deck-b-button-bg-dark);
            color: var(--deck-b-button-text-dark);
            border-color: var(--deck-b-border-dark);
        }

        .load-controls .load-deck-b-button:hover:not(:disabled) {
            background-color: var(--deck-b-button-hover-bg-light);
        }

        .waveform-container.deck-a-style {
            background-color: var(--deck-a-waveform-bg-dark);
        }

        .waveform-container.deck-b-style {
            background-color: var(--deck-b-waveform-bg-dark);
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

        --text-color: #0f0f0f;
        --bg-color: #f6f6f6;
    }

    @media (prefers-color-scheme: dark) {
        :root {
            --text-color: #f6f6f6;
            --bg-color: #2f2f2f;
        }
    }
</style>
