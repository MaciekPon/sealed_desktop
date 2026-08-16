//! Username commands: claim/release/resolve/availability/search. Mirrors
//! `sealed_username_ops.dart` (claim/release/resolve) +
//! `IndexerClient.checkUsernameAvailable`/`searchUsers`.
//!
//! Format validation (`crate::username::validate_username`) runs
//! client-side before submitting, same as the contract's own checks —
//! this only saves a round-trip on obviously-invalid input; the contract
//! still enforces the real rules on-chain (mapped back to
//! `ChainError::BadUsernameFormat` by `chain::client`).

use tauri::State;

use crate::state::AppState;
use crate::username::validate_username;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsernameSearchHit {
    pub username: String,
    pub wallet_address: String,
}

/// Also mirrors the freshly-claimed name into the local `user_profile` row
/// — `commands::messaging::send_message`'s `sender_username()` helper reads
/// straight from there to embed our own name in outgoing messages, and
/// nothing else in this codebase ever wrote to that column after the
/// account-creation seed row (bug found 2026-08-07: desktop-sent messages
/// always carried `sender_username: null`, even after a successful claim).
/// Two lock scopes, not one, since the on-chain call must not hold the
/// session guard across its `.await` while this DB write touches it again
/// afterward — see `commands::messaging::send_message`'s doc comment.
#[tauri::command]
pub async fn claim_username(state: State<'_, AppState>, name: String, old_name: Option<String>) -> Result<String, String> {
    let normalized = name.trim().to_lowercase();
    validate_username(&normalized).map_err(|e| e.to_string())?;

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let tx_id = session
        .chain_client
        .claim_username(&session.wallet, &session.escrow, &normalized, old_name.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    drop(session_guard);

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    session.db.connection().execute("UPDATE user_profile SET username = ?1", rusqlite::params![normalized]).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;
    Ok(tx_id)
}

/// See `claim_username`'s doc comment — same local-mirror fix, clearing the
/// column back to `NULL` on release.
#[tauri::command]
pub async fn release_username(state: State<'_, AppState>, old_name: Option<String>) -> Result<String, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let tx_id = session
        .chain_client
        .release_username(&session.wallet, &session.escrow, old_name.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    drop(session_guard);

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    session.db.connection().execute("UPDATE user_profile SET username = NULL", []).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;
    Ok(tx_id)
}

/// Reverse lookup: name -> owner wallet address.
#[tauri::command]
pub async fn resolve_username(state: State<'_, AppState>, name: String) -> Result<String, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let profile = session.chain_client.get_user_by_username(&name).await.map_err(|e| e.to_string())?;
    profile.map(|p| p.wallet_address).ok_or_else(|| "user not found".to_string())
}

#[tauri::command]
pub async fn check_username_available(state: State<'_, AppState>, name: String) -> Result<bool, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    session.indexer_client.check_username_available(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_usernames(state: State<'_, AppState>, query: String, limit: u32) -> Result<Vec<UsernameSearchHit>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let result = session.indexer_client.search_users(&query, limit).await.map_err(|e| e.to_string())?;
    Ok(result
        .users
        .into_iter()
        .map(|h| UsernameSearchHit { username: h.username, wallet_address: h.wallet_address })
        .collect())
}
