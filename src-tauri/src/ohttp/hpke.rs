//! OHTTP request/response encapsulation per RFC 9458, ported from
//! `remote/ohttp/ohttp_encapsulator.dart`.
//!
//! Implements HPKE (RFC 9180) in Base mode with:
//! - `DHKEM(X25519, HKDF-SHA256)` (KEM ID `0x0020`)
//! - `HKDF-SHA256` (KDF ID `0x0001`)
//! - `AES-128-GCM` (AEAD ID `0x0001`)
//!
//! Only these fixed algorithm IDs are supported — same scope as the Dart
//! source, which hardcodes them throughout (`suiteId`, key/nonce sizes).

use aes_gcm::aead::{Aead, KeyInit, Nonce};
use aes_gcm::{Aes128Gcm, Key};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use thiserror::Error;
use rand::rand_core::UnwrapErr;
use rand::rngs::SysRng;
use x25519_dalek::{EphemeralSecret, PublicKey};

use super::config::OhttpConfig;

#[derive(Debug, Error)]
pub enum HpkeError {
    #[error("OHTTP response too short")]
    ResponseTooShort,
    #[error("OHTTP response ciphertext too short")]
    CiphertextTooShort,
    #[error("AEAD encryption failed")]
    EncryptFailed,
    #[error("AEAD decryption failed")]
    DecryptFailed,
    #[error("invalid gateway public key length")]
    InvalidPublicKey,
}

pub struct EncapsulatedRequest {
    pub encapsulated_message: Vec<u8>,
    /// Ephemeral public key, needed for response decapsulation.
    pub enc: [u8; 32],
    /// HPKE export secret, needed for response decapsulation.
    pub secret: Vec<u8>,
}

const HPKE_SUITE_ID: &[u8] = b"HPKE\x00\x20\x00\x01\x00\x01"; // KEM=0x0020, KDF=0x0001, AEAD=0x0001
const KEM_SUITE_ID: &[u8] = b"KEM\x00\x20";

pub fn encapsulate_request(
    config: &OhttpConfig,
    binary_request: &[u8],
) -> Result<EncapsulatedRequest, HpkeError> {
    if config.public_key.len() != 32 {
        return Err(HpkeError::InvalidPublicKey);
    }
    let mut gateway_pubkey_bytes = [0u8; 32];
    gateway_pubkey_bytes.copy_from_slice(&config.public_key);
    let gateway_pubkey = PublicKey::from(gateway_pubkey_bytes);

    let ephemeral_secret = EphemeralSecret::random_from_rng(&mut UnwrapErr(SysRng));
    let enc: [u8; 32] = *PublicKey::from(&ephemeral_secret).as_bytes();
    let dh = *ephemeral_secret.diffie_hellman(&gateway_pubkey).as_bytes();

    let mut kem_context = Vec::with_capacity(64);
    kem_context.extend_from_slice(&enc);
    kem_context.extend_from_slice(&config.public_key);
    let shared_secret = dhkem_extract_and_expand(&dh, &kem_context);

    let hdr = ohttp_key_info_header(config);
    let mut hpke_info = Vec::new();
    hpke_info.extend_from_slice(b"message/bhttp request");
    hpke_info.push(0x00);
    hpke_info.extend_from_slice(&hdr);

    let (key, base_nonce, exporter_secret) = key_schedule_base(&shared_secret, &hpke_info);

    let cipher = Aes128Gcm::new(&Key::<Aes128Gcm>::try_from(key.as_slice()).map_err(|_| HpkeError::EncryptFailed)?);
    let nonce = Nonce::<Aes128Gcm>::try_from(base_nonce.as_slice()).map_err(|_| HpkeError::EncryptFailed)?;
    let ciphertext = cipher
        .encrypt(&nonce, binary_request)
        .map_err(|_| HpkeError::EncryptFailed)?;

    let mut encapsulated_message = Vec::with_capacity(hdr.len() + 32 + ciphertext.len());
    encapsulated_message.extend_from_slice(&hdr);
    encapsulated_message.extend_from_slice(&enc);
    encapsulated_message.extend_from_slice(&ciphertext);

    // HPKE export for response decapsulation:
    // export(exporter_secret, "message/bhttp response", Nk=16)
    let export_context = b"message/bhttp response";
    let secret = labeled_expand(&exporter_secret, b"sec", export_context, 16, HPKE_SUITE_ID);

    Ok(EncapsulatedRequest {
        encapsulated_message,
        enc,
        secret,
    })
}

/// Per RFC 9458 §4.4 / the ohttp-js reference: response format is
/// `responseNonce(max(Nk,Nn)) || encrypted_response`. Key derivation:
/// `salt = enc || responseNonce; prk = Extract(salt, secret); key =
/// Expand(prk, "key", Nk); nonce = Expand(prk, "nonce", Nn)` — a plain
/// (non-HPKE-suite-labeled) HKDF, distinct from the request's full HPKE
/// KeySchedule above.
pub fn decapsulate_response(
    encrypted_response: &[u8],
    enc: &[u8; 32],
    secret: &[u8],
) -> Result<super::binary_http::BinaryHttpResponse, HpkeError> {
    let response_nonce_len = 16usize; // max(Nk=16, Nn=12)
    if encrypted_response.len() < response_nonce_len + 16 {
        return Err(HpkeError::ResponseTooShort);
    }
    let response_nonce = &encrypted_response[..response_nonce_len];
    let enc_response = &encrypted_response[response_nonce_len..];

    let mut salt = Vec::with_capacity(32 + response_nonce_len);
    salt.extend_from_slice(enc);
    salt.extend_from_slice(response_nonce);

    let prk = hmac_sha256(&salt, secret);
    let aead_key = hkdf_expand(&prk, b"key", 16);
    let aead_nonce = hkdf_expand(&prk, b"nonce", 12);

    if enc_response.len() < 16 {
        return Err(HpkeError::CiphertextTooShort);
    }
    let cipher = Aes128Gcm::new(&Key::<Aes128Gcm>::try_from(aead_key.as_slice()).map_err(|_| HpkeError::DecryptFailed)?);
    let nonce = Nonce::<Aes128Gcm>::try_from(aead_nonce.as_slice()).map_err(|_| HpkeError::DecryptFailed)?;
    let plaintext = cipher
        .decrypt(&nonce, enc_response)
        .map_err(|_| HpkeError::DecryptFailed)?;

    super::binary_http::decode_response(&plaintext).map_err(|_| HpkeError::DecryptFailed)
}

// ===========================================================================
// HPKE internals (RFC 9180)
// ===========================================================================

/// DHKEM ExtractAndExpand (RFC 9180 §4.1).
fn dhkem_extract_and_expand(dh: &[u8], kem_context: &[u8]) -> Vec<u8> {
    let mut labeled_ikm = Vec::new();
    labeled_ikm.extend_from_slice(b"HPKE-v1");
    labeled_ikm.extend_from_slice(KEM_SUITE_ID);
    labeled_ikm.extend_from_slice(b"eae_prk");
    labeled_ikm.extend_from_slice(dh);

    let prk = hmac_sha256(&[0u8; 32], &labeled_ikm); // empty salt -> zero key
    labeled_expand(&prk, b"shared_secret", kem_context, 32, KEM_SUITE_ID)
}

/// HPKE KeySchedule in Base mode (RFC 9180 §5.1). Returns (key, base_nonce,
/// exporter_secret).
fn key_schedule_base(shared_secret: &[u8], info: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let psk_id_hash = labeled_extract(&[], b"psk_id_hash", &[], HPKE_SUITE_ID);
    let info_hash = labeled_extract(&[], b"info_hash", info, HPKE_SUITE_ID);

    let mut ks_context = Vec::with_capacity(1 + psk_id_hash.len() + info_hash.len());
    ks_context.push(0x00); // mode: Base
    ks_context.extend_from_slice(&psk_id_hash);
    ks_context.extend_from_slice(&info_hash);

    let secret = labeled_extract(shared_secret, b"secret", &[], HPKE_SUITE_ID);
    let key = labeled_expand(&secret, b"key", &ks_context, 16, HPKE_SUITE_ID);
    let base_nonce = labeled_expand(&secret, b"base_nonce", &ks_context, 12, HPKE_SUITE_ID);
    let exporter_secret = labeled_expand(&secret, b"exp", &ks_context, 32, HPKE_SUITE_ID);

    (key, base_nonce, exporter_secret)
}

/// LabeledExtract (RFC 9180 §4): `Extract(salt, "HPKE-v1" || suite_id || label || ikm)`.
fn labeled_extract(salt: &[u8], label: &[u8], ikm: &[u8], suite_id: &[u8]) -> Vec<u8> {
    let mut labeled_ikm = Vec::new();
    labeled_ikm.extend_from_slice(b"HPKE-v1");
    labeled_ikm.extend_from_slice(suite_id);
    labeled_ikm.extend_from_slice(label);
    labeled_ikm.extend_from_slice(ikm);

    let effective_salt: Vec<u8> = if salt.is_empty() { vec![0u8; 32] } else { salt.to_vec() };
    hmac_sha256(&effective_salt, &labeled_ikm)
}

/// LabeledExpand (RFC 9180 §4).
fn labeled_expand(prk: &[u8], label: &[u8], info: &[u8], length: usize, suite_id: &[u8]) -> Vec<u8> {
    let mut labeled_info = Vec::new();
    labeled_info.extend_from_slice(&(length as u16).to_be_bytes());
    labeled_info.extend_from_slice(b"HPKE-v1");
    labeled_info.extend_from_slice(suite_id);
    labeled_info.extend_from_slice(label);
    labeled_info.extend_from_slice(info);

    hkdf_expand(prk, &labeled_info, length)
}

/// OHTTP key info header = `keyId(1) || kemId(2 BE) || kdfId(2 BE) || aeadId(2 BE)`.
fn ohttp_key_info_header(config: &OhttpConfig) -> [u8; 7] {
    let mut hdr = [0u8; 7];
    hdr[0] = config.key_id;
    hdr[1..3].copy_from_slice(&config.kem_id.to_be_bytes());
    hdr[3..5].copy_from_slice(&config.kdf_id.to_be_bytes());
    hdr[5..7].copy_from_slice(&config.aead_id.to_be_bytes());
    hdr
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

/// Plain HKDF-Expand (RFC 5869), hand-rolled to match the Dart source's
/// direct `T(i) = HMAC(prk, T(i-1) || info || i)` loop byte-for-byte.
fn hkdf_expand(prk: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let hash_len = 32usize;
    let n = length.div_ceil(hash_len);
    let mut result = Vec::with_capacity(n * hash_len);
    let mut t: Vec<u8> = Vec::new();
    for i in 1..=n {
        let mut mac = Hmac::<Sha256>::new_from_slice(prk).expect("HMAC accepts any key length");
        mac.update(&t);
        mac.update(info);
        mac.update(&[i as u8]);
        t = mac.finalize().into_bytes().to_vec();
        result.extend_from_slice(&t);
    }
    result.truncate(length);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ohttp::binary_http;
    use x25519_dalek::StaticSecret;

    fn test_config() -> (OhttpConfig, [u8; 32]) {
        let gateway_secret = StaticSecret::from([7u8; 32]);
        let gateway_public = *PublicKey::from(&gateway_secret).as_bytes();
        let config = OhttpConfig {
            key_id: 0x80,
            kem_id: 0x0020,
            kdf_id: 0x0001,
            aead_id: 0x0001,
            public_key: gateway_public.to_vec(),
        };
        (config, gateway_secret.to_bytes())
    }

    #[test]
    fn encapsulate_request_produces_valid_envelope_structure() {
        let (config, _) = test_config();
        let url = url::Url::parse("https://testnet-api.4160.nodely.dev/v2/status").unwrap();
        let binary_request = binary_http::encode_request("GET", &url, &[], None).unwrap();

        let result = encapsulate_request(&config, &binary_request).unwrap();

        assert!(result.encapsulated_message.len() > 7 + 32 + 16);
        assert_eq!(result.encapsulated_message[0], 0x80); // keyId
        assert_eq!(&result.encapsulated_message[1..3], &[0x00, 0x20]); // kemId
        assert_eq!(&result.encapsulated_message[3..5], &[0x00, 0x01]); // kdfId
        assert_eq!(&result.encapsulated_message[5..7], &[0x00, 0x01]); // aeadId
        assert_eq!(result.enc.len(), 32);
        assert_eq!(result.secret.len(), 16);
    }

    #[test]
    fn each_encapsulation_produces_different_ciphertext() {
        let (config, _) = test_config();
        let url = url::Url::parse("https://example.com/test").unwrap();
        let binary_request = binary_http::encode_request("GET", &url, &[], None).unwrap();

        let r1 = encapsulate_request(&config, &binary_request).unwrap();
        let r2 = encapsulate_request(&config, &binary_request).unwrap();
        assert_ne!(r1.encapsulated_message, r2.encapsulated_message);
    }

    /// Ported from `ohttp_encapsulator_test.dart` ("decapsulateResponse
    /// decrypts correctly") — manually derives the same response keys and
    /// encrypts a known plaintext Binary HTTP response, then checks our
    /// `decapsulate_response` recovers it.
    #[test]
    fn decapsulate_response_decrypts_correctly() {
        let enc: [u8; 32] = std::array::from_fn(|i| (i + 1) as u8);
        let secret = vec![0x42u8; 16];
        let response_nonce: Vec<u8> = (0..16u8).map(|i| i + 10).collect();

        let mut plain_response = vec![0x01u8]; // known-length response
        plain_response.extend_from_slice(&[0x40, 0xC8]); // status 200
        plain_response.push(0x00); // empty headers
        let body = b"{\"status\":\"ok\"}";
        plain_response.push(body.len() as u8);
        plain_response.extend_from_slice(body);
        plain_response.push(0x00); // empty trailers

        let mut salt = enc.to_vec();
        salt.extend_from_slice(&response_nonce);
        let prk = hmac_sha256(&salt, &secret);
        let aead_key = hkdf_expand(&prk, b"key", 16);
        let aead_nonce = hkdf_expand(&prk, b"nonce", 12);

        let cipher = Aes128Gcm::new(&Key::<Aes128Gcm>::try_from(aead_key.as_slice()).unwrap());
        let nonce = Nonce::<Aes128Gcm>::try_from(aead_nonce.as_slice()).unwrap();
        let ciphertext = cipher.encrypt(&nonce, plain_response.as_slice()).unwrap();

        let mut encrypted_response = response_nonce.clone();
        encrypted_response.extend_from_slice(&ciphertext);

        let decoded = decapsulate_response(&encrypted_response, &enc, &secret).unwrap();
        assert_eq!(decoded.status_code, 200);
        assert_eq!(decoded.body, body);
    }
}
