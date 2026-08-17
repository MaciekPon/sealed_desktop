//! Stateless LogicSig signer for the Sealed `TreasuryEscrow` contract
//! account, ported from `treasury_escrow_signer.dart`.
//!
//! `TreasuryEscrow` is a contract-account LogicSig (not delegated): the
//! LogicSig program bytes themselves form the account (its address is
//! `sha512_256("Program" || program)`), and "signing" a transaction sent
//! from it needs no Ed25519 signature — the wire format is just the program
//! bytes wrapped as `{lsig: {l: <prog>}, txn: <fields>}` in canonical
//! msgpack.

use super::address;
use super::msgpack::{pack_sorted_map, Field};
use base64::Engine;
use rmp::encode as rmp_enc;

/// Compiled TEAL bytes for the `TreasuryEscrow` LogicSig, ported from
/// `features/wallet/treasury_escrow_program.dart`'s `kTreasuryEscrowProgramB64`.
/// Source: `programs/sealed/out/TreasuryEscrow.teal` compiled via algod
/// (`/v2/teal/compile`) — deterministic output, one value shared across
/// MainNet and TestNet in the Dart source (this LogicSig account isn't
/// keyed to a specific App ID), matching the golden-vector checks in this
/// module's tests and the `escrowProgramB64`/`escrowAddress` recorded in
/// `programs/sealed/.mimc-testnet.json`.
///
/// **Fixed 2026-08-06:** this constant previously held a different, wrong
/// program (a stale value from early in the desktop port, address
/// `VQJ2L6FKQ2MYILEJJZJRU44DOWT7MRMNTLBHYKLKQXVSAM52LUNMW4XT6Q` — not the
/// canonical treasury escrow account any deployed Sealed contract
/// recognizes). Caught while auditing the mainnet cutover; unrelated to it.
pub const TREASURY_ESCROW_PROGRAM_B64: &str =
    "CzEQgQESQQA3MQcxABJBAC8xCEAAKjEBgZChDw5BACAxIDIDEkEAGDEJMgMSQQAQMRZAAAsyBIECEkEAA4EBQ4EAQw==";

/// Build the singleton escrow signer from the inlined compiled bytes.
/// Cheap to construct (one sha512_256 + base32 encode) — callers may build
/// it once and hold onto it for the life of a session.
pub fn build_treasury_escrow_signer() -> TreasuryEscrowSigner {
    let program = base64::engine::general_purpose::STANDARD
        .decode(TREASURY_ESCROW_PROGRAM_B64)
        .expect("TREASURY_ESCROW_PROGRAM_B64 is a valid, fixed base64 constant");
    TreasuryEscrowSigner::new(program)
}

pub struct TreasuryEscrowSigner {
    program: Vec<u8>,
    // Only read by this module's own `address_matches_golden_vector` test
    // (a `cfg(test)`-only call site, invisible to a plain `cargo build`).
    // Transaction building uses `address_pubkey` directly.
    #[allow(dead_code)]
    pub address: String,
    pub address_pubkey: [u8; 32],
}

impl TreasuryEscrowSigner {
    pub fn new(program: Vec<u8>) -> Self {
        let pubkey = address::logicsig_pubkey(&program);
        let addr = address::encode_address(&pubkey);
        Self {
            program,
            address: addr,
            address_pubkey: pubkey,
        }
    }

    /// Wrap an unsigned txn-field map as a signed-LogicSig blob:
    /// `{"lsig": {"l": program}, "txn": fields}` in canonical msgpack.
    pub fn encode_signed_txn(&self, txn_fields: &[(&str, Field)]) -> Vec<u8> {
        let mut buf = Vec::new();
        rmp_enc::write_map_len(&mut buf, 2).unwrap();
        rmp_enc::write_str(&mut buf, "lsig").unwrap();
        rmp_enc::write_map_len(&mut buf, 1).unwrap();
        rmp_enc::write_str(&mut buf, "l").unwrap();
        rmp_enc::write_bin(&mut buf, &self.program).unwrap();
        rmp_enc::write_str(&mut buf, "txn").unwrap();
        buf.extend_from_slice(&pack_sorted_map(txn_fields));
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const ESCROW_PROG_B64: &str = TREASURY_ESCROW_PROGRAM_B64;
    /// Matches `programs/sealed/.mimc-testnet.json`'s `escrowAddress` and
    /// the address `TreasuryEscrowSigner` derives on the mobile client from
    /// the same `kTreasuryEscrowProgramB64` bytes.
    const EXPECTED_ADDRESS: &str = "V3RW2DEEOBKA7KMWMHP5MDSJMFNINEY4IC755WV6VN54JJJBPSNLWOSLAE";

    fn program() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(ESCROW_PROG_B64)
            .unwrap()
    }

    #[test]
    fn address_matches_golden_vector() {
        let signer = TreasuryEscrowSigner::new(program());
        assert_eq!(signer.address, EXPECTED_ADDRESS);
        assert_eq!(signer.address_pubkey.len(), 32);
    }

    #[test]
    fn signed_txn_envelope_shape() {
        let signer = TreasuryEscrowSigner::new(program());
        let fields: Vec<(&str, Field)> = vec![
            ("fee", Field::UInt(1000)),
            ("fv", Field::UInt(1_000_000)),
            ("lv", Field::UInt(1_001_000)),
            ("snd", Field::Bin(signer.address_pubkey.to_vec())),
            ("rcv", Field::Bin(signer.address_pubkey.to_vec())),
            ("amt", Field::UInt(0)), // omitted by canonical encoding
            ("type", Field::Str("pay".into())),
        ];
        let blob = signer.encode_signed_txn(&fields);
        // 0x82 = fixmap with 2 entries ("lsig", "txn").
        assert_eq!(blob[0], 0x82);
    }
}
