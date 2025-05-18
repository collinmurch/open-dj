# Project Rules for Open DJ

## Overview
This is a Tauri 2.0 desktop application with Svelte 5 (TypeScript) frontend and Bun as the package manager. The app is a DJ controller allowing users to load songs and mix them together. The project structure is:

- `src/`: Svelte 5 frontend code (UI, song loading, mixing controls).
- `src-tauri/`: Rust code for native functionality (audio transformations, parsing).

Follow these rules to ensure correct, up-to-date, and project-specific code generation.

## CRITICAL: This Project Uses Svelte 5 ONLY
**NEVER use Svelte 4 syntax. This project uses Svelte 5 with runes.**

### Forbidden Svelte 4 Patterns (DO NOT USE):
```svelte
<!-- WRONG - Svelte 4 syntax -->
<script>
  export let prop = 'default';
  let count = 0;
  $: doubled = count * 2;
  $: console.log(count);
</script>
<button on:click={() => count++}>Click</button>
```

### Required Svelte 5 Patterns (USE THESE):
```svelte
<!-- CORRECT - Svelte 5 syntax -->
<script lang="ts">
  let { prop = 'default' } = $props<{ prop?: string }>();
  let count = $state(0);
  let doubled = $derived(count * 2);
  $effect(() => {
    console.log(count);
  });
</script>
<button onclick={() => count++}>Click</button>
```

## General Rules
- Use **TypeScript** for all JavaScript/TS code with strict mode enabled (`"strict": true` in `tsconfig.json`).
- Prefer **interfaces** over types for TypeScript definitions.
- Avoid enums; use const objects for better type safety.
- Use **Bun** as the package manager. Commands:
  - Install dependencies: `bun install`
  - Run dev server: `bun run dev`
  - Build: `bun run build`
- Follow the official Svelte 5, Tauri 2.0, and TypeScript documentation for best practices.
- Maintain a clear separation between frontend (`src/`) and backend (`src-tauri/`).
- There is no need to write a markdown file summarizing any given set of changes.
- Avoid commenting unless a function or implementation is especially difficult to digest.

## Rust Rules
Rust edition `2024` must be used. All packages and syntax must follow the `2024` edition and syntax specific to prior editions must be avoided.
- Snake case should be used for communication types when interfacing with frontend (Svelte) APIs, which should be set to camel case with `#[serde(rename_all = "camelCase")]`

## Svelte 5 Rules (RUNES SYSTEM)
Svelte 5 uses runes and updated syntax. LLMs must avoid Svelte 4 patterns.
- Camel case should be used for all interfaces that communicate with the backend (Rust) APIs

### Reactivity (Use Runes)
- Use `$state` for reactive state:
  ```typescript
  let count = $state(0);
  let user = $state({ name: 'John', age: 30 });
  ```
- Use `$derived` for computed values:
  ```typescript
  let doubled = $derived(count * 2);
  let fullName = $derived(`${user.firstName} ${user.lastName}`);
  ```
- Use `$effect` for side effects and lifecycle:
  ```typescript
  $effect(() => {
    console.log(`Count is now ${count}`);
  });

  // Cleanup effect
  $effect(() => {
    const interval = setInterval(() => {
      console.log('tick');
    }, 1000);

    return () => clearInterval(interval);
  });
  ```
- **NEVER USE** Svelte 4 `$:` reactive statements - they don't exist in Svelte 5

### Component Props (Use $props)
- Use `$props` for declaring props:
  ```typescript
  let { optionalProp = 42, requiredProp } = $props<{ optionalProp?: number; requiredProp: string }>();
  ```
- Use `$bindable` for two-way bindable props:
  ```typescript
  let { value = $bindable() } = $props<{ value?: number }>();
  ```
- **NEVER USE** Svelte 4 `export let` syntax - it doesn't work in Svelte 5

### Event Handling (Direct Event Properties)
- Use direct event properties instead of Svelte 4 `on:` syntax:
  ```svelte
  <button onclick={() => count++}>Increment</button>
  <input oninput={(e) => handleInput(e)} />
  <div onkeydown={(e) => handleKeyDown(e)}>Content</div>
  <form onsubmit={(e) => handleSubmit(e)}>...</form>
  ```
- **NEVER USE** `on:click`, `on:keydown`, `on:input`, etc. - they don't work in Svelte 5

### Svelte 5 Component Communication
- For parent-child communication, use props and callbacks:
  ```svelte
  <!-- Parent -->
  <script lang="ts">
    let volume = $state(50);
  </script>
  <VolumeSlider {volume} onVolumeChange={(newVolume) => volume = newVolume} />

  <!-- Child: VolumeSlider.svelte -->
  <script lang="ts">
    let { volume, onVolumeChange } = $props<{
      volume: number;
      onVolumeChange: (value: number) => void;
    }>();
  </script>
  <input type="range" value={volume} oninput={(e) => onVolumeChange(+e.target.value)} />
  ```

### Component Development
- Use kebab-case for component file names (e.g., `song-player.svelte`).
- Create `.svelte` files for UI components and `.svelte.ts` for state logic:
  ```typescript
  // song-player.svelte.ts
  class SongPlayer {
    track = $state<Song | null>(null);
    volume = $state(50);
    isPlaying = $state(false);

    async play() {
      const result = await invoke('play_song', { songId: this.track?.id });
      this.isPlaying = true;
    }
  }
  export const songPlayer = new SongPlayer();
  ```
  ```svelte
  <!-- song-player.svelte -->
  <script lang="ts">
    import { songPlayer } from './song-player.svelte.ts';
  </script>
  <div class="player">
    <h3>{songPlayer.track?.name || 'No track loaded'}</h3>
    <button onclick={() => songPlayer.play()}>
      {songPlayer.isPlaying ? 'Pause' : 'Play'}
    </button>
  </div>
  ```
- Use Svelte stores for global state (e.g., playlist management):
  ```typescript
  // src/lib/stores.ts
  import { writable } from 'svelte/store';
  export const playlist = writable<Song[]>([]);
  ```

### UI and Styling
- Use **Tailwind CSS** for styling with the `cn()` utility from `$lib/utils`.
- Import Shadcn components from `$lib/components/ui` for reusable UI elements.
- Define CSS variables without color space functions:
  ```css
  :root {
    --primary: 222.2 47.4% 11.2%;
    --primary-foreground: 210 40% 98%;
  }
  ```
- Example usage:
  ```svelte
  <div class="bg-primary text-primary-foreground">Mixing Panel</div>
  ```

## Tauri 2.0 Rules
- Use `@sveltejs/adapter-static` in `svelte.config.js` to disable SSR and enable SSG or SPA mode:
  ```javascript
  import adapter from '@sveltejs/adapter-static';
  import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
  export default {
    preprocess: vitePreprocess(),
    kit: { adapter: adapter() }
  };
  ```
- Disable SSR in `src/routes/+layout.ts`:
  ```typescript
  export const ssr = false;
  export const prerender = true;
  ```
- Configure `tauri.conf.json` in `src-tauri/`:
  - Set `build.distDir` to `../build`.
  - Set `build.devPath` to `http://localhost:5173`.
  - Example:
    ```json
    {
      "build": {
        "beforeBuildCommand": "bun run build",
        "beforeDevCommand": "bun run dev",
        "devPath": "http://localhost:5173",
        "distDir": "../build"
      },
      "tauri": {
        "bundle": {
          "identifier": "com.djcontroller.app"
        }
      }
    }
    ```
- Use Tauri's `@tauri-apps/api` for IPC (Inter-Process Communication) to call Rust functions:
  ```svelte
  <script lang="ts">
    import { invoke } from '@tauri-apps/api/core';

    let songPath = $state('');
    let loading = $state(false);

    async function loadSong() {
      loading = true;
      try {
        const result = await invoke('load_song', { filePath: songPath });
        console.log('Song loaded:', result);
      } catch (error) {
        console.error('Failed to load song:', error);
      } finally {
        loading = false;
      }
    }
  </script>
  ```
- Define Rust commands in `src-tauri/src/main.rs` for audio processing:
  ```rust
  #[tauri::command]
  fn load_song(file_path: String) -> Result<String, String> {
    // Audio processing logic
    Ok("Song loaded".to_string())
  }
  ```

## Accessibility
- Use semantic HTML (e.g., `<button>`, `<input>`) in Svelte components.
- Add ARIA attributes for mixing controls (e.g., `aria-label="Adjust volume"`).
- Ensure keyboard navigation for play/pause, volume, and track selection.

## Common LLM Pitfalls to Avoid

### CRITICAL SVELTE 5 VIOLATIONS (NEVER DO THESE):
- ❌ **DO NOT USE**: `on:click` → ✅ **USE**: `onclick`
- ❌ **DO NOT USE**: `on:input` → ✅ **USE**: `oninput`
- ❌ **DO NOT USE**: `on:keydown` → ✅ **USE**: `onkeydown`
- ❌ **DO NOT USE**: `$: reactive = statement` → ✅ **USE**: `let reactive = $derived(statement)`
- ❌ **DO NOT USE**: `$: { sideEffect(); }` → ✅ **USE**: `$effect(() => { sideEffect(); })`
- ❌ **DO NOT USE**: `export let prop` → ✅ **USE**: `let { prop } = $props()`
- ❌ **DO NOT USE**: `let count = 0` (for reactive state) → ✅ **USE**: `let count = $state(0)`

### Other Common Mistakes:
- **Tauri Misconfigurations**:
  - Do not enable SSR in `svelte.config.js`.
  - Do not use server-side load functions for Tauri APIs during prerendering.
- **Bun Errors**:
  - Do not use `npm` or `yarn` commands; use `bun`.
  - Do not use `bun:sqlite3`; use `bun:sqlite`.

## Before Writing Any Svelte Code, Ask:
1. Am I using `$state` for reactive variables?
2. Am I using `$derived` instead of `$:` for computations?
3. Am I using `$effect` instead of `$:` for side effects?
4. Am I using `onclick` instead of `on:click`?
5. Am I using `$props` instead of `export let`?

## References
- Svelte 5: https://svelte.dev/docs
- Tauri 2.0: https://v2.tauri.app/
- TypeScript: https://www.typescriptlang.org/docs/
- Tailwind CSS: https://tailwindcss.com/docss
