/**
 * In-app auto-update state, backed by `tauri-plugin-updater` (Rust) +
 * `@tauri-apps/plugin-updater`/`@tauri-apps/plugin-process` (JS). The
 * `Update` handle itself (needed to actually download+install) is kept in a
 * module-level variable rather than store state — it isn't serializable
 * data, just a live object with methods, and nothing needs to react to it
 * directly (components only read the plain fields below).
 *
 * The endpoint (`tauri.conf.json`'s `plugins.updater.endpoints`) only ever
 * resolves to a *published* GitHub release, never a draft — so nothing here
 * can trigger on a build nobody's actually shipped yet.
 */
import { create } from "zustand";
import { isTauri } from "../lib/tauriEnv";
import type { Update } from "@tauri-apps/plugin-updater";

let pendingUpdate: Update | null = null;

interface UpdateState {
  checked: boolean;
  available: boolean;
  version: string | null;
  installing: boolean;
  error: string | null;
  check: () => Promise<void>;
  installAndRelaunch: () => Promise<void>;
}

export const useUpdateStore = create<UpdateState>((set) => ({
  checked: false,
  available: false,
  version: null,
  installing: false,
  error: null,

  check: async () => {
    // Not in a real Tauri webview (mock/dev-preview mode) — nothing to
    // check against, and the plugin isn't there to import.
    if (!isTauri) {
      set({ checked: true });
      return;
    }
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update?.available) {
        pendingUpdate = update;
        set({ checked: true, available: true, version: update.version });
      } else {
        set({ checked: true, available: false });
      }
    } catch {
      // A transient network hiccup or unreachable endpoint should never
      // surface as an error to the user — just means no banner shows.
      set({ checked: true, available: false });
    }
  },

  installAndRelaunch: async () => {
    if (!pendingUpdate) return;
    set({ installing: true, error: null });
    try {
      await pendingUpdate.downloadAndInstall();
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      set({ installing: false, error: String(e) });
    }
  },
}));
