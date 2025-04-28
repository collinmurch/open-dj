import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { invoke } from '@tauri-apps/api/core';
import { derived, writable, get } from "svelte/store";

export const audioUrl = writable<string | null>(null);
export const audioStore = writable<HTMLAudioElement | null>(null);
export const isPlaying = writable(false);
export const selectedFile = writable<File | null>(null);
export const audioProgress = writable(0);
export const audioDuration = writable(0);
export const audioError = writable<string | null>(null);

// Type definition for the analysis results from Rust
export interface VolumeInterval {
	start_time: number;
	end_time: number;
	rms_amplitude: number;
}

// Store for the analysis results
export const analysisResults = writable<VolumeInterval[] | null>(null);

export const formattedProgress = derived(audioProgress, ($progress) => {
	const minutes = Math.floor($progress / 60);
	const seconds = Math.floor($progress % 60);
	return `${minutes}:${seconds.toString().padStart(2, "0")}`;
});

export const formattedDuration = derived(audioDuration, ($duration) => {
	const minutes = Math.floor($duration / 60);
	const seconds = Math.floor($duration % 60);
	return `${minutes}:${seconds.toString().padStart(2, "0")}`;
});

export async function selectMp3File() {
	analysisResults.set(null);
	audioError.set(null);
	audioProgress.set(0);
	audioDuration.set(0);
	audioUrl.set(null);
	selectedFile.set(null);

	try {
		const filePath = await open({
			multiple: false,
			filters: [
				{
					name: "Audio Files",
					extensions: ["mp3", "mpeg"],
				},
			],
		});

		if (filePath === null) {
			console.log("No file selected");
			return;
		}

		if (typeof filePath !== "string") {
			throw new Error("Expected a single file path string.");
		}

		console.log("Selected file path:", filePath);

		// Hit Rust backend to process audio file
		try {
			const results = await invoke<VolumeInterval[]>('process_audio_file', { path: filePath });
			console.log(`Received ${results.length} analysis intervals from Rust.`);
			analysisResults.set(results);
		} catch (invokeError) {
			console.error("Error invoking Rust command:", invokeError);
			const errorMessage = invokeError instanceof Error ? invokeError.message : String(invokeError);
			audioError.set(`Backend analysis failed: ${errorMessage}`);
			analysisResults.set(null);
		}

		// Proceed to load file for frontend playback regardless of backend analysis outcome
		const fileBytes = await readFile(filePath);
		const fileName = filePath.split(/[\/]/).pop() || "audio.mp3"; // More robust path splitting

		const fileObj = new File([fileBytes], fileName, { type: "audio/mpeg" });
		selectedFile.set(fileObj);

		// Create blob URL for the audio element
		const blob = new Blob([fileBytes], { type: "audio/mpeg" });
		const url = URL.createObjectURL(blob);
		audioUrl.set(url);
		console.log("File processed for frontend:", fileName);

	} catch (error) {
		console.error("Error during file selection or processing:", error);
		const errorMessage = error instanceof Error ? error.message : String(error);
		audioError.set(`Failed to load audio: ${errorMessage}`);
		analysisResults.set(null);
		audioUrl.set(null);
		selectedFile.set(null);
	}
}

export function togglePlay() {
	audioStore.update((audioElement) => {
		if (!audioElement) return null;

		if (audioElement.paused) {
			audioElement.play().catch((error) => {
				console.error("Error playing audio:", error);
				audioError.set("Error playing audio");
			});
		} else {
			audioElement.pause();
		}

		return audioElement;
	});
}

export function updateProgress() {
	audioStore.update((audioElement) => {
		if (!audioElement) return null;

		audioProgress.set(audioElement.currentTime);
		if (!Number.isNaN(audioElement.duration)) {
			audioDuration.set(audioElement.duration);
		}

		return audioElement;
	});
}

export function seekAudio(event: MouseEvent) {
	audioStore.update((audioElement) => {
		if (!audioElement) return null;

		const progressContainer = event.currentTarget as HTMLElement;
		const rect = progressContainer.getBoundingClientRect();
		const clickPosition = event.clientX - rect.left;
		const seekTime = (clickPosition / rect.width) * audioElement.duration;

		audioElement.currentTime = seekTime;

		return audioElement;
	});
}

export function seekBySeconds(seconds: number) {
	audioStore.update((audioElement) => {
		if (!audioElement) return null;

		const newTime = audioElement.currentTime + seconds;
		audioElement.currentTime = Math.max(
			0,
			Math.min(newTime, audioElement.duration),
		);

		return audioElement;
	});
}

export function onAudioLoaded() {
	audioStore.update((audioElement) => {
		if (!audioElement) return null;

		if (!Number.isNaN(audioElement.duration)) {
			audioDuration.set(audioElement.duration);
		}
		audioProgress.set(audioElement.currentTime);

		return audioElement;
	});
}
