//! Algorand transaction group construction, ported from the group/txn
//! building + hashing logic in `sealed_chain_client.dart`. Sends are 2-txn
//! groups: a `TreasuryEscrow` self-payment (txn 0, fee bumped to cover both)
//! and an app-call NoOp (txn 1, fee=0, covered by txn 0's fee-pool surplus).

use rmp::encode as rmp_enc;
use sha2::{Digest, Sha512_256};

use super::msgpack::{pack_sorted_map, Field};

/// ARC4 method selector = `sha512_256(ascii(signature))[0..4]`. Computed on
/// demand rather than hardcoded, so it's correct by construction against
/// the ARC4 spec formula (cross-checked against the golden selector bytes
/// hardcoded in the Dart source — see the test below).
pub fn abi_selector(signature: &str) -> [u8; 4] {
    let digest = Sha512_256::digest(signature.as_bytes());
    [digest[0], digest[1], digest[2], digest[3]]
}

pub struct SuggestedParams {
    pub min_fee: u64,
    pub first_valid: u64,
    pub last_valid: u64,
    pub genesis_id: String,
    pub genesis_hash: [u8; 32],
}

/// Owned transaction-field map (String keys, since callers build these
/// dynamically — `pack_sorted_map` only needs `&str` at pack time).
pub type TxnFields = Vec<(&'static str, Field)>;

pub fn build_escrow_self_pay_txn(escrow_pubkey: &[u8; 32], fee: u64, params: &SuggestedParams) -> TxnFields {
    vec![
        ("fee", Field::UInt(fee)),
        ("fv", Field::UInt(params.first_valid)),
        ("gen", Field::Str(params.genesis_id.clone())),
        ("gh", Field::Bin(params.genesis_hash.to_vec())),
        ("lv", Field::UInt(params.last_valid)),
        ("rcv", Field::Bin(escrow_pubkey.to_vec())),
        ("snd", Field::Bin(escrow_pubkey.to_vec())),
        ("type", Field::Str("pay".into())),
        // 'amt' omitted — canonical encoding strips zero ints.
    ]
}

pub fn build_app_call_txn(
    sender_pubkey: &[u8; 32],
    app_id: u64,
    method_selector: [u8; 4],
    app_args: Vec<Vec<u8>>,
    fee: u64,
    params: &SuggestedParams,
) -> TxnFields {
    let mut apaa = vec![method_selector.to_vec()];
    apaa.extend(app_args);
    vec![
        ("apid", Field::UInt(app_id)),
        ("apaa", Field::BinList(apaa)),
        ("fee", Field::UInt(fee)),
        ("fv", Field::UInt(params.first_valid)),
        ("gen", Field::Str(params.genesis_id.clone())),
        ("gh", Field::Bin(params.genesis_hash.to_vec())),
        ("lv", Field::UInt(params.last_valid)),
        ("snd", Field::Bin(sender_pubkey.to_vec())),
        ("type", Field::Str("appl".into())),
    ]
}

pub fn build_app_call_txn_with_boxes(
    sender_pubkey: &[u8; 32],
    app_id: u64,
    method_selector: [u8; 4],
    app_args: Vec<Vec<u8>>,
    boxes: Vec<(u64, Vec<u8>)>,
    fee: u64,
    params: &SuggestedParams,
) -> TxnFields {
    let mut fields = build_app_call_txn(sender_pubkey, app_id, method_selector, app_args, fee, params);
    fields.push(("apbx", Field::Boxes(boxes)));
    fields
}

pub(crate) fn encode_tx_for_signing(fields: &TxnFields) -> Vec<u8> {
    let mut buf = Vec::from(*b"TX");
    buf.extend_from_slice(&pack_sorted_map(fields));
    buf
}

/// Wraps unsigned txn fields as a bare `{"txn": {...}}` map — the shape a
/// `txn-groups[].txns[]` entry must have when `allow-empty-signatures: true`
/// lets the `sig` key be omitted entirely. This is **not** the same
/// encoding as [`encode_tx_for_signing`]: that function's `"TX" ||
/// msgpack(fields)` is only ever valid as an ed25519 signing preimage /
/// txid hash input — its leading `"TX"` bytes are not valid msgpack framing.
fn encode_unsigned_txn_for_simulate(fields: &TxnFields) -> Vec<u8> {
    let mut buf = Vec::new();
    rmp_enc::write_map_len(&mut buf, 1).unwrap();
    rmp_enc::write_str(&mut buf, "txn").unwrap();
    buf.extend_from_slice(&pack_sorted_map(fields));
    buf
}

/// Build the full raw-msgpack body for `POST /v2/transactions/simulate`
/// (`Content-Type: application/msgpack`) — confirmed live against both
/// AlgoNode and Nodely testnet algod that this is the *only* format they
/// accept for this endpoint today. The JSON-with-base64-txns shape older
/// algod docs describe (`{"txn-groups": [{"txns": ["<base64>"]}], ...}`)
/// is rejected outright: `{"message":"failed to decode object: json decode
/// error [...]: only encoded map or array can be decoded into a struct"}` —
/// because algod expects each `txns[]` entry to be a msgpack map value
/// spliced in directly (via [`encode_unsigned_txn_for_simulate`]), not a
/// base64 string or a `bin`-wrapped blob, and because `exec-trace-config`
/// must be present (as an empty map) even though we don't use it.
///
/// Field order matches what `algosdk`'s `SimulateRequest` encoder produces
/// (alphabetical by key — verified byte-for-byte against a live capture),
/// though msgpack map decoding shouldn't itself be order-sensitive.
pub(crate) fn encode_simulate_request(app_call_txn: &TxnFields) -> Vec<u8> {
    let inner_txn = encode_unsigned_txn_for_simulate(app_call_txn);

    let mut out = Vec::new();
    rmp_enc::write_map_len(&mut out, 4).unwrap();

    rmp_enc::write_str(&mut out, "allow-empty-signatures").unwrap();
    rmp_enc::write_bool(&mut out, true).unwrap();

    rmp_enc::write_str(&mut out, "allow-unnamed-resources").unwrap();
    rmp_enc::write_bool(&mut out, true).unwrap();

    rmp_enc::write_str(&mut out, "exec-trace-config").unwrap();
    rmp_enc::write_map_len(&mut out, 0).unwrap();

    rmp_enc::write_str(&mut out, "txn-groups").unwrap();
    rmp_enc::write_array_len(&mut out, 1).unwrap();
    rmp_enc::write_map_len(&mut out, 1).unwrap();
    rmp_enc::write_str(&mut out, "txns").unwrap();
    rmp_enc::write_array_len(&mut out, 1).unwrap();
    out.extend_from_slice(&inner_txn);

    out
}

/// `rawTxID(t) = sha512_256("TX" || msgpack(t_without_grp))`. Callers pass
/// the txn fields *before* a `grp` entry has been added.
fn compute_tx_id_raw(fields: &TxnFields) -> [u8; 32] {
    Sha512_256::digest(encode_tx_for_signing(fields)).into()
}

/// `Group ID = sha512_256("TG" || msgpack({"txlist": [rawTxID(t0), rawTxID(t1)]}))`.
pub fn compute_group_id(txns: &[TxnFields]) -> [u8; 32] {
    let txids: Vec<[u8; 32]> = txns.iter().map(compute_tx_id_raw).collect();

    let mut body = Vec::new();
    rmp_enc::write_map_len(&mut body, 1).unwrap();
    rmp_enc::write_str(&mut body, "txlist").unwrap();
    rmp_enc::write_array_len(&mut body, txids.len() as u32).unwrap();
    for txid in &txids {
        rmp_enc::write_bin(&mut body, txid).unwrap();
    }

    let mut buf = Vec::from(*b"TG");
    buf.extend_from_slice(&body);
    Sha512_256::digest(buf).into()
}

/// Base32-no-pad TxID for an already-grouped (has `grp` set) txn.
pub fn compute_tx_id(grouped_fields: &TxnFields) -> String {
    let hash: [u8; 32] = Sha512_256::digest(encode_tx_for_signing(grouped_fields)).into();
    super::address::base32_encode_nopad(&hash)
}

/// Signed-txn envelope for a wallet (Ed25519) signer: `{sig, txn}` in
/// canonical msgpack. Mirrors `_encodeSignedTxWithEd25519`.
pub fn encode_signed_tx_with_ed25519(fields: &TxnFields, sig: &[u8; 64]) -> Vec<u8> {
    let mut buf = Vec::new();
    rmp_enc::write_map_len(&mut buf, 2).unwrap();
    rmp_enc::write_str(&mut buf, "sig").unwrap();
    rmp_enc::write_bin(&mut buf, sig).unwrap();
    rmp_enc::write_str(&mut buf, "txn").unwrap();
    buf.extend_from_slice(&pack_sorted_map(fields));
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::address;

    const ESCROW_ADDRESS: &str = "VQJ2L6FKQ2MYILEJJZJRU44DOWT7MRMNTLBHYKLKQXVSAM52LUNMW4XT6Q";

    /// Golden vector from `sealed_app/test/chain/sealed_chain_client_test.dart`
    /// ("group ID matches algosdk-js computeGroupID").
    #[test]
    fn group_id_matches_golden_vector() {
        let sender_pk = [0x11u8; 32];
        let escrow_pk = address::decode_address(ESCROW_ADDRESS).unwrap();
        let recipient_tag = [0x22u8; 32];
        let sender_eph = [0x33u8; 32];
        let ct = b"hello";
        let mut framed = sender_eph.to_vec();
        framed.extend_from_slice(ct);

        let genesis_hash: [u8; 32] = base64_decode_32("SGO1GKSzyE7IEPItTxCByw9x8FmnrCDexi9/cOUJOiI=");
        let params = SuggestedParams {
            min_fee: 1000,
            first_valid: 1_000_000,
            last_valid: 1_001_000,
            genesis_id: "testnet-v1.0".to_string(),
            genesis_hash,
        };

        let escrow_txn = build_escrow_self_pay_txn(&escrow_pk, params.min_fee * 2, &params);
        let send_message_selector = abi_selector("sendMessage(byte[32],byte[])void");
        let app_args = vec![
            recipient_tag.to_vec(),
            super::super::msgpack::encode_abi_dynamic_bytes(&framed),
        ];
        let app_call_txn = build_app_call_txn(&sender_pk, 762153589, send_message_selector, app_args, 0, &params);

        let group_id = compute_group_id(&[escrow_txn, app_call_txn]);
        let hex: String = group_id.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "3d11b611b2c098b5dc08c4c325a250907c1ece72db7a2b45de4d8da3a1b90e52");
    }

    #[test]
    fn selectors_match_golden_bytes_from_dart() {
        // Hardcoded byte arrays copied from `SealedChainClient` constants —
        // confirms `abi_selector` (computed fresh here) agrees with them.
        assert_eq!(abi_selector("claimUsername(byte[])void"), [0x27, 0x34, 0x39, 0x94]);
        assert_eq!(abi_selector("releaseUsername()void"), [0xec, 0x03, 0xaf, 0x64]);
        assert_eq!(abi_selector("redeem(byte[],byte[])void"), [0x85, 0x60, 0xbf, 0x42]);
        assert_eq!(abi_selector("getCredits(address)uint64"), [0xe1, 0x43, 0x9c, 0x0c]);
        assert_eq!(abi_selector("sendMessage(byte[32],byte[])void"), [0x05, 0x20, 0xee, 0xbb]);
        assert_eq!(abi_selector("sendAliasMessage(byte[32],byte[32],byte[])void"), [0x5d, 0x7a, 0x12, 0x65]);
        assert_eq!(abi_selector("createChannel(byte[32],byte[32])void"), [0xe8, 0x60, 0x0f, 0x7a]);
        assert_eq!(abi_selector("acceptChannel(byte[32],byte[32])void"), [0xda, 0xf1, 0xf7, 0x94]);
        assert_eq!(abi_selector("deleteChannel(byte[32])void"), [0x19, 0x78, 0x33, 0x3e]);
        assert_eq!(abi_selector("readChannel(byte[32])byte[]"), [0x26, 0xc7, 0x81, 0x1a]);
        assert_eq!(abi_selector("publishKeys(byte[32],byte[32],byte[])void"), [0xe2, 0x8c, 0x04, 0x34]);
    }

    fn base64_decode_32(s: &str) -> [u8; 32] {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(s).unwrap();
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        out
    }

    /// Regression test for a real bug: `simulate_abi_return` used to feed
    /// `encode_tx_for_signing`'s `"TX" || msgpack(fields)` signing preimage
    /// into algod's `/v2/transactions/simulate` `txns[]` array, which algod
    /// rejects with a 400 ("only encoded map or array can be decoded into a
    /// struct") because a leading `"TX"` isn't valid msgpack map/array
    /// framing. `encode_unsigned_txn_for_simulate` must produce a bare
    /// `{"txn": {...}}` map instead.
    #[test]
    fn encode_unsigned_txn_for_simulate_wraps_fields_under_txn_key() {
        let sender_pk = [0x11u8; 32];
        let params = SuggestedParams {
            min_fee: 1000,
            first_valid: 1,
            last_valid: 1001,
            genesis_id: "testnet-v1.0".to_string(),
            genesis_hash: [0u8; 32],
        };
        let selector = abi_selector("getCredits(address)uint64");
        let fields = build_app_call_txn_with_boxes(&sender_pk, 1, selector, vec![sender_pk.to_vec()], vec![], 0, &params);

        let encoded = encode_unsigned_txn_for_simulate(&fields);

        // fixmap(1 entry) 0x81, then msgpack fixstr(3) "txn" = 0xa3 't' 'x' 'n'.
        assert_eq!(&encoded[..5], &[0x81, 0xa3, b't', b'x', b'n']);
        // Must not be the "TX"-prefixed signing-preimage shape.
        assert_ne!(&encoded[..2], b"TX");
    }

    /// Regression test for a second, larger bug found alongside the one
    /// above: even wrapped correctly under `"txn"`, live algod (both
    /// AlgoNode and Nodely testnet) rejects the JSON-with-base64-txns
    /// `/v2/transactions/simulate` body entirely — confirmed with a live
    /// call during development. The endpoint only accepts a single raw
    /// msgpack document (`Content-Type: application/msgpack`) with each
    /// `txns[]` entry spliced in as a *nested msgpack map*, plus a required
    /// (if empty) `exec-trace-config` key. This locks in the exact
    /// structure confirmed live-working, byte-for-byte against a captured
    /// reference request built from `py-algorand-sdk`'s own
    /// `SimulateRequest` encoder.
    #[test]
    fn encode_simulate_request_matches_live_confirmed_shape() {
        let sender_pk = [0x11u8; 32];
        let params = SuggestedParams {
            min_fee: 1000,
            first_valid: 1,
            last_valid: 1001,
            genesis_id: "testnet-v1.0".to_string(),
            genesis_hash: [0u8; 32],
        };
        let selector = abi_selector("getCredits(address)uint64");
        let fields = build_app_call_txn_with_boxes(&sender_pk, 1, selector, vec![sender_pk.to_vec()], vec![], 0, &params);

        let request = encode_simulate_request(&fields);

        // fixmap(4 entries) = 0x84, then keys in alphabetical order (matches
        // algosdk's own SimulateRequest encoder, verified against a live capture).
        let mut expected_key = Vec::new();
        rmp_enc::write_map_len(&mut expected_key, 4).unwrap();
        rmp_enc::write_str(&mut expected_key, "allow-empty-signatures").unwrap();
        assert!(request.starts_with(&expected_key));

        // "exec-trace-config" must be present, and empty (fixmap 0) —
        // omitting this key entirely reproduces the live 400 this test guards against.
        let mut exec_trace_key = Vec::new();
        rmp_enc::write_str(&mut exec_trace_key, "exec-trace-config").unwrap();
        let pos = request.windows(exec_trace_key.len()).position(|w| w == exec_trace_key.as_slice()).expect("exec-trace-config key present");
        let mut empty_map = Vec::new();
        rmp_enc::write_map_len(&mut empty_map, 0).unwrap();
        assert_eq!(&request[pos + exec_trace_key.len()..pos + exec_trace_key.len() + empty_map.len()], empty_map.as_slice());

        // The inner txn must be spliced in as a nested map (the same
        // `{"txn": {...}}` bytes `encode_unsigned_txn_for_simulate` produces
        // standalone), not wrapped in a msgpack `bin` header — i.e. it must
        // appear verbatim somewhere in the outer request.
        let inner = encode_unsigned_txn_for_simulate(&fields);
        assert!(request.windows(inner.len()).any(|w| w == inner.as_slice()), "inner txn map must appear unwrapped");
    }
}
