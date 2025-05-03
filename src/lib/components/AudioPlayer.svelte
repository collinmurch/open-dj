<script lang="ts">
    import type { PlayerStore } from "$lib/stores/playerStore";

    // Accept the store instance and audioUrl as props in a single call
    let {
        store,
        audioUrl: urlProp = null,
    }: { store: PlayerStore; audioUrl?: string | null } = $props();

    // --- Component State ---
    let audioElement = $state<HTMLAudioElement | null>(null);
    // State is now managed by the passed 'store' prop
    // urlProp is used to trigger loading in the effect below

    // --- State for RAF Loop (internal) ---
    let rafId: number | null = $state(null);
    let lastKnownCurrentTime = $state(0);
    let lastTimeUpdateTimestamp = $state(0);

    // --- Utility Functions ---
    function formatTime(timeSeconds: number): string {
        const validTime =
            timeSeconds > 0 && isFinite(timeSeconds) ? timeSeconds : 0;
        const minutes = Math.floor(validTime / 60);
        const seconds = Math.floor(validTime % 60);
        return `${minutes}:${seconds.toString().padStart(2, "0")}`;
    }

    // --- Derived State (read directly from store) ---
    const progressPercent = $derived(
        $store.duration > 0 && isFinite($store.duration)
            ? ($store.currentTime / $store.duration) * 100
            : 0,
    );
    const canInteract = $derived(
        $store.duration > 0 && isFinite($store.duration),
    );

    // --- Effects ---
    // Effect to handle changes in the audioUrl prop
    $effect(() => {
        if (audioElement) {
            const currentSrc = audioElement.currentSrc;
            if (urlProp && urlProp !== currentSrc) {
                // New URL: Reset store, load
                stopAnimationLoop();
                store.reset();
                store.setIsLoading(true);
                lastKnownCurrentTime = 0;
                lastTimeUpdateTimestamp = performance.now();
                audioElement.src = urlProp;
                audioElement.load();
            } else if (!urlProp && audioElement.hasAttribute("src")) {
                // URL removed: Reset store, unload
                stopAnimationLoop();
                store.reset();
                lastKnownCurrentTime = 0;
                lastTimeUpdateTimestamp = performance.now();
                audioElement.removeAttribute("src");
                audioElement.load();
            }
        }
    });

    $effect(() => {
        return () => {
            stopAnimationLoop();
        };
    });

    // --- Animation Loop ---
    function animationLoop() {
        if (!$store.isPlaying || !audioElement || !$store.duration) {
            stopAnimationLoop();
            return;
        }

        const now = performance.now();
        const elapsedSinceUpdate = (now - lastTimeUpdateTimestamp) / 1000;
        const predictedTime = lastKnownCurrentTime + elapsedSinceUpdate;

        const newTime = Math.max(0, Math.min(predictedTime, $store.duration));
        store.setCurrentTime(newTime);

        rafId = requestAnimationFrame(animationLoop);
    }

    function stopAnimationLoop() {
        if (rafId !== null) {
            cancelAnimationFrame(rafId);
            rafId = null;
        }
    }

    // --- HTMLAudioElement Event Handlers ---
    function onLoadedMetadata() {
        if (!audioElement) return;
        const newDuration = audioElement.duration;
        const validDuration =
            newDuration && isFinite(newDuration) ? newDuration : 0;
        store.setDuration(validDuration);

        lastKnownCurrentTime = audioElement.currentTime;
        lastTimeUpdateTimestamp = performance.now();
        store.setCurrentTime(lastKnownCurrentTime);

        const playing = !audioElement.paused;
        store.setIsPlaying(playing);
        store.setIsLoading(false);

        if (playing) {
            animationLoop();
        }
    }

    function onTimeUpdate() {
        if (audioElement) {
            const newKnownTime = audioElement.currentTime;
            lastKnownCurrentTime = newKnownTime;
            lastTimeUpdateTimestamp = performance.now();
        }
    }

    function onPlay() {
        store.setIsPlaying(true);
        store.setError(null);
        lastTimeUpdateTimestamp = performance.now();
        lastKnownCurrentTime = audioElement?.currentTime ?? 0;
        store.setCurrentTime(lastKnownCurrentTime);
        animationLoop();
    }

    function onPause() {
        store.setIsPlaying(false);
        stopAnimationLoop();
        if (audioElement) store.setCurrentTime(audioElement.currentTime);
    }

    function onEnded() {
        store.setIsPlaying(false);
        stopAnimationLoop();
        if (audioElement) store.setCurrentTime($store.duration);
    }

    function onError() {
        if (!audioElement) return;
        const message = `Playback error: ${audioElement.error?.message || "Unknown error"}`;
        store.setError(message);
        store.setIsPlaying(false);
        store.setIsLoading(false);
        stopAnimationLoop();
    }

    // --- Control Event Handlers (Logic using store state) ---
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
        if (
            !canInteract ||
            !$store.duration ||
            !(event.currentTarget instanceof HTMLElement)
        )
            return;

        const progressBar = event.currentTarget;
        const rect = progressBar.getBoundingClientRect();
        const clickX = event.clientX - rect.left;
        const percentage = Math.max(0, Math.min(clickX / rect.width, 1));
        const seekTime = percentage * $store.duration;

        seekAudio(seekTime);
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

    // --- Public API Methods (Interact with audioElement and store) ---
    export function togglePlay() {
        if (!audioElement || !canInteract) return;
        if (audioElement.paused)
            audioElement.play().catch((err) => {
                const message = `Playback error: ${err.message}`;
                store.setError(message);
                store.setIsPlaying(false);
                stopAnimationLoop();
            });
        else audioElement.pause(); // This will trigger onPause handler which updates store
    }
    export function seekAudio(time: number) {
        if (!audioElement || !$store.duration) return;
        const clampedTime = Math.max(0, Math.min(time, $store.duration));
        audioElement.currentTime = clampedTime;

        // Update baseline time used by RAF loop
        lastKnownCurrentTime = clampedTime;
        lastTimeUpdateTimestamp = performance.now();

        // Force update the reactive store state immediately, regardless of play state
        store.setCurrentTime(clampedTime);
    }
    export function seekBySeconds(seconds: number) {
        if (!audioElement || !canInteract) return;
        // Use store's current time as basis for calculation
        const targetTime = $store.currentTime + seconds;
        seekAudio(targetTime);
    }
</script>

<!-- Template uses store values -->
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
                        ><polygon points="19 20 9 12 19 4 19 20"></polygon><line
                            x1="5"
                            y1="19"
                            x2="5"
                            y2="5"
                        ></line></svg
                    >
                </button>
                <button
                    class="control-button"
                    onclick={handleTogglePlay}
                    aria-label={$store.isPlaying ? "Pause" : "Play"}
                    disabled={!canInteract}
                >
                    {#if $store.isPlaying}
                        <!-- SVG Pause -->
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
                            ><rect x="6" y="4" width="4" height="16"
                            ></rect><rect x="14" y="4" width="4" height="16"
                            ></rect></svg
                        >
                    {:else}
                        <!-- SVG Play -->
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
                            ><polygon points="5 3 19 12 5 21 5 3"
                            ></polygon></svg
                        >
                    {/if}
                </button>
                <button
                    class="control-button"
                    onclick={handleSeekForward}
                    aria-label="Forward 10 seconds"
                    disabled={!canInteract}
                >
                    <!-- SVG Forward -->
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
                        ><polygon points="5 4 15 12 5 20 5 4"></polygon><line
                            x1="19"
                            y1="5"
                            x2="19"
                            y2="19"
                        ></line></svg
                    >
                </button>
            </div>

            <div class="time-display">
                <span>{formatTime($store.currentTime)}</span>
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
                <span>{formatTime($store.duration)}</span>
            </div>
            {#if $store.error}
                <p class="error-message">{$store.error}</p>
            {/if}
        </div>
    {:else if $store.isLoading}
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
