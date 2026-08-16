//! Dispatch layer for alias-chat envelopes delivered as a regular DM (Phase
//! 7h), as opposed to the QR/paste flow. Keeps `messaging.rs`'s
//! `sync_incoming_messages` alias-agnostic in spirit — it only calls
//! [`classify`] and, on a match, one of the two `handle_*` functions here,
//! rather than embedding alias-specific DB logic directly.
//!
//! Mirrors mobile's `message_sync.dart` "Alias onboarding envelope router"
//! (`:628-655`): an incoming message's decrypted-but-not-yet-gzip-decompressed
//! plaintext is classified by exact length + a leading version byte
//! (`0x01` = invite, `0x02` = accept) *before* falling through to normal
//! gzip+JSON message parsing. Normal text DMs always gzip-compress their
//! JSON payload first (gzip's magic bytes are `0x1f 0x8b`), so there is no
//! collision with `0x01`/`0x02`.

use rusqlite::Connection;

use super::{contacts, envelope, incoming_invites, onboarding, AliasError};

pub enum IncomingAliasEnvelope {
    None,
    Invite,
    Accept,
}

/// Exact-length + leading-version-byte match against an already
/// hybrid-decrypted, pre-gzip plaintext. Reuses [`envelope::decode_invite_envelope`]/
/// [`envelope::decode_accept_envelope`]'s own length/version checks rather
/// than duplicating the magic numbers here.
pub fn classify(decrypted: &[u8]) -> IncomingAliasEnvelope {
    if envelope::decode_invite_envelope(decrypted).is_some() {
        IncomingAliasEnvelope::Invite
    } else if envelope::decode_accept_envelope(decrypted).is_some() {
        IncomingAliasEnvelope::Accept
    } else {
        IncomingAliasEnvelope::None
    }
}

/// Receiver side: an invite arrived passively via background sync. Records
/// it as a durable "awaiting my decision" row — mirrors mobile's
/// `AliasOnboardingService.recordIncomingInvite` — rather than
/// auto-accepting; the user explicitly Accepts/Declines later via
/// `commands::alias::accept_incoming_invite`/`decline_incoming_invite`.
/// Sync-only (no `.await` anywhere in this call chain), idempotent via
/// [`incoming_invites::record_incoming_invite`]'s `INSERT OR IGNORE`.
///
/// Returns `true` iff a *new* incoming-invite row was actually recorded
/// (as opposed to a re-delivered duplicate) — see [`handle_incoming_accept`]'s
/// doc comment for why callers need this instead of always treating the
/// call as "something changed".
pub fn handle_incoming_invite(conn: &Connection, sender_wallet: &str, envelope_bytes: &[u8], received_at: i64) -> Result<bool, AliasError> {
    let invite = envelope::decode_invite_envelope(envelope_bytes).ok_or(AliasError::MalformedInviteEnvelope)?;
    let invite_ref_hex = envelope::hex_encode(&envelope::invite_ref(&invite.enc_pub, &invite.scan_pub, &invite.pq_pub));
    // Best-effort: the sender is already a known regular-DM contact (that's
    // how this envelope reached them in the first place), so their cached
    // username is usually available without a live lookup.
    let peer_username = crate::contacts::get_contact_keys(conn, sender_wallet)?.username;
    let inserted = incoming_invites::record_incoming_invite(conn, &invite_ref_hex, sender_wallet, peer_username.as_deref(), envelope_bytes, received_at)?;
    Ok(inserted)
}

/// Creator side: an accept reply arrived for an invite this account
/// created. Matches against this account's own pending invites by
/// `invite_ref_prefix` and auto-completes immediately — no user decision
/// needed here, since the user already decided when they sent the invite.
/// A no-op (not an error) if no matching pending invite is found (stray or
/// duplicate delivery, or an accept for an invite this account never
/// created). Needs `&mut Connection` because
/// [`contacts::promote_creator_pending_to_contact`] is transactional.
///
/// **Bug found and fixed 2026-08-11**: alias invite/accept envelopes never
/// get recorded in the `messages` table (deliberately — they're not chat
/// messages), so `sync_incoming_messages`'s `has_message` dedup check never
/// catches them: the *same* accept transaction gets re-fetched and
/// re-processed on every single sync pass, forever, for the lifetime of
/// the account (confirmed live via `log_sync_diagnostic` output — one
/// transaction reprocessed dozens of times over a few minutes). Once the
/// matching pending invite is consumed on the first successful match, every
/// later reprocessing attempt correctly falls into the "no match" branch
/// (harmless — the promotion isn't repeated), but it's wasted work and log
/// noise that scales with the account's total historical alias-handshake
/// traffic. Fixed by checking `alias_contacts` for a prefix match *first*:
/// if the contact was already established from this exact accept, skip
/// silently without even attempting `complete_from_accept_envelope`/
/// `promote_creator_pending_to_contact` again.
///
/// **Second bug found and fixed 2026-08-11**: this used to return `Result<()>`,
/// so `sync_incoming_messages` had no way to tell "matched and promoted a
/// contact" apart from "no-op, nothing to do" — both looked identical to
/// the caller. That mattered because `sync_incoming_messages`'s `new_count`
/// (returned all the way up through `sync_messages`/`force_resync` to the
/// frontend, and used by the background tick to decide whether to emit
/// `messages-updated` at all) never incremented for alias envelope
/// processing in the first place (a `continue` skips straight past
/// `new_count += 1`, which only regular text messages reach) — so a sync
/// pass that silently completed an alias handshake always reported
/// `newCount === 0`, which meant both the frontend's manual "Sync now" flow
/// *and* the background tick's `messages-updated` event (gated on
/// `new_count > 0`) never found out anything had changed. Returning `true`
/// here lets the caller count this as real, meaningful sync activity.
pub fn handle_incoming_accept(conn: &mut Connection, accept_bytes: &[u8], now: i64) -> Result<bool, AliasError> {
    let accept = envelope::decode_accept_envelope(accept_bytes).ok_or(AliasError::MalformedAcceptEnvelope)?;

    let already_established = contacts::get_all_alias_contacts(conn)?
        .iter()
        .any(|c| c.contact_id.get(..16).is_some_and(|prefix_hex| prefix_hex == envelope::hex_encode(&accept.invite_ref_prefix)));
    if already_established {
        return Ok(false);
    }

    let all_pending = contacts::get_all_pending_invites(conn)?;
    let pending = all_pending
        .iter()
        .find(|p| envelope::hex_decode(&p.invite_ref).is_some_and(|r| r.len() >= 8 && r[..8] == accept.invite_ref_prefix[..]));
    let Some(pending) = pending else {
        // Temporary diagnostic (2026-08-11) — see `messaging::log_sync_diagnostic`'s
        // doc comment. Prints the received prefix against every locally-held
        // pending invite's prefix, since a mismatch here (rather than a
        // decrypt/classify failure earlier) is the leading suspect for a
        // live "accept never completes" report.
        let received_prefix_hex = envelope::hex_encode(&accept.invite_ref_prefix);
        let local_prefixes: Vec<String> = all_pending.iter().map(|p| p.invite_ref.get(..16).unwrap_or(&p.invite_ref).to_string()).collect();
        crate::messaging::log_sync_diagnostic(&format!(
            "handle_incoming_accept: no matching pending invite for received prefix {received_prefix_hex} — {} pending invite(s) held locally with ref-prefixes {local_prefixes:?}",
            all_pending.len()
        ));
        return Ok(false);
    };

    let completed = onboarding::complete_from_accept_envelope(&pending.invite_ref, &pending.my_pq_sk, accept_bytes)?;
    // Temporary diagnostic (2026-08-11) — see `messaging::log_sync_diagnostic`'s
    // doc comment. Fingerprints the *creator's* freshly-decapsulated
    // `pq_shared_secret` at the moment the handshake completes, so a later
    // `apply_alias_sync_result` log line (fingerprinting the same contact's
    // stored `pq_shared_secret` right before a failed message decrypt) can
    // confirm whether the value is even stable across that gap, before
    // assuming the mismatch is cross-platform (Rust vs Dart) rather than a
    // local storage/read bug.
    crate::messaging::log_sync_diagnostic(&format!(
        "handle_incoming_accept: handshake completed for contact {}, pq_shared_secret fingerprint={}",
        pending.invite_ref,
        crate::messaging::hex_fingerprint(&completed.pq_shared_secret)
    ));
    contacts::promote_creator_pending_to_contact(conn, pending, &completed, now)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alias::onboarding as alias_onboarding;
    use crate::db::Db;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-alias-invite-delivery-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&dir.join("test.db"), &[7u8; 32]).unwrap()
    }

    #[test]
    fn classify_recognizes_invite_and_accept_and_rejects_normal_text() {
        let created = alias_onboarding::create_invitation_envelope().unwrap();
        assert!(matches!(classify(&created.envelope_bytes), IncomingAliasEnvelope::Invite));

        let accepted = alias_onboarding::accept_invitation_from_envelope(&created.envelope_bytes).unwrap();
        assert!(matches!(classify(&accepted.accept_envelope_bytes), IncomingAliasEnvelope::Accept));

        // Gzip-compressed JSON (what a normal text DM's decrypted plaintext
        // always looks like) must never classify as an alias envelope.
        let gzip_like = crate::messaging::gzip_compress(br#"{"content":"hi"}"#).unwrap();
        assert!(matches!(classify(&gzip_like), IncomingAliasEnvelope::None));
        assert!(matches!(classify(b"short"), IncomingAliasEnvelope::None));
    }

    #[test]
    fn handle_incoming_invite_records_a_pending_row_idempotently() {
        let db = temp_db("invite");
        let conn = db.connection();
        let created = alias_onboarding::create_invitation_envelope().unwrap();

        assert!(handle_incoming_invite(conn, "WALLETA", &created.envelope_bytes, 1000).unwrap());
        assert!(!handle_incoming_invite(conn, "WALLETA", &created.envelope_bytes, 1000).unwrap()); // re-delivered, must not duplicate

        let invite_ref_hex = envelope::hex_encode(&created.invite_ref);
        let rows = incoming_invites::list_incoming_invites(conn, "pending").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].invite_ref, invite_ref_hex);
        assert_eq!(rows[0].peer_wallet, "WALLETA");
    }

    #[test]
    fn handle_incoming_accept_completes_a_matching_pending_invite() {
        let mut db = temp_db("accept");
        let created = alias_onboarding::create_invitation_envelope().unwrap();
        let invite_ref_hex = envelope::hex_encode(&created.invite_ref);
        contacts::save_pending_invite(db.connection(), &invite_ref_hex, None, Some("WALLETB"), &created, 1000).unwrap();

        let accepted = alias_onboarding::accept_invitation_from_envelope(&created.envelope_bytes).unwrap();
        assert!(handle_incoming_accept(db.connection_mut(), &accepted.accept_envelope_bytes, 2000).unwrap());

        assert!(contacts::get_pending_invite(db.connection(), &invite_ref_hex).unwrap().is_none());
        let contact = contacts::get_alias_contact(db.connection(), &invite_ref_hex).unwrap().unwrap();
        assert!(contact.is_creator);
        assert_eq!(contact.peer_wallet.as_deref(), Some("WALLETB"));

        // Re-delivered/re-synced copy of the same accept transaction — must
        // report `false` (nothing new) and must not error, matching the
        // "already established" short-circuit's doc comment.
        assert!(!handle_incoming_accept(db.connection_mut(), &accepted.accept_envelope_bytes, 3000).unwrap());
    }

    #[test]
    fn handle_incoming_accept_is_a_noop_when_no_pending_invite_matches() {
        let mut db = temp_db("no-match");
        let created = alias_onboarding::create_invitation_envelope().unwrap();
        let accepted = alias_onboarding::accept_invitation_from_envelope(&created.envelope_bytes).unwrap();

        // No pending invite was ever saved — must not error, must report `false`.
        assert!(!handle_incoming_accept(db.connection_mut(), &accepted.accept_envelope_bytes, 2000).unwrap());
    }
}
