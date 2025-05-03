import { writable } from 'svelte/store';

export interface PlayerState {
    currentTime: number;
    duration: number;
    isPlaying: boolean;
    isLoading: boolean; // Added to reflect player loading state
    error: string | null;
}

export function createPlayerStore() {
    const { subscribe, set, update } = writable<PlayerState>({
        currentTime: 0,
        duration: 0,
        isPlaying: false,
        isLoading: false,
        error: null,
    });

    return {
        subscribe,
        set, // Allow wholesale replacement if needed
        update,
        // Helper methods could be added here if complex logic emerges
        setCurrentTime: (time: number) => update(s => ({ ...s, currentTime: time })),
        setDuration: (duration: number) => update(s => ({ ...s, duration: duration })),
        setIsPlaying: (playing: boolean) => update(s => ({ ...s, isPlaying: playing })),
        setIsLoading: (loading: boolean) => update(s => ({ ...s, isLoading: loading })),
        setError: (error: string | null) => update(s => ({ ...s, error: error })),
        reset: () => set({
            currentTime: 0,
            duration: 0,
            isPlaying: false,
            isLoading: false,
            error: null,
        })
    };
}

// Define the type for the store instance returned by the factory
export type PlayerStore = ReturnType<typeof createPlayerStore>; 