/** App preferences — mirrors `AppSettingsService` (desktop-relevant subset only, see `settings.rs`). */

import { create } from "zustand";
import { settings } from "../lib/tauri";

interface SettingsState {
  autoSyncEnabled: boolean;
  loaded: boolean;
  load: () => Promise<void>;
  setAutoSyncEnabled: (enabled: boolean) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  autoSyncEnabled: true,
  loaded: false,

  load: async () => {
    const current = await settings.get();
    set({ autoSyncEnabled: current.autoSyncEnabled, loaded: true });
  },

  setAutoSyncEnabled: async (enabled) => {
    await settings.setAutoSyncEnabled(enabled);
    set({ autoSyncEnabled: enabled });
  },
}));
