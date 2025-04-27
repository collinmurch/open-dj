<script lang="ts">
    import FileSelector from "$lib/components/FileSelector.svelte";
    import AudioPlayer from "$lib/components/AudioPlayer.svelte";
    import { audioUrl, selectedFile } from "$lib/stores/audioStore";

    // Debug store values
    $effect(() => {
        console.log("Main page - Audio URL:", $audioUrl);
        console.log("Main page - Selected file:", $selectedFile);
    });
</script>

<main class="container">
    <h1>Open DJ</h1>

    <div class="player-container">
        <FileSelector />
        <AudioPlayer />
    </div>
</main>

<style>
    :root {
        font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
        font-size: 16px;
        line-height: 24px;
        font-weight: 400;
        color: #0f0f0f;
        background-color: #f6f6f6;
        font-synthesis: none;
        text-rendering: optimizeLegibility;
        -webkit-font-smoothing: antialiased;
        -moz-osx-font-smoothing: grayscale;
        -webkit-text-size-adjust: 100%;
    }

    .container {
        margin: 0;
        padding-top: 10vh;
        display: flex;
        flex-direction: column;
        justify-content: center;
        text-align: center;
    }

    .player-container {
        margin-top: 2rem;
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 1rem;
        width: 80%;
        max-width: 500px;
        margin-left: auto;
        margin-right: auto;
    }

    @media (prefers-color-scheme: dark) {
        :root {
            color: #f6f6f6;
            background-color: #2f2f2f;
        }
    }
</style>
