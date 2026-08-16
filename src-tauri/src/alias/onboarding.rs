//! Pure alias-chat handshake logic, ported from
//! `alias_onboarding_service.dart`'s envelope-based flow ("Flow A" in the
//! plan — the only one reachable from the live Dart UI; the older
//! on-chain-discovery flow in the same Dart file is dead, not ported).
//!
//! No DB, no network — every function here takes and returns plain values,
//! so the full three-step handshake is testable in-memory (see the tests
//! module). Callers (`commands::alias`) own persistence and the invite-ref
//! matching search.

use super::envelope::{self, AcceptEnvelope, InviteEnvelope};
use super::AliasError;
use crate::crypto::{pq, x25519};

pub struct CreatedInvite {
    pub invite_ref: [u8; 32],
    pub envelope_bytes: Vec<u8>,
    pub my_enc_seed: [u8; 32],
    pub my_enc_pub: [u8; 32],
    pub my_scan_seed: [u8; 32],
    pub my_scan_pub: [u8; 32],
    pub my_pq_pub: Vec<u8>,
    pub my_pq_sk: Vec<u8>,
}

/// Creator side, step 1: mint a fresh invite. Generates two throwaway
/// X25519 identities (enc + scan roles) and one ML-KEM-512 keypair — the
/// creator is the only side that ever holds a PQ secret key (see the
/// module doc on [`accept_invitation_from_envelope`]).
pub fn create_invitation_envelope() -> Result<CreatedInvite, AliasError> {
    let my_enc_seed = x25519::random_seed();
    let my_enc_pub = x25519::public_key_from_seed(&my_enc_seed);
    let my_scan_seed = x25519::random_seed();
    let my_scan_pub = x25519::public_key_from_seed(&my_scan_seed);
    let pq_keys = pq::generate_pq_keypair()?;

    let invite_ref = envelope::invite_ref(&my_enc_pub, &my_scan_pub, &pq_keys.public_key);
    let envelope_bytes = envelope::encode_invite_envelope(&InviteEnvelope {
        enc_pub: my_enc_pub,
        scan_pub: my_scan_pub,
        pq_pub: pq_keys.public_key.to_vec(),
    });

    Ok(CreatedInvite {
        invite_ref,
        envelope_bytes,
        my_enc_seed,
        my_enc_pub,
        my_scan_seed,
        my_scan_pub,
        my_pq_pub: pq_keys.public_key.to_vec(),
        my_pq_sk: pq_keys.private_key.to_vec(),
    })
}

pub struct AcceptedInvite {
    pub contact_id: String,
    pub accept_envelope_bytes: Vec<u8>,
    pub peer_enc_pub: [u8; 32],
    pub peer_scan_pub: [u8; 32],
    /// The creator's ML-KEM public key, as received in the invite envelope.
    /// Not used by the live send/receive path (that reuses the raw
    /// `pq_shared_secret` directly) — persisted for schema parity with the
    /// Dart `AliasContact` model.
    pub peer_pq_pub: Vec<u8>,
    pub my_enc_seed: [u8; 32],
    pub my_enc_pub: [u8; 32],
    pub my_scan_seed: [u8; 32],
    pub my_scan_pub: [u8; 32],
    pub pq_shared_secret: [u8; 32],
}

/// Acceptor side, step 2: consume a scanned/pasted invite envelope and
/// complete the handshake in one step (no pending state on this side —
/// only the creator waits). The acceptor never generates a PQ keypair; it
/// only encapsulates against the creator's public key, matching the live
/// Dart flow's intentionally one-directional PQ design.
pub fn accept_invitation_from_envelope(invite_envelope_bytes: &[u8]) -> Result<AcceptedInvite, AliasError> {
    let invite = envelope::decode_invite_envelope(invite_envelope_bytes).ok_or(AliasError::MalformedInviteEnvelope)?;
    let invite_ref = envelope::invite_ref(&invite.enc_pub, &invite.scan_pub, &invite.pq_pub);

    let my_enc_seed = x25519::random_seed();
    let my_enc_pub = x25519::public_key_from_seed(&my_enc_seed);
    let my_scan_seed = x25519::random_seed();
    let my_scan_pub = x25519::public_key_from_seed(&my_scan_seed);

    let kem = pq::kem_encapsulate(&invite.pq_pub)?;

    let mut invite_ref_prefix = [0u8; 8];
    invite_ref_prefix.copy_from_slice(&invite_ref[..8]);
    let accept_envelope_bytes = envelope::encode_accept_envelope(&AcceptEnvelope {
        invite_ref_prefix,
        enc_pub: my_enc_pub,
        scan_pub: my_scan_pub,
        kem_ciphertext: kem.ciphertext,
    });

    Ok(AcceptedInvite {
        contact_id: envelope::hex_encode(&invite_ref),
        accept_envelope_bytes,
        peer_enc_pub: invite.enc_pub,
        peer_scan_pub: invite.scan_pub,
        peer_pq_pub: invite.pq_pub,
        my_enc_seed,
        my_enc_pub,
        my_scan_seed,
        my_scan_pub,
        pq_shared_secret: kem.shared_secret,
    })
}

pub struct CompletedInvite {
    pub peer_enc_pub: [u8; 32],
    pub peer_scan_pub: [u8; 32],
    pub pq_shared_secret: [u8; 32],
}

/// Creator side, step 3: finish the handshake once the acceptor's reply
/// comes back. `pending_invite_ref_hex`/`my_pq_sk` are the matching pending
/// row's fields — the caller (`commands::alias::complete_invite`) is
/// responsible for the linear scan over pending invites that finds the
/// right one by comparing `invite_ref_prefix`; this function re-verifies
/// the match defensively rather than trusting the caller blindly.
pub fn complete_from_accept_envelope(
    pending_invite_ref_hex: &str,
    my_pq_sk: &[u8],
    accept_envelope_bytes: &[u8],
) -> Result<CompletedInvite, AliasError> {
    let accept = envelope::decode_accept_envelope(accept_envelope_bytes).ok_or(AliasError::MalformedAcceptEnvelope)?;

    let pending_ref_bytes = envelope::hex_decode(pending_invite_ref_hex).ok_or(AliasError::NoMatchingPendingInvite)?;
    if pending_ref_bytes.len() < 8 || pending_ref_bytes[..8] != accept.invite_ref_prefix[..] {
        return Err(AliasError::NoMatchingPendingInvite);
    }

    let pq_shared_secret = pq::kem_decapsulate(&accept.kem_ciphertext, my_pq_sk)?;

    Ok(CompletedInvite {
        peer_enc_pub: accept.enc_pub,
        peer_scan_pub: accept.scan_pub,
        pq_shared_secret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing test for this whole phase: run the full three-step
    /// handshake in memory and confirm both sides land on an identical
    /// `pq_shared_secret` and `invite_ref`/`contact_id`.
    #[test]
    fn full_handshake_round_trip_derives_identical_shared_secret() {
        let created = create_invitation_envelope().unwrap();
        let accepted = accept_invitation_from_envelope(&created.envelope_bytes).unwrap();

        assert_eq!(accepted.contact_id, envelope::hex_encode(&created.invite_ref));
        assert_eq!(accepted.peer_enc_pub, created.my_enc_pub);
        assert_eq!(accepted.peer_scan_pub, created.my_scan_pub);

        let creator_invite_ref_hex = envelope::hex_encode(&created.invite_ref);
        let completed =
            complete_from_accept_envelope(&creator_invite_ref_hex, &created.my_pq_sk, &accepted.accept_envelope_bytes).unwrap();

        assert_eq!(completed.pq_shared_secret, accepted.pq_shared_secret);
        assert_eq!(completed.peer_enc_pub, accepted.my_enc_pub);
        assert_eq!(completed.peer_scan_pub, accepted.my_scan_pub);
    }

    #[test]
    fn accept_rejects_malformed_invite_envelope() {
        assert!(accept_invitation_from_envelope(&[0u8; 10]).is_err());
    }

    #[test]
    fn complete_rejects_mismatched_invite_ref_prefix() {
        let created = create_invitation_envelope().unwrap();
        let accepted = accept_invitation_from_envelope(&created.envelope_bytes).unwrap();

        let wrong_ref_hex = envelope::hex_encode(&[0xffu8; 32]);
        let err = complete_from_accept_envelope(&wrong_ref_hex, &created.my_pq_sk, &accepted.accept_envelope_bytes).unwrap_err();
        assert!(matches!(err, AliasError::NoMatchingPendingInvite));
    }

    #[test]
    fn complete_rejects_malformed_accept_envelope() {
        let created = create_invitation_envelope().unwrap();
        let creator_invite_ref_hex = envelope::hex_encode(&created.invite_ref);
        assert!(complete_from_accept_envelope(&creator_invite_ref_hex, &created.my_pq_sk, &[0u8; 10]).is_err());
    }
}
