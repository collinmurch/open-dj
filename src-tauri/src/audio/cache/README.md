# Audio Analysis Caching System

## Overview

The audio analysis caching system provides persistent storage for audio analysis results (BPM, waveform data) to avoid re-analyzing files that haven't changed. This significantly improves performance for large music libraries.

## Architecture

### Core Components

- **Fingerprinting**: Uses Blake3 hash of first 64KB + file metadata for change detection
- **Storage**: JSON files in `.open-dj/cache/metadata/` directory
- **Index**: Fast lookup table mapping file paths to cache entries
- **Fallback**: Always falls back to direct analysis if cache fails

### Cache Structure

```
music_library/
└── .open-dj/
    └── cache/
        └── metadata/
            ├── index.json           # Path -> hash mapping
            ├── {hash1}.json         # Cached analysis data
            ├── {hash2}.json         # More cached data
            └── ...
```

## Usage

### Frontend Commands

All existing analysis commands have cache-enabled variants:

```typescript
// Basic metadata with cache
const results = await invoke('analyze_features_batch_with_cache', {
  paths: ['/path/to/song1.mp3', '/path/to/song2.mp3'],
  cacheDir: '/path/to/music/library'
});

// Complete analysis (metadata + waveform) with cache
const results = await invoke('analyze_features_and_waveforms_batch_with_cache', {
  paths: ['/path/to/song.mp3'],
  cacheDir: '/path/to/music/library'
});
```

### Cache Management Commands

```typescript
// Ensure cache directory exists
const cacheDir = await invoke('ensure_cache_directory', {
  musicDir: '/path/to/music/library'
});

// Get cache statistics
const [entryCount, sizeBytes] = await invoke('get_cache_stats', {
  cacheDir: cacheDir
});

// Clean up orphaned cache entries
await invoke('cleanup_cache', {
  cacheDir: cacheDir,
  currentFiles: listOfCurrentMusicFiles
});

// Rebuild index from scratch
const entryCount = await invoke('rebuild_cache_index', {
  cacheDir: cacheDir
});

// Clear entire cache
await invoke('clear_cache', {
  cacheDir: cacheDir
});
```

## Performance Benefits

- **Cache Hit**: Sub-millisecond lookup for previously analyzed files
- **Cache Miss**: Standard analysis time + small caching overhead
- **Batch Operations**: Parallel processing with per-file cache checking
- **Graceful Degradation**: Cache failures never break analysis

## Cache Validation

Files are re-analyzed when:
- File size changes
- File modification time changes
- Cache entry is corrupted
- Cache directory is inaccessible

## Error Handling

The caching system is designed to be completely transparent:
- Cache errors are logged but don't fail analysis
- Corrupted cache entries are automatically cleaned up
- Analysis always succeeds even if caching fails
- Fallback to direct analysis is seamless

## Implementation Details

### File Fingerprinting

1. Read first 64KB of audio file
2. Compute Blake3 hash of content
3. Combine with file size and modification time
4. Store fingerprint with analysis results

### Cache Entry Format

```json
{
  "fingerprint": {
    "contentHash": "blake3_hex_string",
    "durationMs": 180000,
    "sampleRate": 44100,
    "fileSize": 5242880,
    "lastModified": "2024-01-15T10:30:00Z"
  },
  "bpmAnalysis": {
    "durationSeconds": 180.0,
    "bpm": 128.5,
    "firstBeatSec": 0.25
  },
  "waveformAnalysis": {
    "levels": [...],
    "maxBandEnergy": 0.85
  },
  "cachedAt": "2024-01-15T10:30:05Z"
}
```

### Index Format

```json
{
  "version": 1,
  "entries": {
    "/path/to/song1.mp3": "blake3_hash_1",
    "/path/to/song2.mp3": "blake3_hash_2"
  }
}
```

## Migration and Compatibility

- Cache version field allows for future format changes
- Index rebuilding handles format migrations
- Graceful fallback ensures compatibility across versions
- Cache directory can be safely deleted without data loss