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

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
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

    if !settings::load(&state.app_dir).auto_sync_enabled {
        return;
    }

    match crate::commands::messaging::sync_messages(state, false).await {
        Ok(new_count) if new_count > 0 => {
            notify_new_messages(app_handle);
            if let Err(e) = app_handle.emit(MESSAGES_UPDATED_EVENT, ()) {
                eprintln!("[sync] failed to emit {MESSAGES_UPDATED_EVENT}: {e}");
            }
        }
        Ok(_) => {}
        Err(e) => eprintln!("[sync] background tick failed: {e}"),
    }
}

fn notify_new_messages(app_handle: &AppHandle) {
    if let Err(e) = app_handle.notification().builder().title(NOTIFICATION_TITLE).body(NOTIFICATION_BODY).show() {
        eprintln!("[sync] failed to show notification: {e}");
    }
}
