// src/lib/types.ts

import type { PlayerStore } from "./stores/playerStore";

// Represents the analysis results for volume intervals. Matches Rust struct.
export interface VolumeInterval {
    start_time: number; // f64 in Rust maps to number
    end_time: number;
    rms_amplitude: number; // f32 in Rust maps to number
}

// Represents the full volume analysis data. Matches Rust struct.
export interface VolumeAnalysis {
    intervals: VolumeInterval[];
    max_rms_amplitude: number;
}

// Represents the combined audio features returned by Rust. Matches Rust struct.
export interface AudioFeatures {
    bpm: number | null; // Option<f32> -> number | null
    volume: VolumeAnalysis | null; // Option<AudioAnalysis> -> VolumeAnalysis | null
    durationSeconds?: number | null; // Added duration (Option<f64> -> number | null)
}

// Structure for individual track information in the library
export interface TrackInfo {
    path: string;
    name: string;
    bpm?: number | null; // Optional, null means analysis failed or pending
    features?: AudioFeatures | null | undefined; // Undefined: pending, null: error, AudioFeatures: success
    durationSeconds?: number | null; // Added for direct access
}

// Structure for the result of the batch volume analysis command
export type VolumeAnalysisBatchResult = {
    [path: string]: {
        Ok?: VolumeAnalysis;
        Err?: string;
    } | null; // Rust's Result<AudioAnalysis, String>
};

// Structure for the result of the new batch features analysis command
export type FeaturesAnalysisBatchResult = {
    [path: string]: {
        Ok?: AudioFeatures;
        Err?: string;
    } | null; // Rust's Result<AudioFeatures, String>
};

// Structure expected from the Rust BPM command
export interface BpmAnalysisResult {
    bpm: number;
}

// Structure expected from Rust Result<T, E> serialization
export interface RustResult<T, E> {
    Ok?: T;
    Err?: E;
}

// State for the library store
export interface LibraryState {
    selectedFolder: string | null;
    audioFiles: TrackInfo[];
    selectedTrack: TrackInfo | null;
    isLoading: boolean; // Loading folder contents
    isAnalyzing: boolean; // Running Rust analysis
    error: string | null;
}

// --- Player Store Types ---

// Represents the state of an audio player instance
export interface PlayerState {
    currentTime: number;
    duration: number;
    isPlaying: boolean;
    isLoading: boolean; // Loading audio file itself
    error: string | null;
    cuePointTime: number | null; // Added cue point time (maps from cue_point_seconds)
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
    // Add other global states like crossfader position, etc.
}

// --- Utility Types ---

// Type for the Player component instance
export interface PlayerComponent {
    togglePlay: () => void;
    seekAudio: (time: number) => void;
    seekBySeconds: (seconds: number) => void;
    // Add other methods if needed
    element: HTMLDivElement | null; // Reference to the root element
    store: PlayerStore;
} 