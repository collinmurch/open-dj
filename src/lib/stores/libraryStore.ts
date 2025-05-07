import { writable } from 'svelte/store';
import type { TrackInfo, LibraryState, FeaturesAnalysisBatchResult, AudioFeatures } from '$lib/types';
import { open } from '@tauri-apps/plugin-dialog';
import { readDir } from '@tauri-apps/plugin-fs';
import { join } from '@tauri-apps/api/path';
import { invoke } from '@tauri-apps/api/core';

function createLibraryStore() {
    const { subscribe, set, update } = writable<LibraryState>({
        selectedFolder: null,
        audioFiles: [],
        selectedTrack: null,
        isLoading: false,
        isAnalyzing: false,
        error: null,
    });

    async function selectLibraryFolder() {
        update(state => ({
            ...state,
            isLoading: true,
            isAnalyzing: false,
            error: null,
            audioFiles: [],
            selectedTrack: null,
            selectedFolder: null,
        }));

        let folderPath: string | null = null;

        try {
            const selectedPath = await open({
                directory: true,
                multiple: false,
                title: 'Select Music Folder',
            });

            if (typeof selectedPath !== 'string') {
                update(state => ({ ...state, isLoading: false }));
                return;
            }

            folderPath = selectedPath;
            update(state => ({ ...state, selectedFolder: folderPath }));
            console.log(`[LibraryStore] Selected folder: ${folderPath}`);

            const entries = await readDir(folderPath);
            const initialFiles: TrackInfo[] = [];
            const filePaths: string[] = [];

            for (const entry of entries) {
                if (entry.isFile && entry.name?.toLowerCase().match(/\.(mp3|mpeg|wav|flac|ogg)$/)) {
                    const fullPath = await join(folderPath, entry.name);
                    initialFiles.push({ path: fullPath, name: entry.name, features: undefined });
                    filePaths.push(fullPath);
                }
            }

            console.log(`[LibraryStore] Found ${initialFiles.length} audio files.`);
            update(state => ({
                ...state,
                audioFiles: initialFiles,
                isLoading: false,
                isAnalyzing: true,
            }));

            const runBackgroundAnalysis = async () => {
                try {
                    console.log("[LibraryStore] Invoking analyze_features_batch...");
                    const results = await invoke<FeaturesAnalysisBatchResult>('analyze_features_batch', { paths: filePaths });
                    console.log("[LibraryStore] Batch features analysis complete.");

                    update(state => {
                        const updatedFiles = state.audioFiles.map(file => {
                            const result = results[file.path];
                            let features: AudioFeatures | null | undefined = undefined;
                            let derivedBpm: number | null | undefined = undefined;
                            let derivedDurationSeconds: number | null | undefined = undefined;

                            if (result === undefined) {
                                console.warn(`[LibraryStore] No analysis result found for ${file.path}`);
                                features = null;
                                derivedBpm = null;
                                derivedDurationSeconds = null;
                            } else if (result?.Err) {
                                console.error(`[LibraryStore] Analysis error for ${file.path}:`, result.Err);
                                features = null;
                                derivedBpm = null;
                                derivedDurationSeconds = null;
                            } else if (result?.Ok) {
                                features = result.Ok;
                                derivedBpm = result.Ok.bpm;
                                derivedDurationSeconds = result.Ok.durationSeconds;
                                console.log(`[LibraryStore] Parsed features for ${file.path}: BPM=${derivedBpm}, Duration=${derivedDurationSeconds}`);
                            } else {
                                console.warn(`[LibraryStore] Unexpected result structure for ${file.path}:`, result);
                                features = null;
                                derivedBpm = null;
                                derivedDurationSeconds = null;
                            }

                            return { ...file, features: features, bpm: derivedBpm, durationSeconds: derivedDurationSeconds };
                        });

                        return { ...state, audioFiles: updatedFiles };
                    });

                } catch (batchError) {
                    console.error("[LibraryStore] Batch features analysis failed invoke:", batchError);
                    const message = batchError instanceof Error ? batchError.message : String(batchError);
                    update(state => {
                        const updatedFiles = state.audioFiles.map(file =>
                            file.features === undefined ? { ...file, features: null, bpm: null } : file
                        );
                        return { ...state, audioFiles: updatedFiles, error: `Analysis failed: ${message}` };
                    });
                } finally {
                    console.log("[LibraryStore] Background analysis process finished.");
                    update(state => ({ ...state, isAnalyzing: false }));
                }
            };

            runBackgroundAnalysis();

        } catch (err) {
            console.error('[LibraryStore] Error selecting or reading folder:', err);
            const message = err instanceof Error ? err.message : String(err);
            update(state => ({ ...state, isLoading: false, isAnalyzing: false, error: `Failed to load library: ${message}` }));
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