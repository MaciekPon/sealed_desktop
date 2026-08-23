//! On-chain bio command — set or clear the caller's public profile bio.
//! Mirrors `UserService.setBio` in `user_service.dart`.
//!
//! Unlike username, there's no local mirror column to keep in sync here —
//! nothing embeds the caller's own bio into outgoing messages the way
//! `commands::messaging::send_message` embeds the cached username. Reads go
//! through the same lazy chain-resolve path as any other wallet's bio
//! (`commands::contacts::resolve_contact_keys`), keyed on the caller's own
//! wallet address — exactly how the Settings screen already reads its own
//! username via `useResolvedUsername`.

use tauri::State;

use crate::bio::validate_bio;
use crate::state::AppState;

/// Two lock scopes, not one: the session guard must never be held across
/// an `.await` while it's also touched again afterward — `Session` holds
/// the Stronghold `Vault`, which is `!Sync`, so a `&Session` held live
/// across a second, later `.await` poisons this command's future as
/// `!Send` (confirmed live 2026-08-23). One scope for the write itself,
/// one for blocking on confirmation.
#[tauri::command]
pub async fn set_bio(state: State<'_, AppState>, bio: String) -> Result<String, String> {
    let trimmed = bio.trim().to_string();
    validate_bio(&trimmed).map_err(|e| e.to_string())?;

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let tx_id = session
        .chain_client
        .set_bio(&session.wallet, &session.escrow, &trimmed)
        .await
        .map_err(|e| e.to_string())?;
    drop(session_guard);

    // Block on confirmation — mirrors `UserService.setBio` in
    // `user_service.dart`. Without this, the frontend's immediate
    // post-save re-resolve (`useResolvedBio`, cached with `staleTime:
    // Infinity`) can race the write and permanently cache the pre-edit
    // bio. See `SealedChainClient::wait_for_confirmation`'s doc comment.
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    session.chain_client.wait_for_confirmation(&tx_id).await.map_err(|e| e.to_string())?;
    Ok(tx_id)
}
