# Open DJ - Backend (Rust) Guidelines

## Rust Requirements

- **Edition**: Use Rust 2024 edition exclusively
- **Quality**: Fix all `cargo check` warnings before completing tasks
- **Testing**: Use `cargo check` instead of building the full application during development

## Backend Architecture

The Rust backend is organized around audio processing and system integration:

```
src/
├── audio/              # Core audio processing
│   ├── analysis/       # BPM and volume analysis
│   ├── devices/        # Audio device management and cue output
│   │   ├── commands.rs # Tauri commands (get_audio_devices, set_cue_deck)
│   │   ├── macos.rs    # CoreAudio device detection (macOS)
│   │   └── store.rs    # Device state management
│   ├── playback/       # Audio playback with commands, events, handlers
│   │   └── handlers/
│   │       └── cue_output.rs  # Cue audio routing and management
│   ├── config.rs       # Audio configuration
│   ├── decoding.rs     # Audio file decoding
│   ├── effects.rs      # Audio effects processing
│   ├── processor.rs    # Main audio processor
│   └── types.rs        # Audio-related types
├── lib.rs              # Library entry point
└── main.rs             # Application entry point
```

## Audio Device System

### Cue Audio Implementation
- **Platform Support**: macOS (CoreAudio) with cross-platform fallback stubs
- **Device Detection**: `audio/devices/macos.rs` handles CoreAudio device enumeration
- **Audio Routing**: `audio/playback/handlers/cue_output.rs` manages per-deck cue output
- **Commands**: `set_cue_deck(deck_id)` switches which deck outputs to cue

## Tauri Command Patterns

### Command Definition
```rust
#[tauri::command]
async fn process_audio(file_path: String, options: AudioOptions) -> Result<AudioResult, String> {
    // Implementation
    Ok(result)
}
```

### Error Handling
- Return `Result<T, String>` for Tauri commands
- Use descriptive error messages for frontend debugging
- Handle panics gracefully in async contexts

### State Management
- Use Tauri's state management for shared resources
- Prefer async functions for I/O operations
- Use proper mutexes for thread-safe access to shared state

## Frontend Interface

### Serialization
- Use `#[serde(rename_all = "camelCase")]` for frontend-facing types
- Frontend expects camelCase, backend uses snake_case internally

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    file_path: String,
    duration_ms: u64,
    sample_rate: u32,
}
```

### Communication Types
- Create dedicated types for IPC communication
- Keep serializable types simple and focused
- Use appropriate derive macros for serde

## Audio Processing Guidelines

### Performance
- Use async/await for I/O operations
- Consider threading for CPU-intensive audio processing
- Cache analysis results when appropriate

### File Handling
- Validate file paths before processing
- Handle audio format variations gracefully
- Use appropriate error types for different failure modes

## Development Workflow

### Code Quality
```bash
cd src-tauri
cargo check          # Verify code compiles and check for warnings
cargo clippy          # Additional linting (if configured)
```

### Dependencies
- Add new dependencies thoughtfully
- Prefer well-maintained crates for audio processing
- Check compatibility with Tauri 2.0

## Common Patterns

### Async Commands
```rust
#[tauri::command]
async fn load_track(path: String) -> Result<TrackMetadata, String> {
    tokio::task::spawn_blocking(move || {
        // CPU-intensive work
    }).await.map_err(|e| e.to_string())?
}
```

### State Access
```rust
#[tauri::command]
async fn get_playback_state(state: tauri::State<'_, PlaybackState>) -> Result<PlayerStatus, String> {
    let status = state.lock().await;
    Ok(status.clone())
}
```