// src/lib/types.ts

export interface TrackInfo {
    path: string;
    name: string;
    bpm?: number | null;
}

// Represents a single interval from the analysis
export interface VolumeInterval {
    start_time: number;
    end_time: number;
    rms_amplitude: number;
}

// Represents the full analysis result from the backend
export interface AudioAnalysis {
    intervals: VolumeInterval[];
    max_rms_amplitude: number;
} 