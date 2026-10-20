//! Background sync task + native notification toast.
//!
//! Mirrors `message_service.dart`'s intent (get new messages onto the
//! device without the user manually refreshing) but not its mechanism:
//! mobile's module doc states "Sync model: silent push wake → chain scan
//! ... No WebSocket; no HTTP polling" — it relies entirely on APNs/FCM
//! silent push to wake the app and trigger `onPushWakeup()`. Desktop has
//! no equivalent OS-level wake mechanism, so per the plan this polls on a
//! fixed interval instead — the one deliberate mechanism swap called out
//! up front, not a bug or an oversight.
//!
//! The toast content mirrors mobile's own privacy rule exactly: mobile's
//! `NotificationService.kGenericNotificationTitle`/`kGenericNotificationBody`
//! are hard-coded to "Sealed"/"New Encrypted Message" and *never* derived
//! from actual message content (a silent/local push payload could reveal
//! metadata to the OS otherwise) — this reuses the same fixed strings for
//! the same reason.
//!
//! Reuses `commands::messaging::sync_messages` rather than re-deriving the
//! session-locking dance — see that module's doc comment for why every
//! phase that both touches the db and awaits network calls needs its own
//! `state.session` lock scope.
//!
//! Clicking the toast brings the window back (2026-09-01, Windows only —
//! see `notify_new_messages`'s doc comment): particularly relevant now that
//! "minimize to tray on close" (see `tray.rs`) can leave the window hidden
//! when a notification arrives.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(target_os = "windows"))]
use tauri_plugin_notification::NotificationExt;

use crate::settings;
use crate::state::AppState;

/// How often to poll for new messages while unlocked. Not derived from
/// mobile (which never polls) — a desktop-specific default. Lowered from
/// 30s to 8s (2026-08-11, user feedback: 30s felt "terribly long" for a
/// desktop chat app), then to 3s (2026-08-12, user wants messages to feel
/// closer to instant and is trying this interval live) — each tick's
/// actual chain/indexer request cost is bounded regardless of this
/// interval (an incremental sync only fetches messages since the last
/// successful sync, typically zero or a single-page result via
/// `query_indexer_transactions`'s pagination), so even 3s stays far from
/// "hammering" the OHTTP-relayed AlgoNode/Sealed-indexer endpoints for a
/// single-user desktop client.
const POLL_INTERVAL: Duration = Duration::from_secs(3);

const NOTIFICATION_TITLE: &str = "Sealed";
const NOTIFICATION_BODY: &str = "New Encrypted Message";

/// Frontend event name, fired after a background tick that cached at
/// least one new message — the frontend listens for this to invalidate
/// its TanStack Query caches instead of polling the DB itself from JS.
pub const MESSAGES_UPDATED_EVENT: &str = "messages-updated";

/// Start the background poll loop. Call once, from `lib.rs`'s `setup`
/// hook, after `AppState` is managed. Runs for the lifetime of the app;
/// each tick is best-effort (errors are logged, never propagated — a
/// transient network hiccup must not crash or wedge the loop).
pub fn spawn(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(POLL_INTERVAL).await;
            tick(&app_handle).await;
        }
    });
}

async fn tick(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();

    // Skip silently while locked — confined to its own block so the
    // guard is dropped before any `.await` that follows (see this
    // module's doc comment on why that matters for `Send`).
    {
        let session_guard = state.session.lock().await;
        if session_guard.is_none() {
            return;
        }
    }

    let app_settings = settings::load(&state.app_dir);
    if !app_settings.auto_sync_enabled {
        return;
    }

    match crate::commands::messaging::sync_messages(state, false).await {
        Ok(new_count) if new_count > 0 => {
            // Sync itself, and the event the frontend relies on for
            // badges/chat-window updates, always run regardless of this
            // setting — only the native OS toast is gated. Turning
            // notifications off must never turn off sync (2026-09-01,
            // explicit product decision — see `settings.rs`'s doc comment).
            if app_settings.notifications_enabled {
                notify_new_messages(app_handle);
            }
            if let Err(e) = app_handle.emit(MESSAGES_UPDATED_EVENT, ()) {
                eprintln!("[sync] failed to emit {MESSAGES_UPDATED_EVENT}: {e}");
            }
        }
        Ok(_) => {}
        Err(e) => eprintln!("[sync] background tick failed: {e}"),
    }

    // Recomputed every tick (not just on `new_count > 0`): the badge must
    // also drop promptly when the user reads messages, which happens via
    // a completely separate command (`mark_conversation_as_read`) this
    // loop never sees directly. At a 3s poll interval, driving it off the
    // regular tick keeps it accurate without a second notification path.
    // Fresh `state` lookup — the earlier binding was moved into
    // `sync_messages` above (`tauri::State` isn't `Copy`).
    let state = app_handle.state::<AppState>();
    let session_guard = state.session.lock().await;
    if let Some(session) = session_guard.as_ref() {
        update_unread_badge(app_handle, session.db.connection());
    }
}

/// Sums unread counts across regular + alias conversations and pushes the
/// result to the platform's taskbar/Dock icon. Best-effort — any failure
/// here must never affect the sync tick itself.
///
/// macOS/Linux get a real number via `set_badge_count`. Windows has no
/// numeric badge API at all (Tauri's own doc comment on it says
/// "Unsupported, use `set_overlay_icon` instead") — `set_overlay_icon`
/// only takes an image, not a count, so Windows gets a plain red dot
/// (generated in-memory, no asset file) rather than the real number.
fn update_unread_badge(app_handle: &AppHandle, conn: &rusqlite::Connection) {
    let regular: i64 = crate::messages::get_conversations(conn).map(|rows| rows.iter().map(|c| c.unread_count).sum()).unwrap_or(0);
    let alias: i64 =
        crate::alias::messages::get_alias_conversations(conn).map(|rows| rows.iter().map(|c| c.unread_count).sum()).unwrap_or(0);
    let total = regular + alias;

    let Some(window) = app_handle.get_webview_window("main") else { return };

    #[cfg(not(target_os = "windows"))]
    {
        let _ = window.set_badge_count(if total > 0 { Some(total) } else { None });
    }
    #[cfg(target_os = "windows")]
    {
        let _ = window.set_overlay_icon(if total > 0 { Some(unread_dot_image()) } else { None });
    }
}

/// A plain red dot with a white ring (for contrast against dark taskbar
/// icons), rendered directly as an RGBA buffer — no bundled asset file.
/// Computed once and cached: the image itself never changes, only whether
/// it's currently applied.
#[cfg(target_os = "windows")]
fn unread_dot_image() -> tauri::image::Image<'static> {
    use std::sync::OnceLock;
    static IMAGE: OnceLock<tauri::image::Image<'static>> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            const SIZE: u32 = 32;
            const CENTER: f32 = SIZE as f32 / 2.0;
            const RADIUS: f32 = 13.0;
            const BORDER: f32 = 2.0;
            let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let dx = x as f32 + 0.5 - CENTER;
                    let dy = y as f32 + 0.5 - CENTER;
                    let dist = (dx * dx + dy * dy).sqrt();
                    let idx = ((y * SIZE + x) * 4) as usize;
                    if dist <= RADIUS {
                        rgba[idx..idx + 4].copy_from_slice(&[0xD3, 0x17, 0x37, 0xFF]); // brand destructive red
                    } else if dist <= RADIUS + BORDER {
                        rgba[idx..idx + 4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
                    }
                    // else: stays fully transparent (buffer is zero-initialized)
                }
            }
            tauri::image::Image::new_owned(rgba, SIZE, SIZE)
        })
        .clone()
}

/// On Windows, bypasses `tauri-plugin-notification` and builds the toast
/// with `notify-rust` directly instead — the Tauri plugin's desktop backend
/// only ever fire-and-forgets (`show()` -> `Result<()>`, no handle); its
/// click/action support (`register_action_types`) is wired up for mobile
/// only. `notify-rust`'s own Windows backend returns a `NotificationHandle`
/// whose `wait_for_response` blocks until the toast is clicked or dismissed
/// — spawned on its own OS thread (not the tokio runtime: it's a blocking
/// `std::sync::mpsc` recv, not an async API) so a click can bring the
/// window back via `tray::show_main_window`. Not attempted on macOS/Linux:
/// `notify-rust`'s per-platform handle types aren't verified to share this
/// same API shape there, and there's no way to test a macOS build locally
/// in this project — left on the original Tauri-plugin path unchanged
/// there, so nothing regresses on a platform this can't be verified against.
#[cfg(target_os = "windows")]
fn notify_new_messages(app_handle: &AppHandle) {
    match notify_rust::Notification::new().summary(NOTIFICATION_TITLE).body(NOTIFICATION_BODY).show() {
        Ok(handle) => {
            let app_handle = app_handle.clone();
            std::thread::spawn(move || {
                // `ResponseHandler` requires `Fn(&NotificationResponse)`, not
                // `Fn(NotificationResponse)` by value — borrow the parameter
                // (match ergonomics let `matches!` see through the reference).
                let _ = handle.wait_for_response(move |response: &notify_rust::NotificationResponse| {
                    if !matches!(response, notify_rust::NotificationResponse::Closed(_)) {
                        crate::tray::show_main_window(&app_handle);
                    }
                });
            });
        }
        Err(e) => eprintln!("[sync] failed to show notification: {e}"),
    }
}

#[cfg(not(target_os = "windows"))]
fn notify_new_messages(app_handle: &AppHandle) {
    if let Err(e) = app_handle.notification().builder().title(NOTIFICATION_TITLE).body(NOTIFICATION_BODY).show() {
        eprintln!("[sync] failed to show notification: {e}");
    }
}
