//! ML-KEM-512 / Kyber512 post-quantum key encapsulation, backed by
//! `pqc_kyber` (round-3 CRYSTALS-Kyber reference port). Verified
//! byte-for-byte compatible with the mobile app's `post_quantum` Dart
//! package (same seed + nonce -> identical public key, secret key,
//! ciphertext, and shared secret) — see the plan's tech-choice table for
//! the cross-check methodology.
//!
//! `pqc_kyber`'s low-level, seed-injectable functions (`crypto_kem_keypair`,
//! `crypto_kem_enc`, `crypto_kem_dec`) are gated behind the crate's
//! `benchmarking` feature (or a `kyber_kat`/`fuzzing` cfg flag, normally used
//! for its own Known-Answer-Test suite). We depend on `pqc_kyber` with
//! `features = ["kyber512", "benchmarking"]` to unlock them — the extra
//! `criterion`/`rayon` build-time dependencies that feature pulls in are not
//! used at runtime. (A global `--cfg kyber_kat` via `.cargo/config.toml` was
//! tried first but forces a full-workspace rebuild on every rustflags change,
//! which is fragile; the feature flag only affects this one crate.)

use pqc_kyber::*;
use rand08::rngs::OsRng as OsRng08;

use super::{CryptoError, CryptoResult};

pub const PQ_PUBLIC_KEY_LEN: usize = KYBER_PUBLICKEYBYTES; // 800
pub const PQ_PRIVATE_KEY_LEN: usize = KYBER_SECRETKEYBYTES; // 1632
pub const PQ_CIPHERTEXT_LEN: usize = KYBER_CIPHERTEXTBYTES; // 768
pub const PQ_SHARED_SECRET_LEN: usize = KYBER_SSBYTES; // 32
pub const PQ_SEED_LEN: usize = 64;

pub struct PqKeyPair {
    pub public_key: [u8; PQ_PUBLIC_KEY_LEN],
    pub private_key: [u8; PQ_PRIVATE_KEY_LEN],
}

pub struct KemResult {
    pub ciphertext: [u8; PQ_CIPHERTEXT_LEN],
    pub shared_secret: [u8; PQ_SHARED_SECRET_LEN],
}

/// Generate a fresh, random ML-KEM-512 keypair. Mirrors
/// `CryptoService.generatePqKeyPair`, used by the alias-chat invite creator
/// (`alias::onboarding::create_invitation_envelope`) — only the creator
/// generates a PQ keypair; the acceptor only ever encapsulates.
pub fn generate_pq_keypair() -> CryptoResult<PqKeyPair> {
    let mut pk = [0u8; PQ_PUBLIC_KEY_LEN];
    let mut sk = [0u8; PQ_PRIVATE_KEY_LEN];
    let mut rng = OsRng08;
    crypto_kem_keypair(&mut pk, &mut sk, &mut rng, None)
        .map_err(|e| CryptoError::Operation(format!("PQ keygen failed: {e:?}")))?;
    Ok(PqKeyPair {
        public_key: pk,
        private_key: sk,
    })
}

/// Generate an ML-KEM-512 keypair deterministically from a 64-byte seed
/// (bytes 0..32 = `d`, bytes 32..64 = `z`). Mirrors
/// `Kyber.kem512().generateKeys(seed)` as used by
/// `KeyService._generatePqKeysFromSeed` / `generateAndSavePqKeyPair`.
pub fn generate_pq_keypair_from_seed(seed: &[u8; PQ_SEED_LEN]) -> CryptoResult<PqKeyPair> {
    let mut pk = [0u8; PQ_PUBLIC_KEY_LEN];
    let mut sk = [0u8; PQ_PRIVATE_KEY_LEN];
    // Unused when a seed is supplied, but required by the function signature.
    let mut rng = OsRng08;
    let d = &seed[0..32];
    let z = &seed[32..64];
    crypto_kem_keypair(&mut pk, &mut sk, &mut rng, Some((d, z)))
        .map_err(|e| CryptoError::Operation(format!("PQ keygen failed: {e:?}")))?;
    Ok(PqKeyPair {
        public_key: pk,
        private_key: sk,
    })
}

/// Perform ML-KEM-512 encapsulation against a recipient's PQ public key.
/// Mirrors `CryptoService.kemEncapsulate`.
pub fn kem_encapsulate(recipient_pq_pubkey: &[u8]) -> CryptoResult<KemResult> {
    if recipient_pq_pubkey.len() != PQ_PUBLIC_KEY_LEN {
        return Err(CryptoError::Validation(format!(
            "recipientPqPubkey must be {PQ_PUBLIC_KEY_LEN} bytes, got {}",
            recipient_pq_pubkey.len()
        )));
    }
    let mut ct = [0u8; PQ_CIPHERTEXT_LEN];
    let mut ss = [0u8; PQ_SHARED_SECRET_LEN];
    let mut rng = OsRng08;
    crypto_kem_enc(&mut ct, &mut ss, recipient_pq_pubkey, &mut rng, None)
        .map_err(|e| CryptoError::Operation(format!("KEM encapsulation failed: {e:?}")))?;
    Ok(KemResult {
        ciphertext: ct,
        shared_secret: ss,
    })
}

/// Perform ML-KEM-512 decapsulation using our own PQ private key. Mirrors
/// `CryptoService.kemDecapsulate`. Never fails cryptographically (per the FO
/// transform's implicit-rejection design) — only length validation can error.
pub fn kem_decapsulate(
    kem_ciphertext: &[u8],
    my_pq_private_key: &[u8],
) -> CryptoResult<[u8; PQ_SHARED_SECRET_LEN]> {
    if kem_ciphertext.len() != PQ_CIPHERTEXT_LEN {
        return Err(CryptoError::Validation(format!(
            "kemCiphertext must be {PQ_CIPHERTEXT_LEN} bytes, got {}",
            kem_ciphertext.len()
        )));
    }
    if my_pq_private_key.len() != PQ_PRIVATE_KEY_LEN {
        return Err(CryptoError::Validation(format!(
            "myPqPrivateKey must be {PQ_PRIVATE_KEY_LEN} bytes, got {}",
            my_pq_private_key.len()
        )));
    }
    let mut ss = [0u8; PQ_SHARED_SECRET_LEN];
    crypto_kem_dec(&mut ss, kem_ciphertext, my_pq_private_key);
    Ok(ss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_from_seed_is_deterministic() {
        let seed = [42u8; PQ_SEED_LEN];
        let a = generate_pq_keypair_from_seed(&seed).unwrap();
        let b = generate_pq_keypair_from_seed(&seed).unwrap();
        assert_eq!(a.public_key, b.public_key);
        assert_eq!(a.private_key, b.private_key);
    }

    #[test]
    fn encapsulate_decapsulate_round_trip() {
        let seed = [7u8; PQ_SEED_LEN];
        let keys = generate_pq_keypair_from_seed(&seed).unwrap();

        let encapsulated = kem_encapsulate(&keys.public_key).unwrap();
        let decapsulated = kem_decapsulate(&encapsulated.ciphertext, &keys.private_key).unwrap();

        assert_eq!(encapsulated.shared_secret, decapsulated);
    }

    #[test]
    fn matches_known_answer_from_dart_post_quantum_cross_check() {
        // Same 64-byte seed / 32-byte nonce used in the manual Dart
        // cross-check (post_quantum's Kyber.kem512()): seed = 0..63,
        // nonce = 0xA0..0xBF. Values below are copied verbatim from that
        // run's output, confirming this module reproduces them exactly.
        let seed: [u8; 64] = std::array::from_fn(|i| i as u8);
        let keys = generate_pq_keypair_from_seed(&seed).unwrap();

        assert_eq!(&hex_encode(&keys.public_key)[..16], "ad9c21b9b7a12a80");
        assert_eq!(&hex_encode(&keys.private_key)[..16], "41e421b495cbc3a6");
    }

    fn hex_encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
