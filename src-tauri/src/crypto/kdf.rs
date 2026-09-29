//! HKDF-SHA256 and HMAC-SHA256 helpers, ported from the KDF-related parts of
//! `crypto_service.dart`. All domain-separation info strings are kept
//! byte-identical to the Dart source.

use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

const HYBRID_INFO: &[u8] = b"sealed-hybrid-aes-gcm-v1";
const CLASSICAL_INFO: &[u8] = b"sealed-aes-gcm-v1";
const KEM_ENCAPS_NONCE_INFO_PREFIX: &[u8] = b"sealed-kem-encaps-v1:";

/// HMAC-SHA256(key, label) — used for recipient-tag derivation.
pub fn hmac_sha256(key: &[u8], label: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(label);
    let result = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Full HKDF-Extract-then-Expand (RFC 5869) into an arbitrary-length output.
fn hkdf_expand_into(salt: Option<&[u8]>, ikm: &[u8], info: &[u8], out: &mut [u8]) {
    let hk = Hkdf::<Sha256>::new(salt, ikm);
    hk.expand(info, out)
        .expect("requested HKDF output length must be <= 255 * 32 bytes");
}

fn hkdf_expand32(salt: Option<&[u8]>, ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    hkdf_expand_into(salt, ikm, info, &mut out);
    out
}

/// Generic HKDF-Expand (empty salt) into a 32-byte output. Used by
/// `KeyService`-equivalent derivations (encryption/view key seeds) where
/// the info string is the only thing distinguishing the two.
pub fn derive_seed32(ikm: &[u8], info: &[u8]) -> [u8; 32] {
    hkdf_expand32(None, ikm, info)
}

/// Generic HKDF-Expand (empty salt) into a 64-byte output — used for the
/// ML-KEM keygen seed (`sealed-pq-kem-v1`).
pub fn derive_seed64(ikm: &[u8], info: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    hkdf_expand_into(None, ikm, info, &mut out);
    out
}

/// Deterministic 32-byte ML-KEM encapsulation nonce ("coins") for a
/// handshake to a peer holding `peer_pq_pubkey`, derived from the account's
/// PQ master seed (itself derived from the mnemonic — see
/// `keys::derive_sealed_keys`) plus the peer's PQ pubkey hash. Mirrors
/// `KeyService.deriveKemEncapsNonce` in `key_service.dart` exactly (same
/// HKDF-SHA256, same info-string construction).
///
/// **Why this exists**: if this account initiated a chat (encapsulated to
/// the peer's PQ pubkey), the resulting `pq_shared_secret` is cached
/// locally — but a "Log out & delete your data" then restore-from-mnemonic
/// wipes that cache. Any later reply from that peer, encrypted with the
/// secret from OUR original encapsulation, would otherwise be permanently
/// undecryptable. Since this nonce is deterministic and keyed only on
/// mnemonic-derived material + the peer's (public) PQ pubkey, re-running
/// the exact same encapsulation after a restore reproduces the identical
/// ciphertext and shared secret, recovering the ability to decrypt. Keyed
/// on the peer's pubkey (not wallet address) so a peer rotating their PQ
/// key naturally yields fresh coins — never nonce reuse against a
/// different `ek`. See `messaging.rs::sync_incoming_messages`'s decrypt-
/// failure fallback for the only call site: the caller MUST verify the
/// retried decrypt's AEAD tag before trusting/caching the result — a
/// peer-initiated chat (we never encapsulated) or a genuinely rotated peer
/// PQ key both yield a well-formed but non-matching secret here, which is
/// exactly what the AEAD check is for.
pub fn derive_kem_encaps_nonce(pq_master_seed: &[u8; 64], peer_pq_pubkey: &[u8]) -> [u8; 32] {
    let peer_hash = Sha256::digest(peer_pq_pubkey);
    let mut info = Vec::with_capacity(KEM_ENCAPS_NONCE_INFO_PREFIX.len() + peer_hash.len());
    info.extend_from_slice(KEM_ENCAPS_NONCE_INFO_PREFIX);
    info.extend_from_slice(&peer_hash);
    derive_seed32(pq_master_seed, &info)
}

/// Derive the hybrid (or classical-only) AES key from X25519 + optional PQ
/// shared secret material. Mirrors `CryptoService.deriveHybridKey`.
pub fn derive_hybrid_key(classical_shared_secret: &[u8; 32], pq_shared_secret: Option<&[u8]>) -> [u8; 32] {
    match pq_shared_secret {
        Some(pq) => {
            let mut combined = Vec::with_capacity(32 + pq.len());
            combined.extend_from_slice(classical_shared_secret);
            combined.extend_from_slice(pq);
            hkdf_expand32(None, &combined, HYBRID_INFO)
        }
        None => hkdf_expand32(None, classical_shared_secret, CLASSICAL_INFO),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kem_encaps_nonce_is_deterministic_and_sensitive_to_the_peer_key() {
        let master_seed = [11u8; 64];
        let peer_a = [1u8; 800];
        let peer_b = [2u8; 800];

        let a1 = derive_kem_encaps_nonce(&master_seed, &peer_a);
        let a2 = derive_kem_encaps_nonce(&master_seed, &peer_a);
        assert_eq!(a1, a2, "same master seed + peer pubkey must reproduce the same nonce");

        let b = derive_kem_encaps_nonce(&master_seed, &peer_b);
        assert_ne!(a1, b, "a different peer pubkey must yield different coins");

        let other_master = derive_kem_encaps_nonce(&[22u8; 64], &peer_a);
        assert_ne!(a1, other_master, "a different master seed must yield different coins");
    }

    #[test]
    fn hybrid_key_differs_with_and_without_pq() {
        let classical = [1u8; 32];
        let pq = [2u8; 32];
        let a = derive_hybrid_key(&classical, None);
        let b = derive_hybrid_key(&classical, Some(&pq));
        assert_ne!(a, b);
    }

}
