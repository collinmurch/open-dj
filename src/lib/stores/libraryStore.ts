import { writable } from 'svelte/store';
import type { TrackInfo } from '$lib/types';
import { open } from '@tauri-apps/plugin-dialog';
import { readDir } from '@tauri-apps/plugin-fs';
import { join } from '@tauri-apps/api/path';

interface LibraryState {
    selectedFolder: string | null;
    audioFiles: TrackInfo[];
    selectedTrack: TrackInfo | null;
    isLoading: boolean;
    error: string | null;
}

function createLibraryStore() {
    const { subscribe, set, update } = writable<LibraryState>({
        selectedFolder: null,
        audioFiles: [],
        selectedTrack: null,
        isLoading: false,
        error: null,
    });

    async function selectLibraryFolder() {
        update(state => ({
            ...state,
            isLoading: true,
            error: null,
            audioFiles: [],
            selectedTrack: null,
            selectedFolder: null
        }));

        let folderPath: string | null = null;

        try {
            const selectedPath = await open({
                directory: true,
                multiple: false,
                title: 'Select Music Folder',
            });

            if (typeof selectedPath === 'string') {
                folderPath = selectedPath;
                update(state => ({ ...state, selectedFolder: folderPath }));
                console.log(`[LibraryStore] Selected folder: ${folderPath}`);

                const entries = await readDir(folderPath);
                let files: TrackInfo[] = [];

                for (const entry of entries) {
                    if (entry.isFile && entry.name && entry.name.toLowerCase().match(/\.(mp3|mpeg)$/)) {
                        const fullPath = await join(folderPath, entry.name);
                        files.push({ path: fullPath, name: entry.name });
                    }
                }

                console.log(`[LibraryStore] Found ${files.length} audio files.`);
                update(state => ({
                    ...state,
                    audioFiles: files,
                    isLoading: false,
                }));

            } else {
                console.log('[LibraryStore] Folder selection cancelled.');
                update(state => ({ ...state, isLoading: false }));
            }
        } catch (err) {
            console.error('[LibraryStore] Error selecting or reading folder:', err);
            const message = err instanceof Error ? err.message : String(err);
            update(state => ({ ...state, isLoading: false, error: `Failed to load library: ${message}` }));
        }
    }

    function setSelectedTrack(track: TrackInfo | null) {
        update(state => ({ ...state, selectedTrack: track }));
    }

    return {
        subscribe,
        selectLibraryFolder,
        setSelectedTrack,
    };
}

export const libraryStore = createLibraryStore(); 