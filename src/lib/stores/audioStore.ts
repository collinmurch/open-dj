import { derived, writable } from "svelte/store";

// Create writable stores for our state
export const audioUrl = writable<string | null>(null);
export const audioStore = writable<HTMLAudioElement | null>(null);
export const isPlaying = writable(false);
export const selectedFile = writable<File | null>(null);
export const audioProgress = writable(0);
export const audioDuration = writable(0);
export const audioError = writable<string | null>(null);
export const audioBlob = writable<Blob | null>(null);

// Derived stores for formatted values
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

export function selectMp3File() {
	const input = document.createElement("input");
	input.type = "file";
	input.accept = "audio/mp3";

	input.onchange = async (e) => {
		const file = (e.target as HTMLInputElement).files?.[0];
		if (!file) return;

		try {
			selectedFile.set(file);
			const blob = new Blob([file], { type: "audio/mp3" });
			audioBlob.set(blob);
			const url = URL.createObjectURL(blob);
			audioUrl.set(url);
			audioError.set(null);
		} catch (error) {
			console.error("Error reading file:", error);
			audioError.set("Error reading file");
		}
	};

	input.click();
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
		audioDuration.set(audioElement.duration);
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
		audioProgress.set(audioElement.currentTime);
		return audioElement;
	});
}
