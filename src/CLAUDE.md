# Open DJ - Frontend (Svelte 5) Guidelines

## 🚨 CRITICAL: This Project Uses Svelte 5 ONLY

**NEVER use Svelte 4 syntax. This project uses Svelte 5 with runes.**

This is the #1 requirement. LLMs frequently default to Svelte 4 patterns. Always use Svelte 5 runes system.

### ❌ FORBIDDEN Svelte 4 Patterns:
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

### ✅ REQUIRED Svelte 5 Patterns:
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

## Frontend Architecture

```
src/
├── app.html                    # Application entry point
├── lib/
│   ├── components/            # Svelte components
│   │   ├── AudioDeviceSelector.svelte  # Cue output device selection
│   │   ├── DeckControls.svelte         # Includes cue audio button (🎧)
│   │   ├── MusicLibrary.svelte
│   │   ├── Slider.svelte
│   │   └── WaveformDisplay.svelte
│   ├── stores/               # Global state management
│   │   ├── libraryStore.ts
│   │   ├── playerStore.ts
│   │   └── syncStore.ts
│   ├── types.ts              # TypeScript type definitions
│   └── utils/                # Utility functions
│       ├── timeUtils.ts
│       └── webglWaveformUtils.ts
└── routes/                   # SvelteKit routing
    ├── +layout.ts
    └── +page.svelte
```

## Svelte 5 Runes System

### Reactivity with $state
```typescript
// Reactive state
let volume = $state(50);
let currentTrack = $state<Track | null>(null);
let deckState = $state({
  isPlaying: false,
  bpm: 120,
  position: 0
});
```

### Computed Values with $derived
```typescript
// Computed/derived values
let formattedTime = $derived(formatTime(currentTrack?.duration || 0));
let canPlay = $derived(currentTrack !== null && !isLoading);
let syncedBpm = $derived(deckState.bpm * syncRatio);
```

### Side Effects with $effect
```typescript
// Side effects and lifecycle
$effect(() => {
  console.log(`Volume changed to ${volume}`);
});

// Cleanup effect
$effect(() => {
  const interval = setInterval(() => updatePlaybackPosition(), 100);
  return () => clearInterval(interval);
});
```

### Component Props with $props
```typescript
// Component props
let { 
  trackId, 
  volume = 50, 
  onVolumeChange 
} = $props<{
  trackId: string;
  volume?: number;
  onVolumeChange: (value: number) => void;
}>();
```

### Two-way Binding with $bindable
```typescript
// For bindable props
let { value = $bindable(0) } = $props<{ value?: number }>();
```

## Event Handling (Direct Properties)

**NEVER use `on:` syntax - it doesn't work in Svelte 5**

```svelte
<!-- ✅ CORRECT Svelte 5 event handling -->
<button onclick={() => playTrack()}>Play</button>
<input oninput={(e) => handleVolumeChange(+e.target.value)} />
<div onkeydown={(e) => handleKeyPress(e)}>Deck Controls</div>
<form onsubmit={(e) => handleSubmit(e)}>Search</form>

<!-- ❌ WRONG - Svelte 4 syntax -->
<button on:click={() => playTrack()}>Play</button>
<input on:input={(e) => handleVolumeChange(+e.target.value)} />
```

## Component Development

### File Naming
- Use kebab-case for component files: `deck-controls.svelte`, `waveform-display.svelte`
- Components are in `src/lib/components/`

### Component Structure
```svelte
<!-- deck-controls.svelte -->
<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  
  let { 
    trackId,
    onPlay,
    onPause 
  } = $props<{
    trackId: string;
    onPlay: () => void;
    onPause: () => void;
  }>();
  
  let isPlaying = $state(false);
  let volume = $state(50);
  
  async function togglePlayback() {
    if (isPlaying) {
      await invoke('pause_track', { trackId });
      onPause();
    } else {
      await invoke('play_track', { trackId });
      onPlay();
    }
    isPlaying = !isPlaying;
  }
</script>

<div class="deck-controls">
  <button onclick={togglePlayback}>
    {isPlaying ? 'Pause' : 'Play'}
  </button>
  <input 
    type="range" 
    bind:value={volume}
    oninput={(e) => invoke('set_volume', { volume: +e.target.value })}
  />
</div>
```

### State Logic Files
Create `.svelte.ts` files for complex state logic:

```typescript
// deck-controller.svelte.ts
class DeckController {
  trackId = $state<string | null>(null);
  volume = $state(50);
  isPlaying = $state(false);
  
  async play() {
    if (this.trackId) {
      await invoke('play_track', { trackId: this.trackId });
      this.isPlaying = true;
    }
  }
}

export const deckController = new DeckController();
```

## Global State Management

Use Svelte stores for application-wide state:

```typescript
// lib/stores/playerStore.ts
import { writable } from 'svelte/store';

interface PlayerState {
  currentTrack: Track | null;
  volume: number;
  isPlaying: boolean;
}

export const playerStore = writable<PlayerState>({
  currentTrack: null,
  volume: 50,
  isPlaying: false
});
```

## Tauri Integration

### Backend Communication
```typescript
import { invoke } from '@tauri-apps/api/core';

// Call Rust functions
async function loadTrack(filePath: string) {
  try {
    const result = await invoke('load_track', { filePath });
    return result;
  } catch (error) {
    console.error('Failed to load track:', error);
    throw error;
  }
}
```

### Interface Types
Use camelCase for types that communicate with Rust backend:

```typescript
// lib/types.ts
export interface TrackMetadata {
  filePath: string;
  durationMs: number;
  sampleRate: number;
  bpm?: number;
}
```

## UI and Styling

### Tailwind CSS Usage
```svelte
<div class="bg-gray-900 text-white p-4 rounded-lg">
  <h2 class="text-xl font-bold mb-2">Deck Controls</h2>
  <div class="flex gap-4 items-center">
    <button class="bg-blue-500 hover:bg-blue-600 px-4 py-2 rounded">
      Play
    </button>
  </div>
</div>
```

### CSS Variables
```css
:root {
  --primary: 222.2 47.4% 11.2%;
  --primary-foreground: 210 40% 98%;
  --accent: 210 40% 8%;
}
```

## Accessibility

- Use semantic HTML elements (`<button>`, `<input>`, `<label>`)
- Add ARIA labels for complex controls:
  ```svelte
  <input 
    type="range" 
    aria-label="Adjust volume" 
    bind:value={volume}
  />
  ```
- Ensure keyboard navigation works for all controls

## CRITICAL Svelte 5 Violations to Avoid

**These will break your code in Svelte 5:**

- ❌ `on:click` → ✅ `onclick`
- ❌ `on:input` → ✅ `oninput`  
- ❌ `on:keydown` → ✅ `onkeydown`
- ❌ `$: reactive = statement` → ✅ `let reactive = $derived(statement)`
- ❌ `$: { sideEffect(); }` → ✅ `$effect(() => { sideEffect(); })`
- ❌ `export let prop` → ✅ `let { prop } = $props()`
- ❌ `let count = 0` (for reactive state) → ✅ `let count = $state(0)`

## Pre-Flight Checklist

Before writing Svelte code, verify:

1. ✅ Am I using `$state` for reactive variables?
2. ✅ Am I using `$derived` instead of `$:` for computations?
3. ✅ Am I using `$effect` instead of `$:` for side effects?
4. ✅ Am I using `onclick` instead of `on:click`?
5. ✅ Am I using `$props` instead of `export let`?

## Configuration

### SvelteKit Setup
Ensure these are configured for Tauri compatibility:

```typescript
// routes/+layout.ts
export const ssr = false;
export const prerender = true;
```

```javascript
// svelte.config.js
import adapter from '@sveltejs/adapter-static';
export default {
  kit: { adapter: adapter() }
};
```