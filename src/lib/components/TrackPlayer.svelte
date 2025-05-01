<script lang="ts">
    import AudioPlayer from "./AudioPlayer.svelte";
    import VolumeAnalysis from "./VolumeAnalysis.svelte";
    import type { AudioAnalysis } from "$lib/types";
    import { readFile } from "@tauri-apps/plugin-fs";
    import { invoke } from "@tauri-apps/api/core";

    // --- Props ---
    // Changed from component ref to filePath prop
    let { filePath = null }: { filePath: string | null } = $props();

    // --- Component References ---
    // No longer need FileSelector ref
    let audioPlayer: AudioPlayer;

    // --- Internal State for the loaded track ---
    let audioUrl = $state<string | null>(null);
    let analysisResult = $state<AudioAnalysis | null>(null);
    let isLoading = $state(false);
    let error = $state<string | null>(null);

    // State mirrored from AudioPlayer (remains the same)
    let currentTime = $state(0);
    let duration = $state(0);

    // --- Effects ---

    // Effect to load audio data and analysis when filePath prop changes
    $effect(() => {
        const currentFilePath = filePath; // Capture prop value for async operations
        let currentAudioUrl: string | null = null; // To manage object URL cleanup

        // Cleanup function for the effect
        const cleanup = () => {
            if (currentAudioUrl) {
                console.log(
                    `[TrackPlayer Effect Cleanup] Revoking URL: ${currentAudioUrl}`,
                );
                URL.revokeObjectURL(currentAudioUrl);
            }
        };

        // Run loading logic only if filePath is not null
        if (currentFilePath) {
            isLoading = true;
            error = null;
            analysisResult = null;
            audioUrl = null; // Clear previous URL before loading new one

            console.log(
                `[TrackPlayer Effect] Loading file: ${currentFilePath}`,
            );

            const loadAndAnalyze = async () => {
                // Reset loading state *before* starting async analysis promise chain,
                // but *after* setting the initial isLoading = true.
                // Analysis sets isLoading = false in its finally block.
                let analysisStarted = false;
                try {
                    // 1. Read file bytes
                    const fileBytes = await readFile(currentFilePath);
                    const blob = new Blob([fileBytes], { type: "audio/mpeg" });
                    currentAudioUrl = URL.createObjectURL(blob); // Assign to temporary var

                    // Only update state if the filePath hasn't changed again
                    if (filePath === currentFilePath) {
                        audioUrl = currentAudioUrl;
                        console.log(
                            `[TrackPlayer Effect] Created blob URL: ${audioUrl}`,
                        );
                    } else {
                        // If filePath changed while loading, revoke immediately
                        cleanup();
                        return; // Abort further processing
                    }

                    // 2. Trigger analysis (don't block UI)
                    analysisStarted = true;
                    invoke<AudioAnalysis>("process_audio_file", {
                        path: currentFilePath,
                    })
                        .then((result) => {
                            // Only update state if the filePath hasn't changed again
                            if (filePath === currentFilePath) {
                                console.log(
                                    `[TrackPlayer Effect] Analysis complete. Max RMS: ${result.max_rms_amplitude}`,
                                );
                                analysisResult = result;
                            } else {
                                console.log(
                                    `[TrackPlayer Effect] Analysis result ignored, filePath changed.`,
                                );
                            }
                        })
                        .catch((invokeError) => {
                            // Only update state if the filePath hasn't changed again
                            if (filePath === currentFilePath) {
                                console.error(
                                    `[TrackPlayer Effect] Rust analysis error:`,
                                    invokeError,
                                );
                                error = `Backend analysis failed: ${invokeError instanceof Error ? invokeError.message : String(invokeError)}`;
                                analysisResult = null;
                            }
                        })
                        .finally(() => {
                            // Only update loading state if the filePath hasn't changed again
                            // (Analysis might finish after a new file started loading)
                            if (filePath === currentFilePath) {
                                isLoading = false; // Loading finishes after analysis completes or fails
                            }
                        });
                } catch (err) {
                    // Only update state if the filePath hasn't changed again
                    if (filePath === currentFilePath) {
                        console.error(
                            `[TrackPlayer Effect] File loading error:`,
                            err,
                        );
                        error = `Failed to load audio: ${err instanceof Error ? err.message : String(err)}`;
                        if (currentAudioUrl)
                            URL.revokeObjectURL(currentAudioUrl); // Clean up if URL was created before error
                        audioUrl = null;
                        analysisResult = null;
                        if (!analysisStarted) isLoading = false; // Ensure loading stops if file read failed before analysis
                    }
                }
            };

            loadAndAnalyze();
        } else {
            // If filePath is null, reset everything
            isLoading = false;
            error = null;
            analysisResult = null;
            // Revoke previous URL if it exists
            const previousUrl = audioUrl;
            if (previousUrl) {
                URL.revokeObjectURL(previousUrl);
            }
            audioUrl = null;
            console.log(
                "[TrackPlayer Effect] filePath is null, cleared state.",
            );
        }

        // Return the cleanup function to revoke the URL when the effect re-runs or component unmounts
        return cleanup;
    });

    // Effect to sync state FROM AudioPlayer TO this component's state (currentTime, duration)
    $effect(() => {
        if (audioPlayer) {
            currentTime = audioPlayer.currentTime;
            duration = audioPlayer.duration;
            // No need to link audio element to FileSelector anymore
        } else {
            currentTime = 0;
            duration = 0;
        }
    });

    // Callback for VolumeAnalysis (remains the same)
    function seekAudioCallback(event: MouseEvent) {
        if (audioPlayer) {
            audioPlayer.seekAudio(event);
        }
    }
</script>

<div class="track-player-wrapper">
    {#if error && !isLoading}
        <!-- Only show error if not currently loading -->
        <p class="error-message">Error: {error}</p>
    {/if}

    <!-- Pass the local reactive state `audioUrl` down -->
    <!-- AudioPlayer handles the case where audioUrl is null -->
    <AudioPlayer bind:this={audioPlayer} {audioUrl} {isLoading} />

    <!-- Pass parts of analysisResult and other state down -->
    <!-- VolumeAnalysis should handle null results gracefully -->
    <VolumeAnalysis
        results={analysisResult?.intervals ?? null}
        maxRms={analysisResult?.max_rms_amplitude ?? 0}
        audioDuration={duration}
        {currentTime}
        seekAudio={seekAudioCallback}
    />
</div>

<style>
    /* Add styles for loading/error messages */
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
        max-width: 600px; /* Consistent max-width */
        margin-bottom: 1rem; /* Space between multiple tracks */
        position: relative; /* For potential absolute positioning of overlays if needed */
        min-height: 200px; /* Give it some minimum height */
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
