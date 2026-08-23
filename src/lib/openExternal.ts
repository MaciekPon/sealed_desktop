import { isTauri } from "./tauriEnv";

/**
 * Opens a URL in the OS's default browser. Uses `@tauri-apps/plugin-opener`
 * (already registered on the Rust side in `lib.rs`) inside a real Tauri
 * webview; falls back to a plain `window.open` outside one (mock-mode
 * preview via `npm run dev` in a regular browser tab) since the plugin's
 * own `invoke` path isn't routed through `lib/invoke.ts`'s mock backend.
 */
export async function openExternalUrl(url: string): Promise<void> {
  if (isTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}
