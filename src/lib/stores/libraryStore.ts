import type { BasicMetadataBatchResult, LibraryState, TrackBasicMetadata, TrackInfo } from '$lib/types';
import { invoke } from '@tauri-apps/api/core';
import { join } from '@tauri-apps/api/path';
import { open } from '@tauri-apps/plugin-dialog';
import { readDir } from '@tauri-apps/plugin-fs';
import { writable } from 'svelte/store';

function createLibraryStore() {
    const { subscribe, update } = writable<LibraryState>({
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
                    initialFiles.push({
                        path: fullPath,
                        name: entry.name,
                        metadata: undefined,
                        volumeAnalysisData: undefined
                    });
                    filePaths.push(fullPath);
                }
            }

            console.log(`[LibraryStore] Found ${initialFiles.length} audio files.`);
            if (initialFiles.length === 0) {
                console.warn("[LibraryStore] No compatible audio files found in the selected folder. Skipping analysis.");
                update(state => ({ ...state, isLoading: false, isAnalyzing: false }));
                return;
            }

            update(state => ({
                ...state,
                audioFiles: initialFiles,
                isLoading: false,
                isAnalyzing: true,
            }));

            const runBackgroundAnalysis = async () => {
                try {
                    // First, ensure cache directory exists
                    let cacheDir: string | null = null;
                    try {
                        cacheDir = await invoke<string>('ensure_cache_directory', { musicDir: folderPath });
                        console.log(`[LibraryStore] Cache directory: ${cacheDir}`);
                    } catch (cacheError) {
                        console.warn("[LibraryStore] Failed to create cache directory, proceeding without cache:", cacheError);
                    }

                    console.log("[LibraryStore] Invoking BPM analysis with cache...");
                    const results = await invoke<BasicMetadataBatchResult>(
                        'analyze_features_batch_with_cache',
                        {
                            paths: filePaths,
                            cacheDir: cacheDir
                        }
                    );
                    console.log("[LibraryStore] Batch BPM analysis finished.");

                    update(state => {
                        const updatedFiles = state.audioFiles.map((file) => {
                            const result = results[file.path];
                            let trackMetadata: TrackBasicMetadata | null | undefined = undefined;

                            if (result === undefined) {
                                console.warn(`[LibraryStore] No BPM analysis result found for ${file.path}`);
                                trackMetadata = null;
                            } else if (result?.Err) {
                                console.error(`[LibraryStore] BPM analysis error for ${file.path}:`, result.Err);
                                trackMetadata = null;
                            } else if (result?.Ok) {
                                trackMetadata = result.Ok;
                            } else {
                                console.warn(`[LibraryStore] Unexpected BPM result structure for ${file.path}:`, result);
                                trackMetadata = null;
                            }

                            return {
                                ...file,
                                metadata: trackMetadata,
                                volumeAnalysisData: undefined // Will be loaded on-demand when track is loaded to deck
                            };
                        });
                        return { ...state, audioFiles: updatedFiles };
                    });

                    // Log cache stats after analysis
                    if (cacheDir) {
                        try {
                            const [entryCount, sizeBytes] = await invoke<[number, number]>('get_cache_stats', { cacheDir });
                            console.log(`[LibraryStore] Cache stats: ${entryCount} entries, ${(sizeBytes / 1024 / 1024).toFixed(2)} MB`);
                        } catch (statsError) {
                            console.warn("[LibraryStore] Failed to get cache stats:", statsError);
                        }
                    }

                } catch (batchError) {
                    console.error("[LibraryStore] CRITICAL ERROR during BPM analysis:", batchError);
                    const message = batchError instanceof Error ? batchError.message : String(batchError);
                    update(state => {
                        const updatedFiles = state.audioFiles.map(file => ({
                            ...file,
                            metadata: file.metadata === undefined ? null : file.metadata,
                            volumeAnalysisData: undefined
                        }));
                        return { ...state, audioFiles: updatedFiles, error: `BPM analysis failed: ${message}` };
                    });
                } finally {
                    console.log("[LibraryStore] Background BPM analysis process finished.");
                    update(state => ({ ...state, isAnalyzing: false }));
                }
            };

            runBackgroundAnalysis();

        } catch (err) {
            console.error('[LibraryStore] CRITICAL ERROR during folder selection or reading:', err);
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
