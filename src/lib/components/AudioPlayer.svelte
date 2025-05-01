<script lang="ts">
    let {
        audioUrl: urlProp,
        isLoading = false,
    }: { audioUrl: string | null; isLoading?: boolean } = $props();

    let audioElement = $state<HTMLAudioElement | null>(null);
    let isPlaying = $state(false);
    let currentTime = $state(0);
    let duration = $state(0);
    let error = $state<string | null>(null);

    function formatTime(timeSeconds: number): string {
        const validTime =
            timeSeconds > 0 && isFinite(timeSeconds) ? timeSeconds : 0;
        const minutes = Math.floor(validTime / 60);
        const seconds = Math.floor(validTime % 60);
        return `${minutes}:${seconds.toString().padStart(2, "0")}`;
    }

    const progressPercent = $derived(
        duration > 0 && isFinite(duration) ? (currentTime / duration) * 100 : 0,
    );
    const canInteract = $derived(duration > 0 && isFinite(duration));

    $effect(() => {
        if (audioElement) {
            const currentSrc = audioElement.currentSrc;
            if (urlProp && urlProp !== currentSrc) {
                currentTime = 0;
                duration = 0;
                isPlaying = false;
                error = null;
                audioElement.src = urlProp;
                audioElement.load();
            } else if (!urlProp && audioElement.hasAttribute("src")) {
                currentTime = 0;
                duration = 0;
                isPlaying = false;
                error = null;
                audioElement.removeAttribute("src");
                audioElement.load();
            }
        }
    });

    function onLoadedMetadata() {
        if (!audioElement) return;
        const newDuration = audioElement.duration;
        duration = newDuration && isFinite(newDuration) ? newDuration : 0;
        currentTime = audioElement.currentTime;
        isPlaying = !audioElement.paused;
    }
    function onTimeUpdate() {
        if (audioElement) currentTime = audioElement.currentTime;
    }
    function onPlay() {
        isPlaying = true;
        error = null;
    }
    function onPause() {
        isPlaying = false;
    }
    function onEnded() {
        isPlaying = false;
        if (audioElement) currentTime = duration;
    }
    function onError() {
        if (!audioElement) return;
        error = `Playback error: ${audioElement.error?.message || "Unknown error"}`;
        isPlaying = false;
    }

    function handleTogglePlay() {
        if (!canInteract) return;
        togglePlay();
    }
    function handleSeekBackward() {
        if (!canInteract) return;
        seekBySeconds(-10);
    }
    function handleSeekForward() {
        if (!canInteract) return;
        seekBySeconds(10);
    }
    function handleProgressClick(event: MouseEvent) {
        if (!canInteract) return;
        seekAudio(event);
    }
    function handleProgressKeyDown(event: KeyboardEvent) {
        if (!canInteract) return;
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            const target = event.currentTarget as HTMLElement;
            const rect = target.getBoundingClientRect();
            const fakeEvent = new MouseEvent("click", {
                clientX: rect.left + rect.width / 2,
                clientY: rect.top + rect.height / 2,
                bubbles: true,
            });
            target.dispatchEvent(fakeEvent);
        }
    }

    export function togglePlay() {
        if (!audioElement || !canInteract) return;
        if (audioElement.paused)
            audioElement.play().catch((err) => {
                error = `Playback error: ${err.message}`;
                isPlaying = false;
            });
        else audioElement.pause();
    }
    export function seekAudio(event: MouseEvent) {
        if (!audioElement || !canInteract) return;
        const target = event.currentTarget as HTMLElement;
        const rect = target.getBoundingClientRect();
        audioElement.currentTime = Math.max(
            0,
            Math.min(
                ((event.clientX - rect.left) / rect.width) * duration,
                duration,
            ),
        );
    }
    export function seekBySeconds(seconds: number) {
        if (!audioElement || !canInteract) return;
        audioElement.currentTime = Math.max(
            0,
            Math.min(audioElement.currentTime + seconds, duration),
        );
    }

    export { audioElement, currentTime, duration };
</script>

<div class="audio-player">
    <audio
        bind:this={audioElement}
        preload="metadata"
        onloadedmetadata={onLoadedMetadata}
        ontimeupdate={onTimeUpdate}
        onplay={onPlay}
        onpause={onPause}
        onended={onEnded}
        onerror={onError}
    >
        <track kind="captions" />
    </audio>

    {#if urlProp}
        <div class="controls">
            <div class="audio-controls">
                <button
                    class="control-button"
                    onclick={handleSeekBackward}
                    aria-label="Rewind 10 seconds"
                    disabled={!canInteract}
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
                    onclick={handleTogglePlay}
                    aria-label={isPlaying ? "Pause" : "Play"}
                    disabled={!canInteract}
                >
                    {#if isPlaying}
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
                    disabled={!canInteract}
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
                <span>{formatTime(currentTime)}</span>
                <div
                    class:progress-bar={true}
                    class:disabled={!canInteract}
                    onclick={handleProgressClick}
                    onkeydown={handleProgressKeyDown}
                    role="slider"
                    tabindex={canInteract ? 0 : -1}
                    aria-label="Audio progress"
                    aria-valuemin="0"
                    aria-valuemax="100"
                    aria-valuenow={progressPercent}
                    aria-disabled={!canInteract}
                >
                    <div
                        class="progress"
                        style:width={`${progressPercent}%`}
                    ></div>
                </div>
                <span>{formatTime(duration)}</span>
            </div>
            {#if error}
                <p class="error-message">{error}</p>
            {/if}
        </div>
    {:else if isLoading}
        <p class="placeholder-message loading">Loading track...</p>
    {:else}
        <p class="placeholder-message no-audio">No audio loaded</p>
    {/if}
</div>

<style>
    .audio-player {
        padding: 1rem;
        background: var(--background-color, #f9f9f9);
        border-radius: 8px;
        min-height: 100px;
        display: flex;
        justify-content: center;
        align-items: center;
    }

    .controls {
        width: 100%;
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
        color: var(--text-color, #333);
        transition:
            color 0.2s,
            opacity 0.2s;
    }
    .control-button:hover:not(:disabled) {
        color: var(--accent-color, #0066cc);
    }
    .control-button:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .control-button:focus {
        outline: none;
    }
    .control-button:focus-visible {
        outline: 2px solid var(--accent-color, #0066cc);
        outline-offset: 2px;
        border-radius: 4px;
    }

    .time-display {
        display: flex;
        align-items: center;
        gap: 1rem;
    }

    .progress-bar {
        flex-grow: 1;
        height: 4px;
        background-color: var(--progress-bg, rgba(0, 0, 0, 0.1));
        cursor: pointer;
        position: relative;
        border-radius: 2px;
        outline: none;
    }
    .progress-bar.disabled {
        cursor: not-allowed;
        opacity: 0.7;
    }

    .progress-bar:focus {
        outline: none;
    }
    .progress-bar:focus-visible:not(.disabled) {
        box-shadow: 0 0 0 2px var(--accent-color, #0066cc);
    }

    .progress {
        height: 100%;
        background-color: var(--accent-color, #0066cc);
        border-radius: 2px;
        transition: width 0.1s linear;
        pointer-events: none;
    }

    .placeholder-message {
        text-align: center;
        color: var(--text-color, #555);
        opacity: 0.7;
        padding: 1rem 0;
        font-style: italic;
    }
    .placeholder-message.loading {
        opacity: 1;
    }

    .error-message {
        color: #d9534f;
        font-size: 0.9em;
        text-align: center;
        margin-top: 0.5rem;
    }

    @media (prefers-color-scheme: dark) {
        .audio-player {
            --background-color: #2a2a2a;
            --progress-bg: rgba(255, 255, 255, 0.1);
            --accent-color: #4a9eff;
            --text-color: #eee;
        }
        .placeholder-message {
            color: var(--text-color, #aaa);
        }
        .error-message {
            color: #f48481;
        }
    }

    @media (prefers-color-scheme: light) {
        .audio-player {
            --background-color: #f9f9f9;
            --progress-bg: rgba(0, 0, 0, 0.1);
            --accent-color: #0066cc;
            --text-color: #333;
        }
        .placeholder-message {
            color: var(--text-color, #555);
        }
    }
</style>
