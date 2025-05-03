import { writable } from 'svelte/store';
import type { TrackInfo, AudioAnalysis } from '$lib/types';
import { open } from '@tauri-apps/plugin-dialog';
import { readDir } from '@tauri-apps/plugin-fs';
import { join } from '@tauri-apps/api/path';
import { invoke } from '@tauri-apps/api/core';

// Structure expected from the Rust BPM command
interface BpmAnalysisResult {
    bpm: number;
}

// Structure expected from Rust Result<T, E> serialization
interface RustResult<T, E> {
    Ok?: T;
    Err?: E;
}

// Structure expected from the Rust *batch* volume command (Map: Path -> Result)
type VolumeAnalysisBatchResult = Record<string, RustResult<AudioAnalysis, string>>;

interface LibraryState {
    selectedFolder: string | null;
    audioFiles: TrackInfo[];
    selectedTrack: TrackInfo | null;
    isLoading: boolean; // Loading folder contents
    isAnalyzing: boolean; // Background analysis (BPM, volume) is running
    error: string | null;
    volumeAnalysisResults: Map<string, AudioAnalysis | null | undefined>; // undefined: pending, null: error, AudioAnalysis: success
}

function createLibraryStore() {
    const { subscribe, set, update } = writable<LibraryState>({
        selectedFolder: null,
        audioFiles: [],
        selectedTrack: null,
        isLoading: false,
        isAnalyzing: false,
        error: null,
        volumeAnalysisResults: new Map(),
    });

    async function selectLibraryFolder() {
        // Reset state before starting
        update(state => ({
            ...state,
            isLoading: true,
            isAnalyzing: false,
            error: null,
            audioFiles: [],
            selectedTrack: null,
            selectedFolder: null,
            volumeAnalysisResults: new Map(),
        }));

        let folderPath: string | null = null;

        try {
            const selectedPath = await open({
                directory: true,
                multiple: false,
                title: 'Select Music Folder',
            });

            if (typeof selectedPath !== 'string') {
                // User cancelled selection
                update(state => ({ ...state, isLoading: false }));
                return;
            }

            folderPath = selectedPath;
            update(state => ({ ...state, selectedFolder: folderPath }));
            console.log(`[LibraryStore] Selected folder: ${folderPath}`);

            // Read directory contents
            const entries = await readDir(folderPath);
            const initialFiles: TrackInfo[] = [];
            const filePaths: string[] = [];

            for (const entry of entries) {
                if (entry.isFile && entry.name?.toLowerCase().match(/\.(mp3|mpeg|wav|flac|ogg)$/)) {
                    const fullPath = await join(folderPath, entry.name);
                    initialFiles.push({ path: fullPath, name: entry.name, bpm: undefined }); // Mark BPM as pending
                    filePaths.push(fullPath);
                    update(s => {
                        s.volumeAnalysisResults.set(fullPath, undefined); // Mark volume as pending
                        return s;
                    });
                }
            }

            console.log(`[LibraryStore] Found ${initialFiles.length} audio files.`);
            update(state => ({
                ...state,
                audioFiles: initialFiles,
                isLoading: false, // Folder loading done
                isAnalyzing: true, // Analysis starts now
            }));

            // --- Trigger background analyses (BPM & Volume) --- 
            // This runs without blocking the UI updates above
            const runBackgroundAnalysis = async () => {
                const bpmPromises = initialFiles.map(file =>
                    invoke<BpmAnalysisResult>('analyze_bpm_for_file', { path: file.path })
                        .then(result => {
                            // console.log(`[LibraryStore] BPM for ${file.name}: ${result.bpm}`);
                            update(state => {
                                const updatedFiles = state.audioFiles.map(f =>
                                    f.path === file.path ? { ...f, bpm: result.bpm } : f
                                );
                                return { ...state, audioFiles: updatedFiles };
                            });
                        })
                        .catch(err => {
                            console.error(`[LibraryStore] BPM analysis failed for ${file.name}:`, err);
                            update(state => {
                                const updatedFiles = state.audioFiles.map(f =>
                                    f.path === file.path ? { ...f, bpm: null } : f // null indicates error
                                );
                                return { ...state, audioFiles: updatedFiles };
                            });
                        })
                );

                const volumePromise = invoke<VolumeAnalysisBatchResult>('analyze_volume_batch', { paths: filePaths })
                    .then(results => {
                        // console.log("[LibraryStore] Batch volume analysis complete.");
                        update(state => {
                            for (const [path, rustResult] of Object.entries(results)) {
                                if (rustResult?.Err !== undefined) {
                                    console.error(`[LibraryStore] Volume analysis error for ${path}:`, rustResult.Err);
                                    state.volumeAnalysisResults.set(path, null); // null indicates error
                                } else if (rustResult?.Ok !== undefined) {
                                    state.volumeAnalysisResults.set(path, rustResult.Ok);
                                } else {
                                    // Should not happen with correct Rust Result<> serialization
                                    console.warn(`[LibraryStore] Unexpected result structure for volume analysis of ${path}:`, rustResult);
                                    state.volumeAnalysisResults.set(path, null);
                                }
                            }
                            return state; // Must return the modified state map
                        });
                    })
                    .catch(batchError => {
                        console.error("[LibraryStore] Batch volume analysis failed:", batchError);
                        // Mark all files that are still 'undefined' (pending) as errored (null)
                        update(state => {
                            filePaths.forEach(path => {
                                if (state.volumeAnalysisResults.get(path) === undefined) {
                                    state.volumeAnalysisResults.set(path, null);
                                }
                            });
                            // Optionally set a general error message, though per-file errors are logged
                            // state.error = `Volume analysis failed: ${batchError instanceof Error ? batchError.message : String(batchError)}`;
                            return { ...state }; // Ensure state update occurs
                        });
                    });

                // Wait for all analyses to finish (or fail)
                try {
                    await Promise.allSettled([...bpmPromises, volumePromise]);
                } finally {
                    console.log("[LibraryStore] All background analysis finished.");
                    update(state => ({ ...state, isAnalyzing: false })); // Mark analysis as complete
                }
            };

            runBackgroundAnalysis(); // Start the background tasks

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