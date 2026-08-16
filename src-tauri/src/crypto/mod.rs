//! Core cryptographic operations, ported from
//! `sealed_app/lib/services/crypto_service.dart`.
//!
//! AES-256-GCM encryption/decryption, X25519 key exchange, message padding,
//! and post-quantum key encapsulation (ML-KEM-512 / Kyber512). All wire
//! formats and domain-separation labels are kept byte-identical to the
//! mobile client so the two can interoperate over the same chain contract.

pub mod aead;
pub mod kdf;
pub mod pq;
pub mod x25519;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("validation error: {0}")]
    Validation(String),
    #[error("operation failed: {0}")]
    Operation(String),
}

pub type CryptoResult<T> = Result<T, CryptoError>;

/// HMAC label shared by both the legacy per-message recipient tag
/// (`computeRecipientTag` / `isMessageForMe`) and the alias-chat symmetric
/// variant (`computeAliasRecipientTag` / `checkRecipientTag`).
const RECIPIENT_TAG_LABEL: &[u8] = b"sealed-recipient-tag-v1";

/// Constant-time byte comparison. Mirrors `CryptoService.constantTimeEquals`.
pub fn constant_time_equals(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// HMAC-SHA256(sharedSecret, "sealed-recipient-tag-v1"). Shared by
/// `computeRecipientTag` and `computeAliasRecipientTag` in the Dart source,
/// which are byte-for-byte identical — kept as one function here.
pub fn recipient_tag_from_secret(shared_secret: &[u8]) -> [u8; 32] {
    kdf::hmac_sha256(shared_secret, RECIPIENT_TAG_LABEL)
}

/// Verify a message's recipient tag using the classical X25519 shared
/// secret derived from our view key and the sender's ephemeral encryption
/// pubkey. Mirrors `CryptoService.isMessageForMe`.
pub fn is_message_for_me(
    view_private_key: &[u8; 32],
    sender_encryption_pubkey: &[u8; 32],
    recipient_tag: &[u8; 32],
) -> bool {
    let shared = x25519::shared_secret_from_seed(view_private_key, sender_encryption_pubkey);
    let expected = recipient_tag_from_secret(&shared);
    constant_time_equals(&expected, recipient_tag)
}

/// Same construction as [`is_message_for_me`], kept as a distinctly-named
/// entry point mirroring `CryptoService.checkRecipientTag` (identical body,
/// different caller-facing name in the Dart source; never fails, returns
/// `false` on any internal error like the original).
pub fn check_recipient_tag(
    sender_encryption_pubkey: &[u8; 32],
    recipient_tag: &[u8; 32],
    my_scan_private_key: &[u8; 32],
) -> bool {
    is_message_for_me(my_scan_private_key, sender_encryption_pubkey, recipient_tag)
}

/// Encrypt with hybrid key derivation (X25519 + optional PQ shared secret).
/// Mirrors `CryptoService.encryptHybrid`. Caller supplies the already-computed
/// classical X25519 shared secret (via [`x25519::shared_secret_from_seed`] or
/// an ephemeral secret's `diffie_hellman`), since Rust's X25519 secret types
/// aren't uniformly reusable the way the Dart `SimpleKeyPair` abstraction is.
pub fn encrypt_hybrid(
    plaintext: &[u8],
    classical_shared_secret: &[u8; 32],
    pq_shared_secret: Option<&[u8]>,
) -> CryptoResult<Vec<u8>> {
    if plaintext.is_empty() {
        return Err(CryptoError::Validation("plainTextBytes cannot be empty".into()));
    }
    let aes_key = kdf::derive_hybrid_key(classical_shared_secret, pq_shared_secret);
    aead::encrypt_combined(&aes_key, plaintext)
}

/// Decrypt with hybrid key derivation. Mirrors `CryptoService.decryptHybrid`.
pub fn decrypt_hybrid(
    ciphertext: &[u8],
    classical_shared_secret: &[u8; 32],
    pq_shared_secret: Option<&[u8]>,
) -> CryptoResult<Vec<u8>> {
    if ciphertext.len() < 28 {
        return Err(CryptoError::Validation(format!(
            "cipherText too short: {} bytes",
            ciphertext.len()
        )));
    }
    let aes_key = kdf::derive_hybrid_key(classical_shared_secret, pq_shared_secret);
    aead::decrypt_combined(&aes_key, ciphertext)
}

/// Encrypt using an alias-channel `enc_key` directly (already the AES key —
/// no per-message ECDH, the key exchange already provided forward secrecy).
/// Mirrors `CryptoService.encryptWithEncKey`.
pub fn encrypt_with_enc_key(enc_key: &[u8; 32], plaintext: &[u8]) -> CryptoResult<Vec<u8>> {
    if plaintext.is_empty() {
        return Err(CryptoError::Validation("plainTextBytes cannot be empty".into()));
    }
    aead::encrypt_combined(enc_key, plaintext)
}

/// Mirrors `CryptoService.decryptWithEncKey`.
pub fn decrypt_with_enc_key(enc_key: &[u8; 32], ciphertext: &[u8]) -> CryptoResult<Vec<u8>> {
    if ciphertext.len() < 28 {
        return Err(CryptoError::Validation(format!(
            "cipherText too short: {} bytes",
            ciphertext.len()
        )));
    }
    aead::decrypt_combined(enc_key, ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_equals_matches_ordinary_eq() {
        assert!(constant_time_equals(b"abc", b"abc"));
        assert!(!constant_time_equals(b"abc", b"abd"));
        assert!(!constant_time_equals(b"abc", b"ab"));
    }

    #[test]
    fn hybrid_encrypt_decrypt_round_trip() {
        let alice = x25519::generate_ephemeral_keypair();
        let bob_seed = [7u8; 32];
        let bob_public = x25519::public_key_from_seed(&bob_seed);

        let alice_shared = *alice
            .secret
            .diffie_hellman(&crate::crypto::x25519::public_key_from_bytes(&bob_public))
            .as_bytes();
        let bob_shared = x25519::shared_secret_from_seed(&bob_seed, &alice.public);
        assert_eq!(alice_shared, bob_shared);

        let plaintext = b"quantum resistant message";
        let ct = encrypt_hybrid(plaintext, &alice_shared, None).unwrap();
        let pt = decrypt_hybrid(&ct, &bob_shared, None).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn hybrid_encrypt_decrypt_with_pq_round_trip() {
        let classical = [1u8; 32];
        let pq = [2u8; 32];
        let plaintext = b"hybrid pq message";
        let ct = encrypt_hybrid(plaintext, &classical, Some(&pq)).unwrap();
        let pt = decrypt_hybrid(&ct, &classical, Some(&pq)).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn enc_key_round_trip() {
        let key = [9u8; 32];
        let plaintext = b"alias chat message";
        let ct = encrypt_with_enc_key(&key, plaintext).unwrap();
        let pt = decrypt_with_enc_key(&key, &ct).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn is_message_for_me_round_trip() {
        let view_seed = [3u8; 32];
        let view_public = x25519::public_key_from_seed(&view_seed);

        let sender = x25519::generate_ephemeral_keypair();
        let shared = sender
            .secret
            .diffie_hellman(&x25519::public_key_from_bytes(&view_public));
        let tag = recipient_tag_from_secret(shared.as_bytes());

        assert!(is_message_for_me(&view_seed, &sender.public, &tag));
        let wrong_tag = [0u8; 32];
        assert!(!is_message_for_me(&view_seed, &sender.public, &wrong_tag));
    }
}
