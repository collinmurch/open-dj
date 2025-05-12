import { writable, get } from 'svelte/store';
import { invoke } from "@tauri-apps/api/core";

export type SyncStatus = 'off' | 'synced' | 'master' | 'pending';

interface SyncState {
    masterDeckId: 'A' | 'B' | null;
    deckASyncStatus: SyncStatus;
    deckBSyncStatus: SyncStatus;
    crossfaderValue: number; // 0.0 = Full A, 0.5 = Center, 1.0 = Full B
}

function createSyncStore() {
    const initialState: SyncState = {
        masterDeckId: null,
        deckASyncStatus: 'off',
        deckBSyncStatus: 'off',
        crossfaderValue: 0.5,
    };

    // Base writable store for the entire SyncState
    const { subscribe, set, update } = writable<SyncState>(initialState);

    // --- Actions ---

    function setCrossfader(value: number) {
        update(state => ({
            ...state,
            crossfaderValue: Math.max(0, Math.min(1, value)),
        }));
    }

    // Function called by +page.svelte when playerStore flags change
    function updateDeckSyncFlags(deckId: 'A' | 'B', isSyncActive: boolean, isMaster: boolean) {
        // console.log(`[SyncStore Update ${deckId}] Received flags: isSync=${isSyncActive}, isMaster=${isMaster}`); // REMOVED Log input
        update(state => {
            let newState = { ...state }; // Create mutable copy

            // Determine the new status for the reporting deck
            const newStatus = isMaster ? 'master' : (isSyncActive ? 'synced' : 'off');

            if (deckId === 'A') {
                newState.deckASyncStatus = newStatus;
            } else {
                newState.deckBSyncStatus = newStatus;
            }

            // Update masterDeckId and enforce consistency
            const otherDeckId = deckId === 'A' ? 'B' : 'A';
            if (isMaster) {
                newState.masterDeckId = deckId;
                // Ensure other deck is not master
                if (otherDeckId === 'A' && newState.deckASyncStatus === 'master') {
                    newState.deckASyncStatus = 'off'; // Turn off if it was master
                }
                if (otherDeckId === 'B' && newState.deckBSyncStatus === 'master') {
                    newState.deckBSyncStatus = 'off'; // Turn off if it was master
                }
            } else {
                // If the reporting deck is turning off master status, clear master only if it *was* the master
                if (state.masterDeckId === deckId) {
                    newState.masterDeckId = null;
                }
            }

            // Final consistency check: If a deck is synced, its master must be the current master
            if (newState.deckASyncStatus === 'synced' && newState.masterDeckId !== 'B') {
                newState.deckASyncStatus = 'off';
            }
            if (newState.deckBSyncStatus === 'synced' && newState.masterDeckId !== 'A') {
                newState.deckBSyncStatus = 'off';
            }
            // Ensure master deck actually has master status
            if (newState.masterDeckId === 'A' && newState.deckASyncStatus !== 'master') {
                newState.deckASyncStatus = 'master'; // Correct if needed
            }
            if (newState.masterDeckId === 'B' && newState.deckBSyncStatus !== 'master') {
                newState.deckBSyncStatus = 'master'; // Correct if needed
            }

            // console.log(`[SyncStore Update ${deckId}] New state:`, JSON.stringify(newState)); // REMOVED Log output
            return newState;
        });
    }

    async function enableSync(slaveDeckId: 'A' | 'B') {
        const masterDeckId = slaveDeckId === 'A' ? 'B' : 'A';
        // Optimistic UI update to 'pending'
        update(s => ({
            ...s,
            deckASyncStatus: slaveDeckId === 'A' ? 'pending' : s.deckASyncStatus,
            deckBSyncStatus: slaveDeckId === 'B' ? 'pending' : s.deckBSyncStatus,
        }));
        try {
            await invoke('enable_sync', { slaveDeckId, masterDeckId });
            // Backend event via playerStore will call updateDeckSyncFlags for final state
        } catch (err) {
            console.error(`[SyncStore] Failed to enable sync for deck ${slaveDeckId}:`, err);
            // Revert pending state on error
            update(s => ({
                ...s,
                deckASyncStatus: slaveDeckId === 'A' ? 'off' : s.deckASyncStatus,
                deckBSyncStatus: slaveDeckId === 'B' ? 'off' : s.deckBSyncStatus,
            }));
        }
    }

    async function disableSync(deckIdToDisable: 'A' | 'B') {
        // Optimistic UI update could go here if needed
        try {
            await invoke('disable_sync', { deckId: deckIdToDisable });
            // Backend event via playerStore will call updateDeckSyncFlags for final state
        } catch (err) {
            console.error(`[SyncStore] Failed to disable sync for deck ${deckIdToDisable}:`, err);
            // Revert optimistic state if needed
        }
    }

    return {
        subscribe,
        setCrossfader,
        enableSync,
        disableSync,
        updateDeckSyncFlags, // Expose the method
    };
}

export const syncStore = createSyncStore(); 