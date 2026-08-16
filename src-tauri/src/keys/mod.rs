//! Derives the app's messaging key material (X25519 encryption/scan keys +
//! ML-KEM keypair) from the wallet seed. Ported from
//! `KeyService.deriveKeysFromLocalWallet` in `services/key_service.dart` —
//! the legacy wallet-signature-based `deriveKeys` path (Solana-era, already
//! `@Deprecated` in the Dart source) is not ported.
//!
//! This derivation must stay byte-identical to mobile: restoring the same
//! wallet mnemonic on desktop must yield the same encryption/scan/PQ keys,
//! so a user's own historical on-chain messages stay decryptable and their
//! published keys match across devices — including messages from a peer
//! whose client cached this account's keys before desktop ever ran
//! `publish_keys`. The X25519 encryption/scan branch was always correct
//! (cross-checked line-by-line against `key_service.dart` below) and is
//! only verified here by determinism/round-trip tests, since it's pure
//! standard HKDF-SHA256/X25519 with no cross-implementation ambiguity. The
//! PQ (ML-KEM) branch previously was **not** cross-checked closely enough
//! and diverged from Dart in a way plain determinism tests can't catch
//! (both sides are internally self-consistent, just different from each
//! other) — see `derive_sealed_keys`'s doc comment for the bug and fix.

use sha2::{Digest, Sha256};

use crate::chain::wallet::AlgorandWallet;
use crate::crypto::{kdf, pq, x25519, CryptoError};

const KEY_DOMAIN_PREFIX: &[u8] = b"sealed-messaging-keys-v1:";
const ENCRYPTION_INFO: &[u8] = b"sealed-encryption-v1";
const VIEW_INFO: &[u8] = b"sealed-view-v1";
const PQ_MASTER_SEED_INFO: &[u8] = b"sealed-pq-master-seed-v1";
const PQ_KEM_INFO: &[u8] = b"sealed-pq-kem-v1";

pub struct SealedKeys {
    pub wallet_address: String,
    pub encryption_pubkey: [u8; 32],
    pub scan_pubkey: [u8; 32],
    pub encryption_private_key: [u8; 32],
    pub view_private_key: [u8; 32],
    pub pq_public_key: Vec<u8>,
    pub pq_private_key: Vec<u8>,
}

/// `trim + lowercase + collapse internal whitespace to single spaces` —
/// must match `KeyService._normalizeMnemonic` exactly, since the PQ master
/// seed below is keyed on this normalized text, not the wallet's binary
/// seed bytes.
fn normalize_mnemonic(raw: &str) -> String {
    raw.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Derive the full [`SealedKeys`] bundle from a loaded wallet.
///
/// **Bug found and fixed 2026-08-10**: the PQ (ML-KEM) branch used to derive
/// its seed straight from `wallet.seed_bytes()` via a single HKDF hop, which
/// diverges from `KeyService._generatePqKeysFromMasterSeed` in
/// `key_service.dart` — mobile keys the *first* HKDF hop off the normalized
/// **mnemonic text** (not the derived seed bytes) with
/// `info="sealed-pq-master-seed-v1"`, producing a 64-byte "PQ master seed",
/// then runs a *second* HKDF hop over that master seed with
/// `info="sealed-pq-kem-v1"` to get the actual ML-KEM keygen seed. Desktop's
/// old one-hop version therefore derived a completely different ML-KEM
/// keypair than mobile for the same account/wallet — self-consistent (so
/// desktop's own sent/received self-copies always worked) but incompatible
/// with any peer who cached this account's mobile-published PQ pubkey
/// before desktop ever ran `publish_keys`: their KEM ciphertexts decapsulate
/// to garbage on desktop (ML-KEM's implicit-rejection behavior — it doesn't
/// error, just returns a wrong-but-plausible-looking shared secret), which
/// gets cached in `contacts_cache` and poisons every subsequent hybrid-
/// encrypted message from that sender, permanently and silently (dropped by
/// `sync_incoming_messages`'s decrypt-or-continue fallback). The classical
/// X25519 encryption/view-key derivation above was always correct — only
/// this PQ branch was wrong.
pub fn derive_sealed_keys(wallet: &AlgorandWallet) -> Result<SealedKeys, CryptoError> {
    let seed = wallet.seed_bytes();

    // 1) Hash seed -> entropy (SHA-256), with a domain separator.
    let mut preimage = Vec::with_capacity(KEY_DOMAIN_PREFIX.len() + seed.len());
    preimage.extend_from_slice(KEY_DOMAIN_PREFIX);
    preimage.extend_from_slice(&seed);
    let entropy: [u8; 32] = Sha256::digest(&preimage).into();

    // 2) Derive two 32-byte seeds via HKDF.
    let encryption_seed = kdf::derive_seed32(&entropy, ENCRYPTION_INFO);
    let view_seed = kdf::derive_seed32(&entropy, VIEW_INFO);

    // 3) X25519 pubkeys from those seeds.
    let encryption_pubkey = x25519::public_key_from_seed(&encryption_seed);
    let scan_pubkey = x25519::public_key_from_seed(&view_seed);

    // 4) ML-KEM-512 keypair — two-hop HKDF over the normalized mnemonic
    // *text*, matching `_deriveCanonicalPqMasterSeed` +
    // `_generatePqKeysFromMasterSeed` exactly. See this function's doc
    // comment for why this can't be a single hop over `seed`.
    let normalized_mnemonic = normalize_mnemonic(wallet.mnemonic());
    let pq_master_seed = kdf::derive_seed64(normalized_mnemonic.as_bytes(), PQ_MASTER_SEED_INFO);
    let pq_seed = kdf::derive_seed64(&pq_master_seed, PQ_KEM_INFO);
    let pq_keys = pq::generate_pq_keypair_from_seed(&pq_seed)?;

    Ok(SealedKeys {
        wallet_address: wallet.address.clone(),
        encryption_pubkey,
        scan_pubkey,
        encryption_private_key: encryption_seed,
        view_private_key: view_seed,
        pq_public_key: pq_keys.public_key.to_vec(),
        pq_private_key: pq_keys.private_key.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_is_deterministic_for_the_same_wallet() {
        let wallet = AlgorandWallet::create();
        let restored = AlgorandWallet::restore(wallet.mnemonic()).unwrap();

        let keys1 = derive_sealed_keys(&wallet).unwrap();
        let keys2 = derive_sealed_keys(&restored).unwrap();

        assert_eq!(keys1.encryption_pubkey, keys2.encryption_pubkey);
        assert_eq!(keys1.scan_pubkey, keys2.scan_pubkey);
        assert_eq!(keys1.encryption_private_key, keys2.encryption_private_key);
        assert_eq!(keys1.view_private_key, keys2.view_private_key);
        assert_eq!(keys1.pq_public_key, keys2.pq_public_key);
        assert_eq!(keys1.pq_private_key, keys2.pq_private_key);
    }

    #[test]
    fn different_wallets_give_different_keys() {
        let a = AlgorandWallet::create();
        let b = AlgorandWallet::create();

        let keys_a = derive_sealed_keys(&a).unwrap();
        let keys_b = derive_sealed_keys(&b).unwrap();

        assert_ne!(keys_a.encryption_pubkey, keys_b.encryption_pubkey);
        assert_ne!(keys_a.scan_pubkey, keys_b.scan_pubkey);
        assert_ne!(keys_a.pq_public_key, keys_b.pq_public_key);
    }

    #[test]
    fn encryption_and_view_keys_differ() {
        let wallet = AlgorandWallet::create();
        let keys = derive_sealed_keys(&wallet).unwrap();
        assert_ne!(keys.encryption_pubkey, keys.scan_pubkey);
        assert_ne!(keys.encryption_private_key, keys.view_private_key);
    }

    #[test]
    fn pq_keys_have_expected_sizes() {
        let wallet = AlgorandWallet::create();
        let keys = derive_sealed_keys(&wallet).unwrap();
        assert_eq!(keys.pq_public_key.len(), crate::crypto::pq::PQ_PUBLIC_KEY_LEN);
        assert_eq!(keys.pq_private_key.len(), crate::crypto::pq::PQ_PRIVATE_KEY_LEN);
    }

    #[test]
    fn normalize_mnemonic_trims_lowercases_and_collapses_whitespace() {
        assert_eq!(normalize_mnemonic("  Abandon   ABANDON\tAbandon\n"), "abandon abandon abandon");
        assert_eq!(normalize_mnemonic("already normal"), "already normal");
    }

    /// Regression guard for the 2026-08-10 bug (see `derive_sealed_keys`'s
    /// doc comment): the PQ keypair must come from the two-hop
    /// mnemonic-text chain, not a one-hop derivation straight off
    /// `wallet.seed_bytes()` — the two must NOT collide for the same
    /// wallet, or this fix has silently regressed back to the
    /// mobile-incompatible scheme.
    #[test]
    fn pq_derivation_does_not_collide_with_the_old_seed_bytes_scheme() {
        let wallet = AlgorandWallet::create();
        let keys = derive_sealed_keys(&wallet).unwrap();

        let old_scheme_seed = kdf::derive_seed64(&wallet.seed_bytes(), PQ_KEM_INFO);
        let old_scheme_keys = pq::generate_pq_keypair_from_seed(&old_scheme_seed).unwrap();

        assert_ne!(keys.pq_public_key, old_scheme_keys.public_key.to_vec());
    }
}
