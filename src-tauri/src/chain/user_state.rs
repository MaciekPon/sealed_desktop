//! On-chain `UserState` ARC4 tuple decoding + box-key derivation, ported
//! from the private helpers at the bottom of `sealed_chain_client.dart`.
//!
//! `UserState` ARC4 tuple shape:
//! `(uint8, byte[], uint8, (uint64,uint64)[], byte[32], byte[32], byte[32])`
//!
//! ```text
//! offset 0:   version      uint8      1B  static
//! offset 1:   usernameOff  uint16     2B  -> points to username length+data
//! offset 3:   batchCount   uint8      1B  static
//! offset 4:   batchesOff   uint16     2B  -> points to batches array
//! offset 6:   encPubkey    byte[32]  32B  static
//! offset 38:  scanPubkey   byte[32]  32B  static
//! offset 70:  pqPubkeyHash byte[32]  32B  static
//! Total head: 102 bytes
//! ```

use sha2::{Digest, Sha256};

pub struct Batch {
    pub amount: u64,
    pub expiry_round: u64,
}

pub struct DecodedUserState {
    pub version: u8,
    pub username: Vec<u8>,
    pub batch_count: u8,
    pub batches: Vec<Batch>,
    pub encryption_pubkey: [u8; 32],
    pub scan_pubkey: [u8; 32],
    pub pq_pubkey_hash: [u8; 32],
}

#[derive(Debug, thiserror::Error)]
#[error("UserState too short: {0} bytes (need >= 102)")]
pub struct UserStateTooShort(pub usize);

pub fn decode_user_state(bytes: &[u8]) -> Result<DecodedUserState, UserStateTooShort> {
    if bytes.len() < 102 {
        return Err(UserStateTooShort(bytes.len()));
    }

    let version = bytes[0];
    let username_off = u16::from_be_bytes([bytes[1], bytes[2]]) as usize;
    let batch_count = bytes[3];
    let batches_off = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    let mut encryption_pubkey = [0u8; 32];
    encryption_pubkey.copy_from_slice(&bytes[6..38]);
    let mut scan_pubkey = [0u8; 32];
    scan_pubkey.copy_from_slice(&bytes[38..70]);
    let mut pq_pubkey_hash = [0u8; 32];
    pq_pubkey_hash.copy_from_slice(&bytes[70..102]);

    let username = if username_off + 2 > bytes.len() {
        Vec::new()
    } else {
        let ulen = u16::from_be_bytes([bytes[username_off], bytes[username_off + 1]]) as usize;
        let uend = username_off + 2 + ulen;
        if uend <= bytes.len() {
            bytes[username_off + 2..uend].to_vec()
        } else {
            Vec::new()
        }
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

    fn sample_bytes() -> Vec<u8> {
        let mut b = vec![0u8; 102];
        b[0] = 1; // version
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

    #[test]
    fn decodes_static_fields_and_username() {
        let bytes = sample_bytes();
        let decoded = decode_user_state(&bytes).unwrap();
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded.encryption_pubkey, [0xAAu8; 32]);
        assert_eq!(decoded.scan_pubkey, [0xBBu8; 32]);
        assert_eq!(decoded.pq_pubkey_hash, [0xCCu8; 32]);
        assert_eq!(decoded.username, b"bob");
    }

    #[test]
    fn rejects_too_short_input() {
        assert!(decode_user_state(&[0u8; 50]).is_err());
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
