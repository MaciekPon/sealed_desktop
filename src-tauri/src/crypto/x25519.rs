//! X25519 key exchange helpers, ported from the X25519-related parts of
//! `crypto_service.dart` (backed by the `cryptography` package's `X25519`
//! primitive there).

use curve25519_dalek::edwards::CompressedEdwardsY;
use rand::rand_core::UnwrapErr;
use rand::rngs::SysRng;
use rand08::RngCore;
use sha2::{Digest, Sha512};
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

/// Convert an Ed25519 public key (an Edwards y-coordinate, as used for
/// Algorand/Solana wallet addresses) to its birationally-equivalent X25519
/// public key (a Montgomery u-coordinate). Ports
/// `key_format_converter.dart`'s `ed25519PublicKeyToX25519` — used to derive
/// a fallback encryption pubkey for a contact wallet that hasn't published
/// its own X25519 keys on-chain yet.
///
/// `None` for a degenerate/invalid point: either the encoding doesn't
/// decompress to a curve point at all, or it maps to the identity/a
/// small-order point (Montgomery `u == 0`). The Dart source rejects the
/// `y == 1` case specifically (its bigint formula divides by zero there);
/// this additionally rejects any point landing on `u == 0` on the X25519
/// side, since an all-zero public key is the classic small-order-point
/// input that makes X25519 ECDH produce an attacker-known shared secret
/// regardless of the other party's private key.
pub fn ed25519_public_key_to_x25519(ed25519_pubkey: &[u8; 32]) -> Option<[u8; 32]> {
    let montgomery = CompressedEdwardsY(*ed25519_pubkey).decompress()?.to_montgomery().to_bytes();
    if montgomery == [0u8; 32] {
        return None;
    }
    Some(montgomery)
}

/// A freshly-generated, one-shot X25519 keypair. Mirrors
/// `CryptoService.generateEphemeralKeyPair`.
pub struct EphemeralKeyPair {
    pub secret: EphemeralSecret,
    pub public: [u8; 32],
}

pub fn generate_ephemeral_keypair() -> EphemeralKeyPair {
    let secret = EphemeralSecret::random_from_rng(&mut UnwrapErr(SysRng));
    let public = *PublicKey::from(&secret).as_bytes();
    EphemeralKeyPair { secret, public }
}

/// A freshly-generated X25519 keypair meant for **repeated** Diffie-Hellman
/// calls within one scope — e.g. a single outgoing message needs ECDH
/// against three different peers' pubkeys (recipient scan key for the
/// stealth tag, recipient encryption key, sender's own encryption key for
/// the self-copy) from the *same* fresh secret. Unlike [`EphemeralKeyPair`]
/// (backed by `EphemeralSecret`, whose `diffie_hellman` consumes `self` by
/// design, enforcing single-use), this is backed by `StaticSecret` purely
/// for its non-consuming `diffie_hellman(&self, ...)` — the "static" naming
/// is x25519-dalek's, not a claim about this being long-lived; it's still
/// generated fresh and dropped at the end of the same scope.
pub struct ReusableKeyPair {
    pub secret: StaticSecret,
    pub public: [u8; 32],
}

pub fn generate_reusable_keypair() -> ReusableKeyPair {
    let secret = StaticSecret::random_from_rng(&mut UnwrapErr(SysRng));
    let public = *PublicKey::from(&secret).as_bytes();
    ReusableKeyPair { secret, public }
}

/// SHA-512(ed25519_seed)[0..32], clamped per RFC 7748. Mirrors
/// `ed25519SeedToX25519Seed` in `key_format_converter.dart` — the private-key
/// half of the wallet-derived-keypair fallback (paired with
/// [`ed25519_public_key_to_x25519`] for the public half; cross-checked
/// against a `dart run` of the same Dart function to confirm the two agree
/// — see the test below).
pub fn ed25519_seed_to_x25519_seed(ed25519_seed: &[u8; 32]) -> [u8; 32] {
    let hash = Sha512::digest(ed25519_seed);
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&hash[..32]);
    seed[0] &= 248;
    seed[31] &= 127;
    seed[31] |= 64;
    seed
}

pub fn public_key_from_bytes(bytes: &[u8; 32]) -> PublicKey {
    PublicKey::from(*bytes)
}

/// Random 32-byte X25519 seed, storable and reconstructible via
/// [`public_key_from_seed`]/[`shared_secret_from_seed`]. Unlike
/// [`EphemeralKeyPair`]/[`ReusableKeyPair`] (which wrap non-serializable
/// `dalek` secret types for single-scope use), alias-chat identities have no
/// deterministic derivation and must be persisted as raw bytes across app
/// restarts — this is the seed to store.
pub fn random_seed() -> [u8; 32] {
    let mut seed = [0u8; 32];
    rand08::rngs::OsRng.fill_bytes(&mut seed);
    seed
}

/// Deterministically derive the public key for a 32-byte seed, the same way
/// `X25519().newKeyPairFromSeed(seed)` does on the Dart side (both clamp the
/// scalar per RFC 7748 before the base-point multiplication, so this is
/// interoperable regardless of implementation).
pub fn public_key_from_seed(seed: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*seed);
    *PublicKey::from(&secret).as_bytes()
}

/// X25519 ECDH between a seed-derived static secret and a peer's public key.
/// Mirrors `CryptoService.computeSharedSecret` for the "keypair derived from
/// a stored seed" call sites (view key, encryption key, wallet-derived key).
pub fn shared_secret_from_seed(my_seed: &[u8; 32], their_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*my_seed);
    let public = PublicKey::from(*their_public);
    *secret.diffie_hellman(&public).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_secret_is_symmetric() {
        let a_seed = [11u8; 32];
        let b_seed = [22u8; 32];
        let a_pub = public_key_from_seed(&a_seed);
        let b_pub = public_key_from_seed(&b_seed);

        let a_shared = shared_secret_from_seed(&a_seed, &b_pub);
        let b_shared = shared_secret_from_seed(&b_seed, &a_pub);
        assert_eq!(a_shared, b_shared);
    }

    fn hex_to_32(hex: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        out
    }

    /// Cross-checked against `key_format_converter.dart`'s
    /// `ed25519PublicKeyToX25519`: ran the same bigint birational-map
    /// formula in a `dart run` scratch script against the Ed25519 pubkey
    /// derived from an all-`0x07` 32-byte seed (via the `cryptography`
    /// package's `Ed25519().newKeyPairFromSeed`), and compared byte-for-byte
    /// against this crate's `curve25519-dalek`-based conversion.
    #[test]
    fn matches_known_answer_from_dart_key_format_converter_cross_check() {
        let ed25519_pub = hex_to_32("ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c");
        let expected_x25519 = hex_to_32("761d88ec830413919dfe9d4d1d56f17e653c8c994082df5b137b90a0ae6edf74");
        assert_eq!(ed25519_public_key_to_x25519(&ed25519_pub).unwrap(), expected_x25519);
    }

    /// Cross-checked against `key_format_converter.dart`'s
    /// `ed25519SeedToX25519Seed`: ran the same SHA-512+clamp steps in a
    /// `dart run` scratch script against the same all-`0x07` 32-byte seed
    /// used in `matches_known_answer_from_dart_key_format_converter_cross_check`
    /// above, then additionally derived the X25519 public key from that
    /// seed (via `cryptography`'s `X25519().newKeyPairFromSeed`) and
    /// confirmed it equals the *other* cross-check's birational-map pubkey
    /// — i.e. both Dart conversions (pubkey-only and seed-to-keypair) agree
    /// with each other, and this function reproduces the seed-to-keypair
    /// side byte-for-byte.
    #[test]
    fn matches_known_answer_from_dart_seed_conversion_cross_check() {
        let ed25519_seed = [7u8; 32];
        let expected_x25519_seed = hex_to_32("28ad39fefd7fa3e200a9c626eef599e61a2d055c48a8288a4e7e4c4bca392878");
        let x25519_seed = ed25519_seed_to_x25519_seed(&ed25519_seed);
        assert_eq!(x25519_seed, expected_x25519_seed);

        // The pubkey derived from this seed must match the pubkey derived
        // via the birational map applied to the corresponding Ed25519
        // pubkey (both cross-checks used the same underlying Dart seed).
        let expected_pub = hex_to_32("761d88ec830413919dfe9d4d1d56f17e653c8c994082df5b137b90a0ae6edf74");
        assert_eq!(public_key_from_seed(&x25519_seed), expected_pub);
    }

    #[test]
    fn random_seed_is_not_reused_and_derives_a_stable_pubkey() {
        let a = random_seed();
        let b = random_seed();
        assert_ne!(a, b);
        assert_eq!(public_key_from_seed(&a), public_key_from_seed(&a));
    }

    #[test]
    fn ephemeral_matches_static_peer() {
        let peer_seed = [33u8; 32];
        let peer_pub = public_key_from_seed(&peer_seed);

        let ephemeral = generate_ephemeral_keypair();
        let ephemeral_shared = ephemeral
            .secret
            .diffie_hellman(&public_key_from_bytes(&peer_pub));
        let peer_shared = shared_secret_from_seed(&peer_seed, &ephemeral.public);
        assert_eq!(*ephemeral_shared.as_bytes(), peer_shared);
    }
}
