//! Alias-chat send/receive network + sync glue, parallel to the top-level
//! `messaging.rs` for wallet DMs. Reuses the exact same `sendMessage` chain
//! call and the same recipient-tag/hybrid-encryption primitives as normal
//! DMs — the only difference is which identity (alias contact vs. wallet)
//! supplies the keys.
//!
//! **Wire format bug found and fixed 2026-08-12** (root-caused via a live,
//! multi-hour cross-device debugging session — see the plan/memory entry
//! for the full trail): this module used to assume alias messages have a
//! simpler wire shape than regular DMs (`[8B timestamp][content]`, no JSON,
//! no `combine_ciphertexts` framing — "the recipient already knows who
//! this is from `contact_id` context"). That assumption was never actually
//! checked against the live Dart source and was **wrong on both counts**:
//! reading `sealed_app/lib/features/messaging/message_sender.dart` shows
//! `sendAliasMessage` routes through the exact same `_send()` helper
//! regular DMs use — full `MessagePayload` JSON envelope (with the
//! *real* sender wallet/username embedded, and `recipient_wallet`/
//! `recipient_username` set to the alias `contactId`/label instead of a
//! real wallet), gzip, `encrypt_hybrid`, then **always** wrapped via
//! `combineCiphertexts(ciphertext, emptyBytes)` before padding — same as a
//! self-copy-less regular DM, never a bare ciphertext. The invite/accept
//! envelope path (`sendMessageBytes`/`_sendRawBytes`) does skip JSON+gzip
//! (that assumption was correct, and matches `messaging::send_raw_bytes_network`)
//! but *still* wraps via `combineCiphertexts` — that part of the framing is
//! never skipped anywhere in the live protocol. Missing the combine/split
//! wrapping alone was enough to break every alias message in both
//! directions (2 stray length-prefix bytes prepended to what
//! `decrypt_hybrid` sees reliably fails the AES-GCM auth-tag check) — this
//! is what live-testing surfaced first, and looked exactly like a PQ
//! key/crypto mismatch investigation for a while, since recipient-tag
//! matching uses a different keypair (`my_scan_seed`) than message
//! decryption (`my_enc_seed`) and kept succeeding.
use rusqlite::Connection;

use super::contacts::AliasContact;
use super::messages::AliasMessage;
use super::AliasError;
use crate::chain::client::{ChainMessage, SealedChainClient};
use crate::chain::escrow::TreasuryEscrowSigner;
use crate::chain::wallet::AlgorandWallet;
use crate::messaging::MessagePayload;

fn now_unix_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub struct SendAliasMessageOutcome {
    pub tx_id: String,
    pub message: AliasMessage,
}

/// `sender_username`: the account's own claimed username, if any — mirrors
/// `userService.displayName` being embedded in the JSON payload for alias
/// sends exactly like regular DMs (see this module's doc comment; alias
/// messages are *not* anonymous about who's sending them, only about which
/// on-chain identity is used to route them).
pub async fn send_alias_message_network(
    chain_client: &SealedChainClient,
    wallet: &AlgorandWallet,
    escrow: &TreasuryEscrowSigner,
    sender_username: Option<&str>,
    contact: &AliasContact,
    plaintext: &str,
) -> Result<SendAliasMessageOutcome, AliasError> {
    let credits = chain_client.get_credits(wallet, &wallet.address).await?;
    if credits < 1 {
        return Err(AliasError::NoCredits);
    }

    let timestamp_ms = now_unix_millis();
    let payload = MessagePayload {
        sender_wallet: wallet.address.clone(),
        sender_username: sender_username.map(str::to_string),
        recipient_wallet: contact.contact_id.clone(),
        recipient_username: contact.label.clone(),
        content: plaintext.to_string(),
        timestamp: timestamp_ms,
    };
    let payload_bytes = serde_json::to_vec(&payload).map_err(crate::messaging::MessagingError::from)?;
    let compressed = crate::messaging::gzip_compress(&payload_bytes)?;

    let ephemeral = crate::crypto::x25519::generate_reusable_keypair();
    let shared_for_tag = ephemeral.secret.diffie_hellman(&crate::crypto::x25519::public_key_from_bytes(&contact.peer_scan_pub));
    let recipient_tag = crate::crypto::recipient_tag_from_secret(shared_for_tag.as_bytes());

    let shared_for_encrypt = ephemeral.secret.diffie_hellman(&crate::crypto::x25519::public_key_from_bytes(&contact.peer_enc_pub));
    let ciphertext =
        crate::crypto::encrypt_hybrid(&compressed, shared_for_encrypt.as_bytes(), Some(contact.pq_shared_secret.as_slice()))?;
    // Always combine-wrapped, even with an empty self-copy — see this
    // module's doc comment. Never send a bare `pad_message(ciphertext)`.
    let combined = crate::messaging::combine_ciphertexts(&ciphertext, &[]);
    let padded = crate::messaging::pad_message(&combined)?;

    let tx_id = chain_client.send_message(wallet, escrow, &recipient_tag, &ephemeral.public, &padded).await?;
    crate::messaging::log_sync_diagnostic(&format!(
        "send_alias_message_network: sent to contact {} as tx {tx_id} ({} plaintext byte(s))",
        contact.contact_id,
        plaintext.len()
    ));

    Ok(SendAliasMessageOutcome {
        tx_id: tx_id.clone(),
        message: AliasMessage {
            id: tx_id.clone(),
            contact_id: contact.contact_id.clone(),
            content: plaintext.to_string(),
            timestamp: timestamp_ms / 1000,
            is_outgoing: true,
        },
    })
}

pub fn apply_alias_send_result(conn: &Connection, outcome: &SendAliasMessageOutcome) -> Result<(), AliasError> {
    super::messages::save_alias_message(conn, &outcome.message)?;
    Ok(())
}

/// Sync-only half — no `.await` anywhere. Scans the same
/// `Vec<ChainMessage>` that `messaging::fetch_sync_data`'s
/// `incoming_candidates` already fetched for wallet-DM sync, trial-matching
/// each against every alias contact's scan key. Returns the number of
/// newly-saved alias messages.
pub fn apply_alias_sync_result(conn: &Connection, candidates: &[ChainMessage]) -> Result<i64, AliasError> {
    let contacts = super::contacts::get_all_alias_contacts(conn)?;
    // Temporary diagnostic (2026-08-11) — see `messaging::log_sync_diagnostic`'s
    // doc comment. This function had zero logging until now; added while
    // live-debugging a report that alias-chat *messages* (as opposed to the
    // invite/accept handshake, already diagnosed separately) never arrive
    // in either direction between two established alias-chat parties.
    crate::messaging::log_sync_diagnostic(&format!(
        "apply_alias_sync_result: {} alias contact(s) held locally, scanning {} candidate(s)",
        contacts.len(),
        candidates.len()
    ));
    if contacts.is_empty() {
        return Ok(0);
    }

    let mut saved = 0i64;
    for msg in candidates {
        if super::messages::has_alias_message(conn, &msg.account_pubkey)? {
            continue;
        }
        let mut tag_matched_any_contact = false;
        for contact in &contacts {
            if !crate::crypto::check_recipient_tag(&msg.sender_encryption_pubkey, &msg.recipient_tag, &contact.my_scan_seed) {
                continue;
            }
            tag_matched_any_contact = true;
            crate::messaging::log_sync_diagnostic(&format!(
                "apply_alias_sync_result: tag matched contact {} for candidate from {} (tx {}), stored pq_shared_secret fingerprint={}",
                contact.contact_id,
                msg.sender_address,
                msg.account_pubkey,
                crate::messaging::hex_fingerprint(&contact.pq_shared_secret)
            ));

            let Some(combined) = crate::messaging::unpad_message(&msg.ciphertext) else {
                crate::messaging::log_sync_diagnostic("apply_alias_sync_result: unpad_message failed after tag match");
                break;
            };
            // Always combine-wrapped on the wire (see this module's doc
            // comment) — split before decrypting, discard the empty self-ct.
            let Some((ciphertext, _self_ct)) = crate::messaging::split_ciphertexts(&combined) else {
                crate::messaging::log_sync_diagnostic("apply_alias_sync_result: split_ciphertexts failed after unpad");
                break;
            };
            let shared = crate::crypto::x25519::shared_secret_from_seed(&contact.my_enc_seed, &msg.sender_encryption_pubkey);
            let Ok(compressed) = crate::crypto::decrypt_hybrid(&ciphertext, &shared, Some(contact.pq_shared_secret.as_slice())) else {
                crate::messaging::log_sync_diagnostic(
                    "apply_alias_sync_result: decrypt_hybrid failed after tag match + correct split_ciphertexts framing",
                );
                break;
            };
            let Ok(raw) = crate::messaging::gzip_decompress(&compressed) else {
                crate::messaging::log_sync_diagnostic("apply_alias_sync_result: gzip_decompress failed after successful decrypt");
                break;
            };
            let Ok(payload) = serde_json::from_slice::<MessagePayload>(&raw) else {
                crate::messaging::log_sync_diagnostic("apply_alias_sync_result: MessagePayload JSON parse failed after successful decompress");
                break;
            };

            super::messages::save_alias_message(
                conn,
                &AliasMessage {
                    id: msg.account_pubkey.clone(),
                    contact_id: contact.contact_id.clone(),
                    content: payload.content,
                    timestamp: payload.timestamp / 1000,
                    is_outgoing: false,
                },
            )?;
            crate::messaging::log_sync_diagnostic("apply_alias_sync_result: alias message saved successfully");
            saved += 1;
            break; // tag matched this contact — no need to try the rest
        }
        if !tag_matched_any_contact && !msg.sender_address.is_empty() {
            crate::messaging::log_sync_diagnostic(&format!(
                "apply_alias_sync_result: candidate from {} (tx {}) matched no alias contact's tag",
                msg.sender_address, msg.account_pubkey
            ));
        }
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_wire_round_trip_with_fixed_keys() {
        use crate::crypto::{decrypt_hybrid, encrypt_hybrid, recipient_tag_from_secret, x25519};

        // Two independent identities, mirroring a completed handshake:
        // "sender" holds the peer's (receiver's) published enc/scan pubkeys;
        // "receiver" holds its own enc/scan seeds plus the raw PQ secret.
        let receiver_enc_seed = x25519::random_seed();
        let receiver_enc_pub = x25519::public_key_from_seed(&receiver_enc_seed);
        let receiver_scan_seed = x25519::random_seed();
        let receiver_scan_pub = x25519::public_key_from_seed(&receiver_scan_seed);
        let pq_shared_secret = [42u8; 32];

        let payload = MessagePayload {
            sender_wallet: "SENDERWALLET".to_string(),
            sender_username: Some("alice".to_string()),
            recipient_wallet: "contact-id-hex".to_string(),
            recipient_username: Some("Bob's alias".to_string()),
            content: "reused wire format test".to_string(),
            timestamp: 1_700_000_000_000,
        };
        let payload_bytes = serde_json::to_vec(&payload).unwrap();
        let compressed = crate::messaging::gzip_compress(&payload_bytes).unwrap();

        let ephemeral = x25519::generate_reusable_keypair();
        let shared_for_tag = ephemeral.secret.diffie_hellman(&x25519::public_key_from_bytes(&receiver_scan_pub));
        let recipient_tag = recipient_tag_from_secret(shared_for_tag.as_bytes());
        let shared_for_encrypt = ephemeral.secret.diffie_hellman(&x25519::public_key_from_bytes(&receiver_enc_pub));
        let ciphertext = encrypt_hybrid(&compressed, shared_for_encrypt.as_bytes(), Some(&pq_shared_secret)).unwrap();
        let combined = crate::messaging::combine_ciphertexts(&ciphertext, &[]);
        let padded = crate::messaging::pad_message(&combined).unwrap();

        // Receiver side.
        assert!(crate::crypto::check_recipient_tag(&ephemeral.public, &recipient_tag, &receiver_scan_seed));
        let unpadded = crate::messaging::unpad_message(&padded).unwrap();
        let (recovered_ciphertext, self_ct) = crate::messaging::split_ciphertexts(&unpadded).unwrap();
        assert!(self_ct.is_empty());
        let shared = x25519::shared_secret_from_seed(&receiver_enc_seed, &ephemeral.public);
        let decompressed = decrypt_hybrid(&recovered_ciphertext, &shared, Some(&pq_shared_secret)).unwrap();
        let raw = crate::messaging::gzip_decompress(&decompressed).unwrap();
        let decoded: MessagePayload = serde_json::from_slice(&raw).unwrap();

        assert_eq!(decoded.timestamp, payload.timestamp);
        assert_eq!(decoded.content, payload.content);
        assert_eq!(decoded.sender_wallet, payload.sender_wallet);
        assert_eq!(decoded.recipient_username, payload.recipient_username);
    }
}
