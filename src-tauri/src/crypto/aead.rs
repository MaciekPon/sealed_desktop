//! AES-256-GCM helpers using the combined wire layout used throughout
//! `crypto_service.dart`: `nonce(12) || ciphertext || tag(16)`.

use aes_gcm::aead::{Aead, Generate, KeyInit, Nonce};
use aes_gcm::{Aes256Gcm, Key};

use super::{CryptoError, CryptoResult};

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

/// AES-256-GCM encrypt. Returns `nonce(12) || ciphertext || tag(16)`.
pub fn encrypt_combined(key: &[u8; 32], plaintext: &[u8]) -> CryptoResult<Vec<u8>> {
    let cipher_key = Key::<Aes256Gcm>::try_from(key.as_slice())
        .expect("32-byte key matches Aes256Gcm::KeySize");
    let cipher = Aes256Gcm::new(&cipher_key);
    let nonce = Nonce::<Aes256Gcm>::generate();
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_| CryptoError::Operation("AES-GCM encryption failed".into()))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// AES-256-GCM decrypt from the `nonce(12) || ciphertext || tag(16)` layout.
pub fn decrypt_combined(key: &[u8; 32], combined: &[u8]) -> CryptoResult<Vec<u8>> {
    if combined.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoError::Validation(format!(
            "cipherText too short: {} bytes",
            combined.len()
        )));
    }
    let cipher_key = Key::<Aes256Gcm>::try_from(key.as_slice())
        .expect("32-byte key matches Aes256Gcm::KeySize");
    let cipher = Aes256Gcm::new(&cipher_key);
    let nonce = Nonce::<Aes256Gcm>::try_from(&combined[..NONCE_LEN])
        .expect("slice of NONCE_LEN bytes matches Aes256Gcm::NonceSize");
    let ciphertext = &combined[NONCE_LEN..];
    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| CryptoError::Operation("AES-GCM decryption failed".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = [0u8; 32];
        let plaintext = b"some plaintext bytes";
        let combined = encrypt_combined(&key, plaintext).unwrap();
        assert_eq!(combined.len(), NONCE_LEN + plaintext.len() + TAG_LEN);
        let decrypted = decrypt_combined(&key, &combined).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn tampering_is_rejected() {
        let key = [1u8; 32];
        let mut combined = encrypt_combined(&key, b"payload").unwrap();
        let last = combined.len() - 1;
        combined[last] ^= 0xFF;
        assert!(decrypt_combined(&key, &combined).is_err());
    }
}
