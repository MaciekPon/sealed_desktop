import { invoke as realInvoke } from "@tauri-apps/api/core";
import { isTauri } from "./tauriEnv";
import { mockInvoke } from "./mockBackend";

/** Drop-in replacement for `@tauri-apps/api/core`'s `invoke` — routes to the mock backend outside a real Tauri webview. See `tauriEnv.ts`. */
export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return realInvoke<T>(cmd, args);
  return mockInvoke<T>(cmd, args);
}
