//! Algorand address + base32 encoding, ported from `chain_address.dart` /
//! the address helpers duplicated across `sealed_chain_client.dart` and
//! `treasury_escrow_signer.dart`.
//!
//! Note on a discrepancy found while porting: `sealed_chain_client.dart`'s
//! private `_encodeAlgorandAddress` (used only by `getUserByUsername`'s
//! box-value decode) takes the checksum from `sha512_256(pubkey)[0..4]`
//! (first 4 bytes), while `AlgorandWallet._addressFromPublicKey` (via
//! `AlgoAddrEncoder`, the real wallet-address path) and
//! `TreasuryEscrowSigner._encodeAlgoAddr` both use `[28..32]` (last 4 bytes)
//! — the actual Algorand spec. Since address *decoding* never validates the
//! checksum anywhere in the mobile app, this is silent and only cosmetic
//! within that one call site, but it would produce a wallet-address string
//! that doesn't match the same account's address as reported elsewhere in
//! the app. This port implements the spec-correct (last-4-bytes) encoder
//! uniformly; flag for follow-up if `getUserByUsername`-derived address
//! strings ever need bug-for-bug parity with mobile.

use sha2::{Digest, Sha512_256};

const B32_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// RFC 4648 base32 encode, no padding.
pub fn base32_encode_nopad(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for &b in bytes {
        buffer = (buffer << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(B32_ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(B32_ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// RFC 4648 base32 decode. Skips any character not in the alphabet (mirrors
/// the Dart decoder, which does the same to tolerate missing padding).
pub fn base32_decode(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for c in s.chars() {
        let Some(idx) = B32_ALPHABET.iter().position(|&a| a as char == c) else {
            continue;
        };
        buffer = (buffer << 5) | idx as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    out
}

/// Encode a 32-byte value (an Ed25519 public key, or a LogicSig hash — both
/// are treated identically by the address format) as an Algorand address:
/// `base32_nopad(value || sha512_256(value)[28..32])`.
pub fn encode_address(value: &[u8; 32]) -> String {
    let checksum = Sha512_256::digest(value);
    let mut raw = [0u8; 36];
    raw[..32].copy_from_slice(value);
    raw[32..].copy_from_slice(&checksum[28..32]);
    base32_encode_nopad(&raw)
}

/// Decode an Algorand address string back to its 32-byte payload. Does not
/// verify the checksum (mirrors every decode call site in the Dart source,
/// none of which validate it either).
pub fn decode_address(address: &str) -> Option<[u8; 32]> {
    let raw = base32_decode(address);
    if raw.len() < 36 {
        return None;
    }
    let mut value = [0u8; 32];
    value.copy_from_slice(&raw[..32]);
    Some(value)
}

/// LogicSig pubkey = `sha512_256("Program" || program_bytes)`. Mirrors
/// `TreasuryEscrowSigner._logicSigPubkey`.
pub fn logicsig_pubkey(program: &[u8]) -> [u8; 32] {
    let mut hasher = Sha512_256::new();
    hasher.update(b"Program");
    hasher.update(program);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    /// Golden vector from `sealed_app/test/chain/treasury_escrow_signer_test.dart`:
    /// compiled TreasuryEscrow.teal -> LogicSig address, cross-checked there
    /// against `algosdk-js LogicSigAccount(prog).address()`.
    const ESCROW_PROG_B64: &str = "CzEQgQESQQA2MQcxABJBAC4xCEAAKTEBgZBODkEAIDEgMgMSQQAYMQkyAxJBABAxFkAACzIEgQISQQADgQFDgQBD";
    const EXPECTED_ESCROW_ADDRESS: &str = "VQJ2L6FKQ2MYILEJJZJRU44DOWT7MRMNTLBHYKLKQXVSAM52LUNMW4XT6Q";

    #[test]
    fn logicsig_address_matches_golden_vector() {
        let program = base64::engine::general_purpose::STANDARD
            .decode(ESCROW_PROG_B64)
            .unwrap();
        let pubkey = logicsig_pubkey(&program);
        let address = encode_address(&pubkey);
        assert_eq!(address, EXPECTED_ESCROW_ADDRESS);
    }

    #[test]
    fn base32_round_trip() {
        let bytes: Vec<u8> = (0..40u8).collect();
        let encoded = base32_encode_nopad(&bytes);
        assert!(encoded.chars().all(|c| B32_ALPHABET.contains(&(c as u8))));
        let decoded = base32_decode(&encoded);
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn address_round_trips_ignoring_checksum() {
        let pubkey = [0x42u8; 32];
        let addr = encode_address(&pubkey);
        assert_eq!(addr.len(), 58);
        let decoded = decode_address(&addr).unwrap();
        assert_eq!(decoded, pubkey);
    }
}
