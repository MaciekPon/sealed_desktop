/** App preferences — mirrors `AppSettingsService` (desktop-relevant subset only, see `settings.rs`). */

import { create } from "zustand";
import { settings } from "../lib/tauri";

interface SettingsState {
  autoSyncEnabled: boolean;
  notificationsEnabled: boolean;
  minimizeToTrayEnabled: boolean;
  loaded: boolean;
  load: () => Promise<void>;
  setAutoSyncEnabled: (enabled: boolean) => Promise<void>;
  setNotificationsEnabled: (enabled: boolean) => Promise<void>;
  setMinimizeToTrayEnabled: (enabled: boolean) => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  autoSyncEnabled: true,
  notificationsEnabled: true,
  minimizeToTrayEnabled: true,
  loaded: false,

  load: async () => {
    const current = await settings.get();
    set({
      autoSyncEnabled: current.autoSyncEnabled,
      notificationsEnabled: current.notificationsEnabled,
      minimizeToTrayEnabled: current.minimizeToTrayEnabled,
      loaded: true,
    });
  },

  setAutoSyncEnabled: async (enabled) => {
    await settings.setAutoSyncEnabled(enabled);
    set({ autoSyncEnabled: enabled });
  },

  setNotificationsEnabled: async (enabled) => {
    await settings.setNotificationsEnabled(enabled);
    set({ notificationsEnabled: enabled });
  },

  setMinimizeToTrayEnabled: async (enabled) => {
    await settings.setMinimizeToTrayEnabled(enabled);
    set({ minimizeToTrayEnabled: enabled });
  },
}));
