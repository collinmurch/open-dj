# Open DJ - Project Overview

## Architecture

Open DJ is a Tauri 2.0 desktop application for DJ mixing and control. The project follows a clear separation between frontend and backend:

- **Frontend (`src/`)**: Svelte 5 with TypeScript, handles UI, user interactions, and client-side state
- **Backend (`src-tauri/`)**: Rust application providing audio processing, file system access, and native functionality

## Technology Stack

- **Frontend**: Svelte 5 (with runes), TypeScript, Tailwind CSS, Bun
- **Backend**: Rust 2024 edition, Tauri 2.0
- **Package Manager**: Bun (not npm/yarn)

## Development Workflow

### Setup
```bash
bun install          # Install frontend dependencies
cd src-tauri && cargo check  # Verify Rust code
```

### Development
```bash
bun run dev          # Start development server
bun run build        # Build for production
```

### Code Quality
- Use TypeScript strict mode for all frontend code
- Fix all Rust warnings before considering tasks complete
- Prefer interfaces over types in TypeScript
- Avoid enums; use const objects for better type safety

## Critical Requirement: Svelte 5 Only

**This project uses Svelte 5 with runes.** Never use Svelte 4 syntax. Refer to `src/CLAUDE.md` for detailed Svelte 5 guidelines.

## Frontend Architecture

### State Management
- **Library Store**: Manages music folder selection, file discovery, and BPM analysis
- **Deck Stores**: Individual stores for each deck (A/B) handling track loading and audio controls
- **Player Stores**: Handle audio playback state, timing, and backend communication
- **Sync Store**: Manages crossfader and deck synchronization

### Component Structure
- **Main Page**: Orchestrates deck controllers, waveforms, and library
- **Deck Controllers**: Handle individual deck state and controls
- **Waveform Components**: WebGL-based audio visualization with beat lines
- **Library Components**: File browsing and track selection

### Data Flow
1. **Library loads** → metadata analysis (BPM, firstBeatSec) → store updates
2. **Track selection** → deck loading → player store updates → waveform props
3. **Beat lines rendering** → metadata lookup via runes → WebGL visualization

## Audio System

### Cue Audio (Headphone Monitoring)
- **Device Selection**: Audio output devices listed in library section
- **Per-Deck Control**: Each deck has a headphone button (🎧) for cue audio routing
- **Mutual Exclusion**: Only one deck can output to cue at a time
- **Platform Support**: Currently macOS only (CoreAudio), with cross-platform fallback stubs

### Audio Routing
- Main output: Both decks mixed through crossfader to main speakers
- Cue output: Selected deck's post-processed audio to chosen headphone device

### Beat Synchronization
- **Beat Detection**: Automatic BPM analysis and first beat detection via Rust backend
- **Beat Lines**: Orange grid lines rendered in waveforms at detected beat positions
- **Sync Controls**: Manual BPM adjustment and automatic beat matching between decks

## Inter-Process Communication

Communication between frontend and backend uses Tauri's IPC system:
- Frontend calls Rust functions via `invoke()` from `@tauri-apps/api/core`
- Use camelCase in frontend TypeScript interfaces
- Use snake_case in Rust with `#[serde(rename_all = "camelCase")]` for serialization

## General Principles

- Maintain clear separation between frontend (`src/`) and backend (`src-tauri/`)
- Follow official documentation for Svelte 5, Tauri 2.0, and TypeScript
- Avoid unnecessary comments unless implementation is complex
- No need to create markdown summaries of changes