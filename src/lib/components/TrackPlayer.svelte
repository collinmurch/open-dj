<script lang="ts">
    import { libraryStore } from "$lib/stores/libraryStore";
    import {
        createPlayerStore,
        type PlayerStore,
    } from "$lib/stores/playerStore";
    import { readFile } from "@tauri-apps/plugin-fs";
    import AudioPlayer from "./AudioPlayer.svelte";
    import VolumeAnalysis from "./VolumeAnalysis.svelte";

    // --- Props ---
    let { filePath = null }: { filePath: string | null } = $props();

    // --- Component References ---
    let audioPlayer: AudioPlayer; // Still needed for seek callback

    // --- Create Player Store Instance ---
    // IMPORTANT: We need this store to persist across filePath changes if the component instance itself doesn't change.
    // Svelte 5 doesn't have explicit onMount/onDestroy in <script> like Svelte 4.
    // We can use $effect.root for a one-time setup or simply declare it here.
    // Declaring it here means a new store is created *if the TrackPlayer instance is recreated*.
    // If TrackPlayer *persists* and only filePath changes, this same store instance is reused.
    const playerStore: PlayerStore = createPlayerStore();

    // --- Internal State ---
    let audioUrl = $state<string | null>(null);

    // --- Derived analysis result based on filePath ---
    // Find the track info corresponding to the current filePath
    const trackInfo = $derived(
        $libraryStore.audioFiles.find((track) => track.path === filePath),
    );
    // Get the analysis features from the track info
    const analysisFeatures = $derived(trackInfo?.features);
    // Extract volume analysis specifically for the VolumeAnalysis component
    // Handle states: undefined (pending), null (error), or actual VolumeAnalysis object
    const volumeAnalysisResult = $derived(
        analysisFeatures === undefined
            ? undefined
            : (analysisFeatures?.volume ?? null),
    );

    // --- Effects ---

    // Effect to load audio data when filePath prop changes
    $effect(() => {
        const currentFilePath = filePath;
        let createdAudioUrl: string | null = null;

        const cleanup = () => {
            if (createdAudioUrl) URL.revokeObjectURL(createdAudioUrl);
        };

        if (currentFilePath) {
            playerStore.setIsLoading(true); // Update store state
            playerStore.setError(null);
            audioUrl = null;

            const loadFile = async () => {
                try {
                    const fileBytes = await readFile(currentFilePath);
                    const blob = new Blob([fileBytes], { type: "audio/mpeg" });
                    createdAudioUrl = URL.createObjectURL(blob);

                    if (filePath === currentFilePath) {
                        audioUrl = createdAudioUrl;
                        // isLoading is set to false inside AudioPlayer's onLoadedMetadata via the store
                    } else {
                        cleanup();
                    }
                } catch (err) {
                    if (filePath === currentFilePath) {
                        console.error(`[TrackPlayer] File loading error:`, err);
                        const message = `Failed to load audio: ${err instanceof Error ? err.message : String(err)}`;
                        playerStore.setError(message); // Update store state
                        cleanup();
                        audioUrl = null;
                        playerStore.setIsLoading(false); // Update store state
                    }
                }
            };

            loadFile();
        } else {
            playerStore.reset();
            const previousUrl = audioUrl;
            audioUrl = null;
            if (previousUrl) URL.revokeObjectURL(previousUrl);
        }

        return cleanup;
    });

    // Callback passed TO VolumeAnalysis for seeking the AudioPlayer
    function seekAudioCallback(time: number) {
        if (audioPlayer) {
            audioPlayer.seekAudio(time); // Still call method on instance
        }
    }
</script>

<div class="track-player-wrapper">
    {#if $playerStore.error && !$playerStore.isLoading}
        <p class="error-message">Error: {$playerStore.error}</p>
    {/if}

    <AudioPlayer bind:this={audioPlayer} store={playerStore} {audioUrl} />

    <VolumeAnalysis
        results={volumeAnalysisResult?.intervals ?? null}
        maxRms={volumeAnalysisResult?.max_rms_amplitude ?? 0}
        isAnalysisPending={analysisFeatures === undefined}
        audioDuration={$playerStore.duration}
        currentTime={$playerStore.currentTime}
        seekAudio={seekAudioCallback}
    />
</div>

<style>
    .error-message {
        text-align: center;
        padding: 1rem;
        font-style: italic;
        color: var(--error-text, #d9534f);
        background-color: var(--error-bg, #fdd);
        border: 1px solid var(--error-border, #fbb);
        border-radius: 4px;
    }

    .track-player-wrapper {
        display: flex;
        flex-direction: column;
        gap: 1rem;
        border: 1px solid #ccc;
        padding: 1rem;
        border-radius: 8px;
        background-color: var(--track-bg, #f9f9f9);
        width: 100%;
        max-width: 600px;
        margin-bottom: 1rem;
        position: relative;
        min-height: 200px;
    }

    @media (prefers-color-scheme: dark) {
        .track-player-wrapper {
            border-color: #444;
            background-color: var(--track-bg, #3a3a3a);
        }
        .error-message {
            color: var(--error-text-dark, #f48481);
            background-color: var(--error-bg-dark, #5e3e3e);
            border: 1px solid var(--error-border-dark, #a75c5c);
        }
    }
</style>
