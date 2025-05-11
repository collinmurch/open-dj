// src/lib/types.ts

import type { PlayerStore } from "./stores/playerStore";

// New structure for basic track metadata. Matches Rust struct TrackBasicMetadata.
export interface TrackBasicMetadata {
    durationSeconds: number | null;
    bpm: number | null;
    firstBeatSec: number | null;
}

// New structure for per-band energy. Matches Rust struct WaveBin.
export interface WaveBin {
    low: number;   // f32
    mid: number;   // f32
    high: number;  // f32
}

/**
 * 3-band EQ parameters for state and communication. Matches Rust struct EqParams.
 */
export interface EqParams {
    lowGainDb: number;
    midGainDb: number;
    highGainDb: number;
}

// Represents the full volume analysis data. Matches Rust struct AudioAnalysis.
export interface VolumeAnalysis {
    levels: WaveBin[][];
    maxBandEnergy: number;
}

// Structure for individual track information in the library
export interface TrackInfo {
    path: string;
    name: string;
    metadata?: TrackBasicMetadata | null | undefined;
    volumeAnalysisData?: VolumeAnalysis | null | undefined;
}

// Structure for the result of the new batch features analysis command
// This will now return TrackBasicMetadata instead of full AudioFeatures
export type BasicMetadataBatchResult = {
    [path: string]: {
        Ok?: TrackBasicMetadata;
        Err?: string;
    } | null;
};

// State for the library store
export interface LibraryState {
    selectedFolder: string | null;
    audioFiles: TrackInfo[];
    selectedTrack: TrackInfo | null;
    isLoading: boolean;
    isAnalyzing: boolean;
    error: string | null;
}

// --- Player Store Types ---

// Represents the state of an audio player instance
export interface PlayerState {
    currentTime: number;
    duration: number;
    isPlaying: boolean;
    isLoading: boolean;
    error: string | null;
    cuePointTime: number | null;
}

// --- Drag and Drop Types ---

// Data transferred during drag-and-drop operations
export interface DragData {
    source: 'library' | 'playerA' | 'playerB';
    track: TrackInfo;
}

// --- Global App State (Example, adjust as needed) ---

export interface AppState {
    library: LibraryState;
    playerA: PlayerState;
    playerB: PlayerState;
}

// --- Utility Types ---

// Type for the Player component instance
export interface PlayerComponent {
    togglePlay: () => void;
    seekAudio: (time: number) => void;
    seekBySeconds: (seconds: number) => void;
    element: HTMLDivElement | null;
    store: PlayerStore;
} 