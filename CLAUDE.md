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