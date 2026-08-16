//! Key-publication command: exposes `SealedChainClient::publish_keys_if_stale`
//! so the active session's locally-derived keys get synced on-chain.
//! Mirrors `KeyManagementNotifier.ensurePublished()` in
//! `providers/keys_provider.dart` — call this opportunistically (after
//! unlock, after a redeem), not just once at account creation, since a
//! fresh account has no `w:` box yet and publishing only becomes possible
//! once one exists.

use tauri::State;

use crate::state::AppState;

/// Best-effort: publish this session's keys on-chain if they're missing or
/// stale relative to what's already published. Returns `true` if a publish
/// txn was actually submitted, `false` if it was a no-op (already
/// up-to-date, no box yet, or no credits).
#[tauri::command]
pub async fn ensure_keys_published(state: State<'_, AppState>) -> Result<bool, String> {
    ensure_keys_published_impl(&state).await
}

pub(crate) async fn ensure_keys_published_impl(state: &AppState) -> Result<bool, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let result = session
        .chain_client
        .publish_keys_if_stale(
            &session.wallet,
            &session.escrow,
            &session.sealed_keys.encryption_pubkey,
            &session.sealed_keys.scan_pubkey,
            &session.sealed_keys.pq_public_key,
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(result.is_some())
}
