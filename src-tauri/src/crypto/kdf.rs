//! HKDF-SHA256 and HMAC-SHA256 helpers, ported from the KDF-related parts of
//! `crypto_service.dart`. All domain-separation info strings are kept
//! byte-identical to the Dart source.

use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

const HYBRID_INFO: &[u8] = b"sealed-hybrid-aes-gcm-v1";
const CLASSICAL_INFO: &[u8] = b"sealed-aes-gcm-v1";

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
    fn hybrid_key_differs_with_and_without_pq() {
        let classical = [1u8; 32];
        let pq = [2u8; 32];
        let a = derive_hybrid_key(&classical, None);
        let b = derive_hybrid_key(&classical, Some(&pq));
        assert_ne!(a, b);
    }

}
