/**
 * True when running inside an actual Tauri webview (i.e. launched via the
 * compiled Rust binary). False when the frontend is opened as a plain
 * page — e.g. `npm run dev` in a regular browser, used as a visual-only
 * preview of the UI when the compiled app can't be run locally (see
 * `lib/mockBackend.ts`).
 */
export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
