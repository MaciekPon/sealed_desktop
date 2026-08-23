//! On-chain `UserState` ARC4 tuple decoding + box-key derivation, ported
//! from the private helpers at the bottom of `sealed_chain_client.dart`.
//!
//! Wire versions v2 and v3 are both live on chain — the contract migrates a
//! v2 box lazily (rewritten as v3 on the wallet's next credit-spending
//! write), so decoding must branch on the version byte rather than assume
//! one fixed shape.
//!
//! `UserState` ARC4 tuple shape:
//! v3: `(uint8, byte[], uint8, (uint64,uint64)[], byte[32], byte[32], byte[32], byte[])`
//!     (bio appended 2026-06-12)
//! v2: same minus the trailing bio `byte[]`.
//!
//! ARC4 tuple head for mixed static/dynamic fields: each field is either
//! inlined (static) or represented by a 2-byte BE offset into the tuple
//! bytes (dynamic). Layout:
//!
//! ```text
//! offset 0:   version      uint8      1B  static
//! offset 1:   usernameOff  uint16     2B  -> points to username length+data
//! offset 3:   batchCount   uint8      1B  static
//! offset 4:   batchesOff   uint16     2B  -> points to batches array
//! offset 6:   encPubkey    byte[32]  32B  static
//! offset 38:  scanPubkey   byte[32]  32B  static
//! offset 70:  pqPubkeyHash byte[32]  32B  static
//! offset 102: bioOff       uint16     2B  -> v3 ONLY
//! Total head: 102 bytes (v2) / 104 bytes (v3)
//! ```
//!
//! Dynamic: `username = bytes[usernameOff .. usernameOff+2+len]` (2-byte BE
//! length prefix + raw UTF-8); `bio (v3) = bytes[bioOff .. bioOff+2+len]`,
//! same shape; `batches = bytes[batchesOff ..]`, 2-byte BE count + count ×
//! (uint64 BE + uint64 BE).

use sha2::{Digest, Sha256};

// Decoded for a complete, byte-accurate parse of the ARC4 tuple, but credit
// balance is read via `SealedChainClient::get_credits`'s own ABI call
// instead of by summing batches locally — nothing reads these back yet.
#[allow(dead_code)]
pub struct Batch {
    pub amount: u64,
    pub expiry_round: u64,
}

pub struct DecodedUserState {
    // Same reasoning as `Batch` above — decoded for completeness, unread.
    #[allow(dead_code)]
    pub version: u8,
    pub username: Vec<u8>,
    #[allow(dead_code)]
    pub batch_count: u8,
    #[allow(dead_code)]
    pub batches: Vec<Batch>,
    pub encryption_pubkey: [u8; 32],
    pub scan_pubkey: [u8; 32],
    pub pq_pubkey_hash: [u8; 32],
    /// Empty for a v2 box (no bio slot) or when the field is simply unset.
    pub bio: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeUserStateError {
    #[error("UserState too short: {0} bytes (need >= 102)")]
    TooShort(usize),
}

/// ARC4 dynamic bytes at `off`: 2B BE length + payload. Empty on any
/// out-of-range read (defensive — box bytes come from algod).
fn read_dyn_bytes(bytes: &[u8], off: usize) -> Vec<u8> {
    if off + 2 > bytes.len() {
        return Vec::new();
    }
    let len = u16::from_be_bytes([bytes[off], bytes[off + 1]]) as usize;
    let end = off + 2 + len;
    if end <= bytes.len() {
        bytes[off + 2..end].to_vec()
    } else {
        Vec::new()
    }
}

pub fn decode_user_state(bytes: &[u8]) -> Result<DecodedUserState, DecodeUserStateError> {
    if bytes.len() < 102 {
        return Err(DecodeUserStateError::TooShort(bytes.len()));
    }

    // Deliberately permissive about the version byte itself — unlike the
    // Dart client (which hard-rejects anything but 2/3), this only branches
    // on it to decide whether a bio slot is present. The static head
    // (offsets 0..102: username/batch offsets + the three 32-byte keys) has
    // been stable across every version seen so far, so an unrecognized
    // version byte still decodes those fields correctly — it only means
    // "don't attempt to read a bio slot that may not exist at this offset".
    // A prior stricter version (hard error on anything != 2/3) broke
    // username/key resolution outright for a real account whose deployed
    // on-chain version didn't match this port's assumptions — see the git
    // history around 2026-08-23. Bio decode is the only piece gated behind
    // an explicit length check (`bytes.len() >= 104`), which is a real
    // out-of-bounds guard, not a version-identity check.
    let version = bytes[0];
    let has_bio_slot = version == 3 && bytes.len() >= 104;

    let username_off = u16::from_be_bytes([bytes[1], bytes[2]]) as usize;
    let batch_count = bytes[3];
    let batches_off = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    let mut encryption_pubkey = [0u8; 32];
    encryption_pubkey.copy_from_slice(&bytes[6..38]);
    let mut scan_pubkey = [0u8; 32];
    scan_pubkey.copy_from_slice(&bytes[38..70]);
    let mut pq_pubkey_hash = [0u8; 32];
    pq_pubkey_hash.copy_from_slice(&bytes[70..102]);

    let username = read_dyn_bytes(bytes, username_off);

    // Bio (v3 only; v2 or an unrecognized version has no bio slot -> empty).
    let bio = if has_bio_slot {
        let bio_off = u16::from_be_bytes([bytes[102], bytes[103]]) as usize;
        read_dyn_bytes(bytes, bio_off)
    } else {
        Vec::new()
    };

    let mut batches = Vec::new();
    if batches_off + 2 <= bytes.len() {
        let arr_len = u16::from_be_bytes([bytes[batches_off], bytes[batches_off + 1]]) as usize;
        let mut cursor = batches_off + 2;
        for _ in 0..arr_len {
            if cursor + 16 > bytes.len() {
                break;
            }
            let amount = u64::from_be_bytes(bytes[cursor..cursor + 8].try_into().unwrap());
            let expiry = u64::from_be_bytes(bytes[cursor + 8..cursor + 16].try_into().unwrap());
            batches.push(Batch { amount, expiry_round: expiry });
            cursor += 16;
        }
    }

    Ok(DecodedUserState {
        version,
        username,
        batch_count,
        batches,
        encryption_pubkey,
        scan_pubkey,
        pq_pubkey_hash,
        bio,
    })
}

/// `true` iff all 32 bytes are zero — signals "not set" for key fields.
pub fn is_zero32(bytes: &[u8; 32]) -> bool {
    bytes.iter().all(|&b| b == 0)
}

/// Wallet credit-state box key = `"w:" || senderPubkey` (34 bytes).
pub fn wallet_box_key(sender_pubkey: &[u8; 32]) -> Vec<u8> {
    let mut key = Vec::with_capacity(34);
    key.extend_from_slice(b"w:");
    key.extend_from_slice(sender_pubkey);
    key
}

/// Name reverse-index box key = `"n:" || sha256(name)` (34 bytes).
pub fn name_box_key(name_utf8: &[u8]) -> Vec<u8> {
    let hash = Sha256::digest(name_utf8);
    let mut key = Vec::with_capacity(34);
    key.extend_from_slice(b"n:");
    key.extend_from_slice(&hash);
    key
}

/// Commitment box key = `"c:" || sha256(preimage)` (34 bytes).
pub fn commitment_box_key(preimage: &[u8]) -> Vec<u8> {
    let hash = Sha256::digest(preimage);
    let mut key = Vec::with_capacity(34);
    key.extend_from_slice(b"c:");
    key.extend_from_slice(&hash);
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v2 head only (102 bytes) — no bio slot.
    fn sample_bytes_v2() -> Vec<u8> {
        let mut b = vec![0u8; 102];
        b[0] = 2; // version
        // usernameOff=102, batchesOff=110 (no batches, past end but off+2 check handles it)
        b[1..3].copy_from_slice(&(102u16).to_be_bytes());
        b[3] = 0; // batchCount
        b[4..6].copy_from_slice(&(102u16).to_be_bytes());
        for i in 0..32 {
            b[6 + i] = 0xAA;
            b[38 + i] = 0xBB;
            b[70 + i] = 0xCC;
        }
        // username "bob" at offset 102: 2B len + payload
        b.extend_from_slice(&(3u16).to_be_bytes());
        b.extend_from_slice(b"bob");
        b
    }

    /// v3 head (104 bytes) — username + bio both present.
    fn sample_bytes_v3() -> Vec<u8> {
        let mut b = vec![0u8; 104];
        b[0] = 3; // version
        b[1..3].copy_from_slice(&(104u16).to_be_bytes()); // usernameOff
        b[3] = 0; // batchCount
        b[4..6].copy_from_slice(&(104u16).to_be_bytes()); // batchesOff (no batches)
        for i in 0..32 {
            b[6 + i] = 0xAA;
            b[38 + i] = 0xBB;
            b[70 + i] = 0xCC;
        }
        let bio_off = 104 + 2 + 3; // right after username's 2B len + "bob"
        b[102..104].copy_from_slice(&(bio_off as u16).to_be_bytes());
        // username "bob" at offset 104
        b.extend_from_slice(&(3u16).to_be_bytes());
        b.extend_from_slice(b"bob");
        // bio "hi there" at bio_off
        b.extend_from_slice(&(8u16).to_be_bytes());
        b.extend_from_slice(b"hi there");
        b
    }

    #[test]
    fn decodes_v2_static_fields_and_username_with_empty_bio() {
        let bytes = sample_bytes_v2();
        let decoded = decode_user_state(&bytes).unwrap();
        assert_eq!(decoded.version, 2);
        assert_eq!(decoded.encryption_pubkey, [0xAAu8; 32]);
        assert_eq!(decoded.scan_pubkey, [0xBBu8; 32]);
        assert_eq!(decoded.pq_pubkey_hash, [0xCCu8; 32]);
        assert_eq!(decoded.username, b"bob");
        assert!(decoded.bio.is_empty());
    }

    #[test]
    fn decodes_v3_username_and_bio() {
        let bytes = sample_bytes_v3();
        let decoded = decode_user_state(&bytes).unwrap();
        assert_eq!(decoded.version, 3);
        assert_eq!(decoded.username, b"bob");
        assert_eq!(decoded.bio, b"hi there");
    }

    #[test]
    fn rejects_too_short_input() {
        assert!(matches!(decode_user_state(&[0u8; 50]), Err(DecodeUserStateError::TooShort(50))));
    }

    /// An unrecognized version byte must NOT fail the whole decode — the
    /// static head (keys/username) still decodes correctly; only the bio
    /// slot is skipped. Regression guard for the 2026-08-23 bug where a
    /// too-strict version check broke username/key resolution entirely for
    /// a real account.
    #[test]
    fn unknown_version_still_decodes_head_fields_with_empty_bio() {
        let mut bytes = sample_bytes_v2();
        bytes[0] = 9;
        let decoded = decode_user_state(&bytes).unwrap();
        assert_eq!(decoded.encryption_pubkey, [0xAAu8; 32]);
        assert_eq!(decoded.username, b"bob");
        assert!(decoded.bio.is_empty());
    }

    /// A version=3 box that's too short for a bio slot (< 104 bytes) must
    /// still decode the head fields — it just can't have a bio, same
    /// reasoning as the unknown-version case above.
    #[test]
    fn v3_too_short_for_bio_slot_still_decodes_head_with_empty_bio() {
        let mut bytes = sample_bytes_v2(); // 102-byte v2-shaped body
        bytes[0] = 3; // claim v3, but body stays 102 bytes (no bio slot)
        let decoded = decode_user_state(&bytes).unwrap();
        assert_eq!(decoded.username, b"bob");
        assert!(decoded.bio.is_empty());
    }

    #[test]
    fn box_keys_have_expected_prefix_and_length() {
        let pubkey = [1u8; 32];
        let key = wallet_box_key(&pubkey);
        assert_eq!(key.len(), 34);
        assert_eq!(&key[..2], b"w:");

        let nkey = name_box_key(b"alice");
        assert_eq!(nkey.len(), 34);
        assert_eq!(&nkey[..2], b"n:");

        let ckey = commitment_box_key(&[9u8; 16]);
        assert_eq!(ckey.len(), 34);
        assert_eq!(&ckey[..2], b"c:");
    }

    #[test]
    fn is_zero32_detects_zero_and_nonzero() {
        assert!(is_zero32(&[0u8; 32]));
        let mut nonzero = [0u8; 32];
        nonzero[31] = 1;
        assert!(!is_zero32(&nonzero));
    }
}
