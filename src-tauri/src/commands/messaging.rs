//! Messaging commands: send/sync/query. Thin async wrappers over
//! `messaging.rs`'s domain logic and `messages.rs`'s repository — mirrors
//! `MessageService`'s public surface (`sendMessage`, `syncMessages`,
//! `forceResync`, `getConversation`, `getAllConversations`,
//! `markConversationAsRead`, `getUnreadCount`) plus `deleteConversation`
//! from `MessageRepository`.
//!
//! Every command that both touches the database *and* awaits a network
//! call locks `state.session` **twice**: once to gather owned data and
//! narrow, `Sync`-safe references (chain client, wallet, etc.) before the
//! `.await`, and again afterward to write the result. See
//! `messaging::SendMessageOutcome`'s doc comment for why a single
//! `&Session`/`&Db`/`&Connection` binding can never survive an `.await` —
//! those types embed a `RefCell`-based statement cache, which makes them
//! `!Sync`, which in turn means `#[tauri::command]`'s `Send`-future
//! requirement fails if a single such binding is read on both sides of an
//! await, no matter how far apart. Two separate lock scopes side-step
//! this by never letting one `&Session` binding span the await at all.

use tauri::State;

use crate::messages::{self, ConversationPreview, DecryptedMessage};
use crate::messaging::{self, SendMessageRequest};
use crate::state::AppState;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecryptedMessageDto {
    pub id: String,
    pub sender_wallet: String,
    pub sender_username: Option<String>,
    pub recipient_wallet: String,
    pub recipient_username: Option<String>,
    pub content: String,
    pub timestamp: i64,
    pub is_outgoing: bool,
    pub on_chain_pubkey: String,
}

impl From<&DecryptedMessage> for DecryptedMessageDto {
    fn from(m: &DecryptedMessage) -> Self {
        Self {
            id: m.id.clone(),
            sender_wallet: m.sender_wallet.clone(),
            sender_username: m.sender_username.clone(),
            recipient_wallet: m.recipient_wallet.clone(),
            recipient_username: m.recipient_username.clone(),
            content: m.content.clone(),
            timestamp: m.timestamp,
            is_outgoing: m.is_outgoing,
            on_chain_pubkey: m.on_chain_pubkey.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPreviewDto {
    pub contact_wallet: String,
    pub contact_username: Option<String>,
    pub last_message_preview: String,
    pub last_message_timestamp: i64,
    pub is_last_message_outgoing: bool,
    pub unread_count: i64,
    pub message_count: i64,
    pub is_blocked: bool,
}

impl From<&ConversationPreview> for ConversationPreviewDto {
    fn from(p: &ConversationPreview) -> Self {
        Self {
            contact_wallet: p.contact_wallet.clone(),
            contact_username: p.contact_username.clone(),
            last_message_preview: p.last_message_preview.clone(),
            last_message_timestamp: p.last_message_timestamp,
            is_last_message_outgoing: p.is_last_message_outgoing,
            unread_count: p.unread_count,
            message_count: p.message_count,
            is_blocked: p.is_blocked,
        }
    }
}

/// The sender's own display name is read straight from the seeded
/// `user_profile` row rather than passed in from the frontend — mirrors
/// `userService.displayName` being sourced from local user state, not a
/// caller-supplied argument. `pub(crate)`: also used by `commands::alias`'s
/// `send_alias_message` — mobile's alias sends embed the real sender's
/// wallet/username in the JSON payload exactly like regular DMs do.
pub(crate) fn sender_username(conn: &rusqlite::Connection) -> Option<String> {
    conn.query_row("SELECT username FROM user_profile LIMIT 1", [], |row| row.get(0)).ok().flatten()
}

#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    recipient_wallet: String,
    recipient_username: Option<String>,
    plaintext: String,
) -> Result<String, String> {
    let (cached, sender_wallet, sender_username) = {
        let session_guard = state.session.lock().await;
        let session = session_guard.as_ref().ok_or("not unlocked")?;
        let cached = crate::contacts::get_contact_keys(session.db.connection(), &recipient_wallet).map_err(|e| e.to_string())?;
        (cached, session.wallet.address.clone(), sender_username(session.db.connection()))
    };

    // Network/crypto phase — no live db reference held across this await;
    // `state.session` is locked again immediately below, as a *fresh*
    // borrow that never coexists with the borrow above across the await.
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let request = SendMessageRequest {
        sender_wallet: &sender_wallet,
        sender_username: sender_username.as_deref(),
        recipient_wallet: &recipient_wallet,
        recipient_username: recipient_username.as_deref(),
        plaintext: &plaintext,
    };
    let outcome = messaging::send_message_network(
        &session.chain_client,
        &session.indexer_client,
        &session.wallet,
        &session.escrow,
        &session.sealed_keys,
        request,
        cached,
    )
    .await
    .map_err(|e| e.to_string())?;
    drop(session_guard);

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    messaging::apply_send_result(session.db.connection(), &recipient_wallet, &outcome).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;
    Ok(outcome.tx_id)
}

#[tauri::command]
pub async fn sync_messages(state: State<'_, AppState>, full_sync: bool) -> Result<i64, String> {
    let since_timestamp = {
        let session_guard = state.session.lock().await;
        let session = session_guard.as_ref().ok_or("not unlocked")?;
        messaging::compute_since_timestamp(session.db.connection(), full_sync).map_err(|e| e.to_string())?
    };

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let fetch = messaging::fetch_sync_data(&session.chain_client, &session.wallet, since_timestamp).await.map_err(|e| e.to_string())?;
    drop(session_guard);

    // `as_mut()`/`connection_mut()`: `apply_sync_result` now needs `&mut
    // Connection` (Phase 7h) — completing an alias invite that arrived via
    // regular DM, inside the same sync pass, requires a transaction
    // (`alias::contacts::promote_creator_pending_to_contact`).
    let mut session_guard = state.session.lock().await;
    let session = session_guard.as_mut().ok_or("not unlocked")?;
    let count = messaging::apply_sync_result(session.db.connection_mut(), &session.wallet, &session.sealed_keys, &fetch).map_err(|e| e.to_string())?;
    let alias_count =
        crate::alias::messaging::apply_alias_sync_result(session.db.connection(), fetch.incoming_candidates()).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;
    Ok(count + alias_count)
}

/// **Bug fixed 2026-08-10**: `prepare_force_resync` (a destructive
/// `messages::clear()`) used to run in its own lock scope *before* the
/// `fetch_sync_data` `.await` below — meaning any failure during that fetch
/// (network hiccup, OHTTP relay error) left the in-memory message cache
/// wiped with nothing to replace it, for the rest of the running app
/// session (an early `Err` return skips `session.db.save()`, so the
/// on-disk copy survives — but the user sees an empty chat list until they
/// restart the app, with no indication why). This got materially more
/// likely once `query_indexer_transactions` gained full pagination (up to
/// 100 sequential indexer requests instead of one), which a user hit
/// live. Fixed by moving the clear to *after* the fetch has fully
/// succeeded — nothing is ever cleared unless the fresh data to replace it
/// is already in hand.
#[tauri::command]
pub async fn force_resync(state: State<'_, AppState>) -> Result<i64, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let fetch = messaging::fetch_sync_data(&session.chain_client, &session.wallet, Some(0)).await.map_err(|e| e.to_string())?;
    drop(session_guard);

    let mut session_guard = state.session.lock().await;
    let session = session_guard.as_mut().ok_or("not unlocked")?;
    messaging::prepare_force_resync(session.db.connection()).map_err(|e| e.to_string())?;
    let count =
        messaging::finalize_force_resync(session.db.connection_mut(), &session.wallet, &session.sealed_keys, &fetch).map_err(|e| e.to_string())?;
    let alias_count =
        crate::alias::messaging::apply_alias_sync_result(session.db.connection(), fetch.incoming_candidates()).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;
    Ok(count + alias_count)
}

#[tauri::command]
pub async fn get_conversation(state: State<'_, AppState>, contact_wallet: String) -> Result<Vec<DecryptedMessageDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    messages::get_conversation_messages(session.db.connection(), &session.wallet.address, &contact_wallet)
        .map(|rows| rows.iter().map(DecryptedMessageDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_all_conversations(state: State<'_, AppState>) -> Result<Vec<ConversationPreviewDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    messages::get_conversations(session.db.connection())
        .map(|rows| rows.iter().map(ConversationPreviewDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_conversation_as_read(state: State<'_, AppState>, contact_wallet: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    messages::mark_as_read(session.db.connection(), &contact_wallet).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_unread_count(state: State<'_, AppState>, contact_wallet: String) -> Result<i64, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    messages::get_unread_count(session.db.connection(), &contact_wallet).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_conversation(state: State<'_, AppState>, contact_wallet: String) -> Result<i64, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let count = messages::delete_conversation(session.db.connection(), &contact_wallet).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;
    Ok(count)
}
