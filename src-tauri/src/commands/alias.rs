//! Alias-chat commands: local QR/paste handshake (create/accept/complete
//! invite) plus send/query/manage over established alias contacts. Mirrors
//! the thin `_impl`-wrapper pattern established in `commands/contacts.rs`/
//! `commands/messaging.rs`.
//!
//! The handshake commands touch zero network I/O (`alias::onboarding`'s
//! functions are plain, non-async) — a single session-lock scope is safe,
//! no `Send`-future hazard to split around. Only `send_alias_message`
//! awaits the chain, so it alone needs the multi-lock-scope shape
//! `commands::messaging::send_message` established.

use tauri::State;

use crate::alias::{self, contacts as alias_contacts, envelope, incoming_invites, messages as alias_messages, messaging as alias_messaging, onboarding};
use crate::state::AppState;

fn b64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).map_err(|e| e.to_string())
}

fn now_unix_seconds() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteDto {
    pub invite_ref: String,
    pub envelope_base64: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInviteDto {
    pub invite_ref: String,
    pub label: Option<String>,
    pub peer_wallet: Option<String>,
    pub created_at: i64,
    pub dismissed: bool,
}

impl From<&alias_contacts::PendingInvite> for PendingInviteDto {
    fn from(p: &alias_contacts::PendingInvite) -> Self {
        Self { invite_ref: p.invite_ref.clone(), label: p.label.clone(), peer_wallet: p.peer_wallet.clone(), created_at: p.created_at, dismissed: p.dismissed }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasContactDto {
    pub contact_id: String,
    pub label: Option<String>,
    pub is_creator: bool,
    pub peer_wallet: Option<String>,
    pub created_at: i64,
    pub established_at: i64,
}

impl From<&alias_contacts::AliasContact> for AliasContactDto {
    fn from(c: &alias_contacts::AliasContact) -> Self {
        Self {
            contact_id: c.contact_id.clone(),
            label: c.label.clone(),
            is_creator: c.is_creator,
            peer_wallet: c.peer_wallet.clone(),
            created_at: c.created_at,
            established_at: c.established_at,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingInviteDto {
    pub invite_ref: String,
    pub peer_wallet: String,
    pub peer_username: Option<String>,
    pub received_at: i64,
}

impl From<&incoming_invites::IncomingInvite> for IncomingInviteDto {
    fn from(i: &incoming_invites::IncomingInvite) -> Self {
        Self { invite_ref: i.invite_ref.clone(), peer_wallet: i.peer_wallet.clone(), peer_username: i.peer_username.clone(), received_at: i.received_at }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptInviteResultDto {
    pub contact: AliasContactDto,
    pub reply_envelope_base64: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasMessageDto {
    pub id: String,
    pub contact_id: String,
    pub content: String,
    pub timestamp: i64,
    pub is_outgoing: bool,
}

impl From<&alias_messages::AliasMessage> for AliasMessageDto {
    fn from(m: &alias_messages::AliasMessage) -> Self {
        Self { id: m.id.clone(), contact_id: m.contact_id.clone(), content: m.content.clone(), timestamp: m.timestamp, is_outgoing: m.is_outgoing }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasConversationPreviewDto {
    pub contact_id: String,
    pub label: Option<String>,
    pub last_message_preview: String,
    pub last_message_timestamp: i64,
    pub is_last_message_outgoing: bool,
    pub unread_count: i64,
    pub message_count: i64,
}

impl From<&alias_messages::AliasConversationPreview> for AliasConversationPreviewDto {
    fn from(p: &alias_messages::AliasConversationPreview) -> Self {
        Self {
            contact_id: p.contact_id.clone(),
            label: p.label.clone(),
            last_message_preview: p.last_message_preview.clone(),
            last_message_timestamp: p.last_message_timestamp,
            is_last_message_outgoing: p.is_last_message_outgoing,
            unread_count: p.unread_count,
            message_count: p.message_count,
        }
    }
}

#[tauri::command]
pub async fn create_invite(state: State<'_, AppState>, label: Option<String>) -> Result<InviteDto, String> {
    let created = onboarding::create_invitation_envelope().map_err(|e| e.to_string())?;
    let invite_ref_hex = envelope::hex_encode(&created.invite_ref);
    let now = now_unix_seconds();

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    // `peer_wallet: None` — the QR/paste flow doesn't know who will accept
    // until they scan/paste back; contrast with `create_invite_for_contact`
    // below, which knows the recipient up front.
    alias_contacts::save_pending_invite(session.db.connection(), &invite_ref_hex, label.as_deref(), None, &created, now).map_err(|e| e.to_string())?;
    // Wrap for the QR/paste wire format a phone's scanner expects
    // (`combined_qr_codec.dart` + `offline_handshake_codec.dart`'s
    // single-frame case) — `classifyScan` rejects a bare envelope, it needs
    // both the `0x03` combined-QR container and the `0xA1` frame header,
    // even for a payload that fits one frame. Used for both the rendered
    // QR and the copyable text — one wire format either way.
    let combined = envelope::encode_combined_qr(&session.wallet.address, &created.envelope_bytes).ok_or("wallet address too long to encode")?;
    let wire = envelope::encode_offline_frame(&combined);
    session.db.save(&session.dek).map_err(|e| e.to_string())?;

    Ok(InviteDto { invite_ref: invite_ref_hex, envelope_base64: b64_encode(&wire), created_at: now })
}

#[tauri::command]
pub async fn list_pending_invites(state: State<'_, AppState>) -> Result<Vec<PendingInviteDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_contacts::get_all_pending_invites(session.db.connection())
        .map(|rows| rows.iter().map(PendingInviteDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dismiss_pending_invite(state: State<'_, AppState>, invite_ref: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_contacts::dismiss_pending_invite(session.db.connection(), &invite_ref).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_pending_invite(state: State<'_, AppState>, invite_ref: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_contacts::delete_pending_invite(session.db.connection(), &invite_ref).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn accept_invite(state: State<'_, AppState>, envelope_base64: String, label: Option<String>) -> Result<AcceptInviteResultDto, String> {
    let wire_bytes = b64_decode(&envelope_base64)?;
    let combined = envelope::decode_offline_frame(&wire_bytes)
        .ok_or("not a single-frame invite QR/paste (multi-frame scans aren't supported on desktop)")?;
    let (peer_wallet_address, invite_bytes) = envelope::decode_combined_qr(&combined).ok_or("malformed invite QR payload")?;
    let accepted = onboarding::accept_invitation_from_envelope(&invite_bytes).map_err(|e| e.to_string())?;
    let now = now_unix_seconds();

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    // The combined-QR container's embedded wallet address (there specifically
    // so the same QR can double as a "start a normal chat" fallback) doubles
    // as `peer_wallet` local metadata here too, same as `create_invite_for_contact`'s.
    alias_contacts::insert_accepted_contact(session.db.connection(), &accepted, label.as_deref(), Some(&peer_wallet_address), now).map_err(|e| e.to_string())?;
    let contact =
        alias_contacts::get_alias_contact(session.db.connection(), &accepted.contact_id).map_err(|e| e.to_string())?.ok_or("contact vanished immediately after insert")?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;

    // Reply also needs the `0xA1` single-frame wrapper (no `0x03` combined
    // layer here — an accept reply isn't a "start a normal chat" fallback,
    // just the raw accept envelope) for the creator's scanner/paste to
    // recognize it.
    let wire_reply = envelope::encode_offline_frame(&accepted.accept_envelope_bytes);
    Ok(AcceptInviteResultDto { contact: AliasContactDto::from(&contact), reply_envelope_base64: b64_encode(&wire_reply) })
}

#[tauri::command]
pub async fn complete_invite(state: State<'_, AppState>, envelope_base64: String) -> Result<AliasContactDto, String> {
    let wire_bytes = b64_decode(&envelope_base64)?;
    let bytes = envelope::decode_offline_frame(&wire_bytes)
        .ok_or("not a single-frame reply QR/paste (multi-frame scans aren't supported on desktop)")?;
    let accept = envelope::decode_accept_envelope(&bytes).ok_or("malformed accept envelope")?;
    let now = now_unix_seconds();

    let mut session_guard = state.session.lock().await;
    let session = session_guard.as_mut().ok_or("not unlocked")?;

    let pending = alias_contacts::get_all_pending_invites(session.db.connection())
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| envelope::hex_decode(&p.invite_ref).is_some_and(|r| r.len() >= 8 && r[..8] == accept.invite_ref_prefix[..]))
        .ok_or_else(|| alias::AliasError::NoMatchingPendingInvite.to_string())?;

    let completed = onboarding::complete_from_accept_envelope(&pending.invite_ref, &pending.my_pq_sk, &bytes).map_err(|e| e.to_string())?;
    let invite_ref = pending.invite_ref.clone();
    alias_contacts::promote_creator_pending_to_contact(session.db.connection_mut(), &pending, &completed, now).map_err(|e| e.to_string())?;
    let contact =
        alias_contacts::get_alias_contact(session.db.connection(), &invite_ref).map_err(|e| e.to_string())?.ok_or("contact vanished immediately after promotion")?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;

    Ok(AliasContactDto::from(&contact))
}

#[tauri::command]
pub async fn send_alias_message(state: State<'_, AppState>, contact_id: String, plaintext: String) -> Result<String, String> {
    let (contact, sender_username) = {
        let session_guard = state.session.lock().await;
        let session = session_guard.as_ref().ok_or("not unlocked")?;
        let contact =
            alias_contacts::get_alias_contact(session.db.connection(), &contact_id).map_err(|e| e.to_string())?.ok_or("unknown alias contact")?;
        (contact, crate::commands::messaging::sender_username(session.db.connection()))
    };

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let outcome = alias_messaging::send_alias_message_network(
        &session.chain_client,
        &session.wallet,
        &session.escrow,
        sender_username.as_deref(),
        &contact,
        &plaintext,
    )
    .await
    .map_err(|e| e.to_string())?;
    drop(session_guard);

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_messaging::apply_alias_send_result(session.db.connection(), &outcome).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;
    Ok(outcome.tx_id)
}

#[tauri::command]
pub async fn get_alias_contacts(state: State<'_, AppState>) -> Result<Vec<AliasContactDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_contacts::get_all_alias_contacts(session.db.connection())
        .map(|rows| rows.iter().map(AliasContactDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_alias_conversations(state: State<'_, AppState>) -> Result<Vec<AliasConversationPreviewDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_messages::get_alias_conversations(session.db.connection())
        .map(|rows| rows.iter().map(AliasConversationPreviewDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_alias_conversation(state: State<'_, AppState>, contact_id: String) -> Result<Vec<AliasMessageDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_messages::get_alias_conversation(session.db.connection(), &contact_id)
        .map(|rows| rows.iter().map(AliasMessageDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mark_alias_conversation_read(state: State<'_, AppState>, contact_id: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_messages::mark_alias_conversation_read(session.db.connection(), &contact_id).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_alias_contact(state: State<'_, AppState>, contact_id: String, label: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_contacts::rename_alias_contact(session.db.connection(), &contact_id, &label).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_alias_contact(state: State<'_, AppState>, contact_id: String) -> Result<(), String> {
    let mut session_guard = state.session.lock().await;
    let session = session_guard.as_mut().ok_or("not unlocked")?;
    alias_contacts::delete_alias_contact(session.db.connection_mut(), &contact_id).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Contact-initiated alias chat (Phase 7h) — delivers the invite/accept
// envelope as a regular DM to an already-known wallet contact, instead of
// only via QR/paste. See `messaging::send_raw_bytes_network`'s doc comment
// for the wire-level reasoning (no JSON/gzip, no self-copy, no hybrid-frame
// shortcut, always wrapped via an empty-self-copy `combine_ciphertexts`).
// ---------------------------------------------------------------------------

/// Creates an invite and delivers it directly to `recipient_wallet` over
/// the regular messaging channel — no QR/paste round trip. Guards against
/// an accidental duplicate: refuses if a non-dismissed pending invite or an
/// established alias contact already exists for this wallet.
#[tauri::command]
pub async fn create_invite_for_contact(state: State<'_, AppState>, recipient_wallet: String, label: Option<String>) -> Result<PendingInviteDto, String> {
    let cached = {
        let session_guard = state.session.lock().await;
        let session = session_guard.as_ref().ok_or("not unlocked")?;
        if alias_contacts::find_pending_invite_for_wallet(session.db.connection(), &recipient_wallet).map_err(|e| e.to_string())?.is_some() {
            return Err("an alias invite to this contact is already pending".to_string());
        }
        if alias_contacts::find_alias_contact_for_wallet(session.db.connection(), &recipient_wallet).map_err(|e| e.to_string())?.is_some() {
            return Err("an alias chat with this contact already exists".to_string());
        }
        crate::contacts::get_contact_keys(session.db.connection(), &recipient_wallet).map_err(|e| e.to_string())?
    };

    let created = onboarding::create_invitation_envelope().map_err(|e| e.to_string())?;
    let invite_ref_hex = envelope::hex_encode(&created.invite_ref);

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let outcome = crate::messaging::send_raw_bytes_network(
        &session.chain_client,
        &session.indexer_client,
        &session.wallet,
        &session.escrow,
        &session.sealed_keys,
        &recipient_wallet,
        &created.envelope_bytes,
        cached,
    )
    .await
    .map_err(|e| e.to_string())?;
    drop(session_guard);

    let now = now_unix_seconds();
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_contacts::save_pending_invite(session.db.connection(), &invite_ref_hex, label.as_deref(), Some(&recipient_wallet), &created, now)
        .map_err(|e| e.to_string())?;
    crate::contacts::save_contact_keys(session.db.connection(), &recipient_wallet, &outcome.contact_keys_update).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;

    let pending = alias_contacts::get_pending_invite(session.db.connection(), &invite_ref_hex)
        .map_err(|e| e.to_string())?
        .ok_or("pending invite vanished immediately after insert")?;
    Ok(PendingInviteDto::from(&pending))
}

/// Invites still awaiting my accept/decline decision, delivered via a
/// regular DM (never via the synchronous QR/paste `accept_invite` path,
/// which has no intermediate pending state on the acceptor side).
#[tauri::command]
pub async fn list_incoming_invites(state: State<'_, AppState>) -> Result<Vec<IncomingInviteDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    incoming_invites::list_incoming_invites(session.db.connection(), "pending")
        .map(|rows| rows.iter().map(IncomingInviteDto::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn accept_incoming_invite(state: State<'_, AppState>, invite_ref: String, label: Option<String>) -> Result<AliasContactDto, String> {
    let (incoming, cached) = {
        let session_guard = state.session.lock().await;
        let session = session_guard.as_ref().ok_or("not unlocked")?;
        let incoming = incoming_invites::get_incoming_invite(session.db.connection(), &invite_ref).map_err(|e| e.to_string())?.ok_or("unknown incoming invite")?;
        let cached = crate::contacts::get_contact_keys(session.db.connection(), &incoming.peer_wallet).map_err(|e| e.to_string())?;
        (incoming, cached)
    };

    let accepted = onboarding::accept_invitation_from_envelope(&incoming.envelope_bytes).map_err(|e| e.to_string())?;

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let outcome = crate::messaging::send_raw_bytes_network(
        &session.chain_client,
        &session.indexer_client,
        &session.wallet,
        &session.escrow,
        &session.sealed_keys,
        &incoming.peer_wallet,
        &accepted.accept_envelope_bytes,
        cached,
    )
    .await
    .map_err(|e| e.to_string())?;
    drop(session_guard);

    let now = now_unix_seconds();
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    alias_contacts::insert_accepted_contact(session.db.connection(), &accepted, label.as_deref(), Some(&incoming.peer_wallet), now).map_err(|e| e.to_string())?;
    incoming_invites::set_incoming_invite_status(session.db.connection(), &invite_ref, "accepted").map_err(|e| e.to_string())?;
    crate::contacts::save_contact_keys(session.db.connection(), &incoming.peer_wallet, &outcome.contact_keys_update).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())?;

    let contact = alias_contacts::get_alias_contact(session.db.connection(), &accepted.contact_id)
        .map_err(|e| e.to_string())?
        .ok_or("contact vanished immediately after insert")?;
    Ok(AliasContactDto::from(&contact))
}

/// Declines a still-pending incoming invite. Local-only — no reply is sent
/// to the sender (a deliberate, privacy-preserving default: the sender
/// simply never hears back, same as ignoring an unsolicited message).
#[tauri::command]
pub async fn decline_incoming_invite(state: State<'_, AppState>, invite_ref: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    incoming_invites::set_incoming_invite_status(session.db.connection(), &invite_ref, "declined").map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}
