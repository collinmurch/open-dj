<script lang="ts">
    import {
        audioDuration,
        audioProgress,
        audioStore,
        audioUrl,
        formattedDuration,
        formattedProgress,
        isPlaying,
        seekAudio,
        seekBySeconds,
        togglePlay,
        updateProgress,
        onAudioLoaded,
    } from "$lib/stores/audioStore";

    // Local reference to the audio element
    let audioElement: HTMLAudioElement;

    // Debug logging
    $effect(() => {
        console.log("AudioPlayer - audioUrl:", $audioUrl);
        console.log("AudioPlayer - audioDuration:", $audioDuration);
    });

    // Update the store when the audio element changes
    $effect(() => {
        if (audioElement && $audioUrl) {
            console.log("Setting audio source to:", $audioUrl);

            // Important: Load the new audio source
            audioElement.src = $audioUrl;
            audioElement.load();
            $audioStore = audioElement;

            // Set up event listeners
            audioElement.addEventListener("loadedmetadata", () => {
                // This is crucial for updating duration
                console.log(
                    "Audio metadata loaded, duration:",
                    audioElement.duration,
                );
                onAudioLoaded();
                updateProgress();
            });

            audioElement.addEventListener("timeupdate", () => {
                updateProgress();
            });

            audioElement.addEventListener("play", () => {
                $isPlaying = true;
            });

            audioElement.addEventListener("pause", () => {
                $isPlaying = false;
            });

            audioElement.addEventListener("error", (e) => {
                console.error("Audio error:", e);
            });
        }
    });

    function handleSeekBackward() {
        seekBySeconds(-10);
    }

    function handleSeekForward() {
        seekBySeconds(10);
    }

    function handleProgressClick(event: MouseEvent) {
        seekAudio(event);
    }

    function handleProgressKeyDown(event: KeyboardEvent) {
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            const target = event.currentTarget as HTMLElement;
            const rect = target.getBoundingClientRect();
            const fakeEvent = new MouseEvent("click", {
                clientX: rect.left + rect.width / 2,
                bubbles: true,
            });
            seekAudio(fakeEvent);
        }
    }
</script>

<div class="audio-player">
    <audio bind:this={audioElement} preload="auto">
        <track kind="captions" />
    </audio>

    {#if $audioUrl}
        <div class="controls">
            <div class="audio-controls">
                <button
                    class="control-button"
                    onclick={handleSeekBackward}
                    aria-label="Rewind 10 seconds"
                >
                    <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="24"
                        height="24"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <polygon points="19 20 9 12 19 4 19 20"></polygon>
                        <line x1="5" y1="19" x2="5" y2="5"></line>
                    </svg>
                </button>
                <button
                    class="control-button"
                    onclick={togglePlay}
                    aria-label={$isPlaying ? "Pause" : "Play"}
                >
                    {#if $isPlaying}
                        <svg
                            xmlns="http://www.w3.org/2000/svg"
                            width="24"
                            height="24"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <rect x="6" y="4" width="4" height="16"></rect>
                            <rect x="14" y="4" width="4" height="16"></rect>
                        </svg>
                    {:else}
                        <svg
                            xmlns="http://www.w3.org/2000/svg"
                            width="24"
                            height="24"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <polygon points="5 3 19 12 5 21 5 3"></polygon>
                        </svg>
                    {/if}
                </button>
                <button
                    class="control-button"
                    onclick={handleSeekForward}
                    aria-label="Forward 10 seconds"
                >
                    <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="24"
                        height="24"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <polygon points="5 4 15 12 5 20 5 4"></polygon>
                        <line x1="19" y1="5" x2="19" y2="19"></line>
                    </svg>
                </button>
            </div>

            <div class="time-display">
                <span>{$formattedProgress}</span>
                <div
                    class="progress-bar"
                    onclick={handleProgressClick}
                    onkeydown={handleProgressKeyDown}
                    role="slider"
                    tabindex="0"
                    aria-label="Audio progress"
                    aria-valuemin="0"
                    aria-valuemax="100"
                    aria-valuenow={($audioProgress / ($audioDuration || 1)) *
                        100}
                >
                    <div
                        class="progress"
                        style="width: {($audioProgress /
                            ($audioDuration || 1)) *
                            100}%"
                    ></div>
                </div>
                <span>{$formattedDuration}</span>
            </div>
        </div>
    {:else}
        <p class="no-audio">No audio file selected</p>
    {/if}
</div>

<style>
    .audio-player {
        width: 100%;
        max-width: 600px;
        margin: 0 auto;
        padding: 1rem;
        background: var(--background-color);
        border-radius: 8px;
        box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
    }

    .controls {
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    .audio-controls {
        display: flex;
        justify-content: center;
        gap: 1rem;
    }

    .control-button {
        background: none;
        border: none;
        cursor: pointer;
        padding: 0.5rem;
        color: var(--text-color);
        transition: color 0.2s;
    }

    .control-button:hover {
        color: var(--accent-color);
    }

    .time-display {
        display: flex;
        align-items: center;
        gap: 1rem;
    }

    .progress-bar {
        flex-grow: 1;
        height: 4px;
        background-color: var(--progress-bg);
        cursor: pointer;
        position: relative;
        border-radius: 2px;
        outline: none;
    }

    .progress-bar:focus {
        box-shadow: 0 0 0 2px var(--accent-color);
    }

    .progress {
        height: 100%;
        background-color: var(--accent-color);
        border-radius: 2px;
        transition: width 0.1s linear;
    }

    .no-audio {
        text-align: center;
        color: var(--text-color);
        opacity: 0.7;
    }

    @media (prefers-color-scheme: dark) {
        .audio-player {
            --background-color: #2a2a2a;
            --progress-bg: rgba(255, 255, 255, 0.1);
            --accent-color: #4a9eff;
            --text-color: #fff;
        }
    }

    @media (prefers-color-scheme: light) {
        .audio-player {
            --background-color: #fff;
            --progress-bg: rgba(0, 0, 0, 0.1);
            --accent-color: #0066cc;
            --text-color: #000;
        }
    }
</style>
