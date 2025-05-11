<script lang="ts">
    import DeckControls from "$lib/components/DeckControls.svelte";
    import MusicLibrary from "$lib/components/MusicLibrary.svelte";
    import Slider from "$lib/components/Slider.svelte";
    import WaveFormDisplay from "$lib/components/WaveformDisplay.svelte";
    import { libraryStore } from "$lib/stores/libraryStore";
    import {
        createPlayerStore,
        type PlayerStore,
    } from "$lib/stores/playerStore";
    import type { VolumeAnalysis, EqParams } from "$lib/types";
    import { invoke } from "@tauri-apps/api/core";

    // --- Library and Track Selection ---
    const selectedTrack = $derived($libraryStore.selectedTrack);
    const isFolderSelected = $derived(!!$libraryStore.selectedFolder);

    // --- Player Store Instances ---
    const playerStoreA: PlayerStore = createPlayerStore("A");
    const playerStoreB: PlayerStore = createPlayerStore("B");

    // --- Deck A: File Path, Analysis, EQ, Fader ---
    let deckAFilePath = $state<string | null>(null);
    let deckAVolumeAnalysis = $state<VolumeAnalysis | null>(null);
    let isDeckAWaveformLoading = $state(false);
    let deckAFaderLevel = $state(1.0);
    let deckAEqParams = $state<EqParams>({
        lowGainDb: 0.0,
        midGainDb: 0.0,
        highGainDb: 0.0,
    });

    // --- Deck B: File Path, Analysis, EQ, Fader ---
    let deckBFilePath = $state<string | null>(null);
    let deckBVolumeAnalysis = $state<VolumeAnalysis | null>(null);
    let isDeckBWaveformLoading = $state(false);
    let deckBFaderLevel = $state(1.0);
    let deckBEqParams = $state<EqParams>({
        lowGainDb: 0.0,
        midGainDb: 0.0,
        highGainDb: 0.0,
    });

    // --- Global Mixer Controls ---
    let crossfaderValue = $state(0.5);

    // --- Deck Volume Derivations (from Crossfader) ---
    const deckAVolume = $derived(() => {
        return 1 - crossfaderValue;
    });
    const deckBVolume = $derived(() => {
        return crossfaderValue;
    });

    // --- Waveform Colors ---
    // const deckAWaveformColor: [number, number, number] = [0.43, 0.3, 0.7]; // Less saturated Purple HSL(255, 40%, 50%)
    // const deckBWaveformColor: [number, number, number] = [0.7125, 0.75, 0.7875]; // HSL(210, 15%, 75%)

    // Deck A Band Colors (Purple-based)
    const deckALowBandColor: [number, number, number] = [0.3, 0.2, 0.6]; // Deeper Purple HSL(270, 50%, 40%)
    const deckAMidBandColor: [number, number, number] = [0.48, 0.38, 0.72]; // Main Purple HSL(255, 45%, 55%)
    const deckAHighBandColor: [number, number, number] = [0.55, 0.55, 0.85]; // Lighter Lavender HSL(240, 50%, 70%)

    // Deck B Band Colors (Gray-based)
    const deckBLowBandColor: [number, number, number] = [0.475, 0.5, 0.525]; // Darker Gray HSL(210, 10%, 50%)
    const deckBMidBandColor: [number, number, number] = [0.66, 0.7, 0.74]; // Main Gray HSL(210, 12%, 70%)
    const deckBHighBandColor: [number, number, number] = [0.88, 0.9, 0.92]; // Light Gray/Off-white HSL(210, 20%, 90%)

    // --- Effects to apply derived volumes to player stores ---
    $effect(() => {
        const volumeA = deckAVolume();
        playerStoreA.setVolume(volumeA);
    });

    $effect(() => {
        const volumeB = deckBVolume();
        playerStoreB.setVolume(volumeB);
    });

    // --- Deck A Data Derivations ---
    const trackInfoA = $derived(
        $libraryStore.audioFiles.find((track) => track.path === deckAFilePath),
    );

    const isTrackLoadedA = $derived(!!deckAFilePath);

    // --- Deck B Data Derivations ---
    const trackInfoB = $derived(
        $libraryStore.audioFiles.find((track) => track.path === deckBFilePath),
    );

    const isTrackLoadedB = $derived(!!deckBFilePath);

    async function loadTrackToDeck(deckId: "A" | "B") {
        if (!selectedTrack) return;

        const currentFilePath = deckId === "A" ? deckAFilePath : deckBFilePath;
        if (currentFilePath === selectedTrack.path) {
            console.log(
                `[Page] Track ${selectedTrack.path} is already loaded on Deck ${deckId}. Skipping reload.`,
            );
            return;
        }

        const trackToLoad = selectedTrack; // Capture selectedTrack

        if (deckId === "A") {
            deckAFilePath = trackToLoad.path;
        } else {
            deckBFilePath = trackToLoad.path;
        }

        if (trackToLoad.path) {
            if (deckId === "A") {
                isDeckAWaveformLoading = true;
                deckAVolumeAnalysis = null;
            } else {
                isDeckBWaveformLoading = true;
                deckBVolumeAnalysis = null;
            }

            try {
                console.log(
                    `[Page] Loading volume analysis for Deck ${deckId}: ${trackToLoad.path}`,
                );
                const result = await invoke<VolumeAnalysis>(
                    "get_track_volume_analysis",
                    { path: trackToLoad.path },
                );
                if (deckId === "A") {
                    deckAVolumeAnalysis = result;
                } else {
                    deckBVolumeAnalysis = result;
                }
                console.log(
                    `[Page] Volume analysis loaded for Deck ${deckId}`,
                    result,
                );
            } catch (error) {
                console.error(
                    `[Page] Error loading volume analysis for Deck ${deckId}: ${trackToLoad.path}`,
                    error,
                );
                if (deckId === "A") {
                    deckAVolumeAnalysis = null;
                } else {
                    deckBVolumeAnalysis = null;
                }
            } finally {
                if (deckId === "A") {
                    isDeckAWaveformLoading = false;
                } else {
                    isDeckBWaveformLoading = false;
                }
            }
        }
    }

    // Seek handlers for VolumeAnalysis components
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
    {/if}

    {#if isFolderSelected}
        <section class="mixer-section">
            <h2>Mixer</h2>
            <div class="waveform-container deck-a-style">
                <WaveFormDisplay
                    volumeAnalysis={deckAVolumeAnalysis}
                    isAnalysisPending={isDeckAWaveformLoading}
                    isTrackLoaded={isTrackLoadedA}
                    audioDuration={$playerStoreA.duration}
                    currentTime={$playerStoreA.currentTime}
                    isPlaying={$playerStoreA.isPlaying}
                    seekAudio={seekDeckA}
                    cuePointTime={$playerStoreA.cuePointTime}
                    lowBandColor={deckALowBandColor}
                    midBandColor={deckAMidBandColor}
                    highBandColor={deckAHighBandColor}
                    eqParams={deckAEqParams}
                    faderLevel={deckAFaderLevel}
                />
            </div>
            <div class="waveform-container deck-b-style">
                <WaveFormDisplay
                    volumeAnalysis={deckBVolumeAnalysis}
                    isAnalysisPending={isDeckBWaveformLoading}
                    isTrackLoaded={isTrackLoadedB}
                    audioDuration={$playerStoreB.duration}
                    currentTime={$playerStoreB.currentTime}
                    isPlaying={$playerStoreB.isPlaying}
                    seekAudio={seekDeckB}
                    cuePointTime={$playerStoreB.cuePointTime}
                    lowBandColor={deckBLowBandColor}
                    midBandColor={deckBMidBandColor}
                    highBandColor={deckBHighBandColor}
                    eqParams={deckBEqParams}
                    faderLevel={deckBFaderLevel}
                />
            </div>
            <div class="crossfader-container">
                <Slider
                    id="crossfader"
                    label="Crossfader"
                    orientation="horizontal"
                    outputMin={0}
                    outputMax={1}
                    centerValue={0.5}
                    step={0.01}
                    bind:value={crossfaderValue}
                    debounceMs={20}
                />
            </div>
        </section>

        <section class="decks-section-horizontal">
            <div class="deck-stacked deck-a-style">
                <DeckControls
                    deckId="A"
                    filePath={deckAFilePath}
                    playerStoreState={$playerStoreA}
                    playerActions={playerStoreA}
                    bind:eqParams={deckAEqParams}
                    bind:faderLevel={deckAFaderLevel}
                />
            </div>
            <div class="deck-stacked deck-b-style">
                <DeckControls
                    deckId="B"
                    filePath={deckBFilePath}
                    playerStoreState={$playerStoreB}
                    playerActions={playerStoreB}
                    bind:eqParams={deckBEqParams}
                    bind:faderLevel={deckBFaderLevel}
                />
            </div>
        </section>

        <!-- Music Library below -->
        <section class="library-section library-section-expanded">
            <h2>Music Library</h2>
            <div class="load-controls">
                <button
                    class="load-deck-a-button"
                    onclick={() => loadTrackToDeck("A")}
                    disabled={!selectedTrack}>Load Selected to Deck A</button
                >
                <button
                    class="load-deck-b-button"
                    onclick={() => loadTrackToDeck("B")}
                    disabled={!selectedTrack}>Load Selected to Deck B</button
                >
            </div>
            <MusicLibrary />
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
        max-width: 1200px;
    }

    .mixer-section {
        width: 100%;
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        padding: 1rem;
        border: 1px solid var(--section-border);
        border-radius: 8px;
        background-color: var(--section-bg);
    }
    .mixer-section h2 {
        margin-top: 0;
        margin-bottom: 1rem;
        padding-bottom: 0.5rem;
        text-align: center;
        border-bottom: 1px solid var(--section-border-light);
    }
    .waveform-container {
        width: 100%;
        height: 80px;
        border-radius: 4px;
        overflow: hidden;
        background-color: var(--waveform-area-bg); /* Default */
        transition: background-color 0.3s ease;
    }
    .waveform-container.deck-a-style {
        background-color: var(--deck-a-waveform-bg-light);
    }
    .waveform-container.deck-b-style {
        background-color: var(--deck-b-waveform-bg-light);
    }

    :global(.mixer-waveform .waveform-scroll-container) {
        border-radius: 4px;
    }

    .decks-section-horizontal {
        display: flex;
        flex-direction: row;
        justify-content: space-between;
        align-items: flex-start;
        gap: 1.5rem;
        width: 100%;
    }

    .deck-stacked {
        display: flex;
        flex-direction: column;
        align-items: center;
        background-color: var(
            --deck-bg
        ); /* Default, can be overridden by deck-a/b-style */
        flex: 1;
        min-width: 0;
        border: 3px solid transparent;
        border-radius: 8px;
        padding: 0.25rem;
        transition:
            border-color 0.3s ease,
            background-color 0.3s ease;
    }
    .deck-stacked.deck-a-style {
        border-color: var(--deck-a-border-light);
        background-color: var(--deck-a-deck-bg-light);
    }
    .deck-stacked.deck-b-style {
        border-color: var(--deck-b-border-light);
        background-color: var(--deck-b-deck-bg-light);
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
        max-height: 40vh;
        overflow-y: auto;
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
        /* Dark theme Deck-specific button styles - Increased specificity */
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

        /* Styles for specific components in dark mode using the variables */
        .waveform-container.deck-a-style {
            background-color: var(--deck-a-waveform-bg-dark);
        }
        .deck-stacked.deck-a-style {
            border-color: var(--deck-a-border-dark);
            background-color: var(--deck-a-deck-bg-dark);
        }
        .waveform-container.deck-b-style {
            background-color: var(--deck-b-waveform-bg-dark);
        }
        .deck-stacked.deck-b-style {
            border-color: var(--deck-b-border-dark);
            background-color: var(--deck-b-deck-bg-dark);
        }
    }

    /* Global base styles - should ideally be in a global CSS file or app.html */
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

        /* General text and page background colors here, if not set by SvelteKit defaults */
        --text-color: #0f0f0f;
        --bg-color: #f6f6f6;
    }
    @media (prefers-color-scheme: dark) {
        :root {
            --text-color: #f6f6f6;
            --bg-color: #2f2f2f;
        }
    }

    .crossfader-container {
        width: 100%;
        max-width: 50%;
        margin-left: auto;
        margin-right: auto;
        padding: 0.5rem 1rem;
        margin-top: 0.5rem;
    }
</style>
