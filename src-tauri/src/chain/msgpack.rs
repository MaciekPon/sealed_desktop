//! Canonical msgpack transaction-field encoding, ported from
//! `_packSortedMap` (duplicated identically in `sealed_chain_client.dart`
//! and `treasury_escrow_signer.dart`): sorted keys, zero/empty/null values
//! omitted. This is what makes the encoding match Algorand's canonical
//! transaction msgpack format (and therefore what `algod` / `algosdk-js`
//! compute the same group-id/txid hashes over).

use rmp::encode as rmp_enc;

/// A single transaction field value. Variants mirror the Dart source's
/// `if (v is int) / (v is String) / (v is Uint8List) / (v is List<Uint8List>)`
/// branches, plus a specialized box-ref list for `apbx`.
pub enum Field {
    UInt(u64),
    Str(String),
    Bin(Vec<u8>),
    BinList(Vec<Vec<u8>>),
    /// `apbx`: box references, each `(app_index, box_name)`. Encoded as a
    /// list of canonical 2-field maps `{i, n}` — since every box ref in
    /// this codebase targets app index 0 (self), the `i` field is always
    /// omitted by the same zero-omission rule applied recursively.
    Boxes(Vec<(u64, Vec<u8>)>),
}

fn is_empty(value: &Field) -> bool {
    match value {
        Field::UInt(v) => *v == 0,
        Field::Str(s) => s.is_empty(),
        Field::Bin(b) => b.is_empty(),
        Field::BinList(items) => items.is_empty(),
        Field::Boxes(boxes) => boxes.is_empty(),
    }
}

/// Canonical msgpack-encode a transaction-field map: sort by key, omit any
/// entry whose value is the zero/empty value for its type, recurse for
/// nested box-ref maps.
pub fn pack_sorted_map(fields: &[(&str, Field)]) -> Vec<u8> {
    let mut entries: Vec<&(&str, Field)> = fields.iter().filter(|(_, v)| !is_empty(v)).collect();
    entries.sort_by_key(|(k, _)| *k);

    let mut buf = Vec::new();
    rmp_enc::write_map_len(&mut buf, entries.len() as u32).expect("Vec<u8> write cannot fail");
    for (key, value) in entries {
        rmp_enc::write_str(&mut buf, key).expect("Vec<u8> write cannot fail");
        write_field(&mut buf, value);
    }
    buf
}

fn write_field(buf: &mut Vec<u8>, value: &Field) {
    match value {
        Field::UInt(v) => {
            rmp_enc::write_uint(buf, *v).expect("Vec<u8> write cannot fail");
        }
        Field::Str(s) => {
            rmp_enc::write_str(buf, s).expect("Vec<u8> write cannot fail");
        }
        Field::Bin(b) => {
            rmp_enc::write_bin(buf, b).expect("Vec<u8> write cannot fail");
        }
        Field::BinList(items) => {
            rmp_enc::write_array_len(buf, items.len() as u32).expect("Vec<u8> write cannot fail");
            for item in items {
                rmp_enc::write_bin(buf, item).expect("Vec<u8> write cannot fail");
            }
        }
        Field::Boxes(boxes) => {
            rmp_enc::write_array_len(buf, boxes.len() as u32).expect("Vec<u8> write cannot fail");
            for (app_index, name) in boxes {
                let sub = pack_sorted_map(&[
                    ("i", Field::UInt(*app_index)),
                    ("n", Field::Bin(name.clone())),
                ]);
                buf.extend_from_slice(&sub);
            }
        }
    }
}

/// ABI dynamic `byte[]` encoding: 2-byte big-endian length prefix + payload.
/// Mirrors `_encodeAbiDynamicBytes`.
pub fn encode_abi_dynamic_bytes(data: &[u8]) -> Vec<u8> {
    let len = data.len() as u16;
    let mut out = Vec::with_capacity(2 + data.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omits_zero_and_empty_fields() {
        let packed = pack_sorted_map(&[
            ("amt", Field::UInt(0)),
            ("fee", Field::UInt(1000)),
            ("note", Field::Bin(vec![])),
            ("type", Field::Str("pay".into())),
        ]);
        // fixmap with 2 entries (0x80 | 2 = 0x82), amt/note dropped.
        assert_eq!(packed[0], 0x82);
    }

    #[test]
    fn sorts_keys_lexicographically() {
        let packed = pack_sorted_map(&[("snd", Field::UInt(1)), ("fee", Field::UInt(2))]);
        // First key after the map-len byte should be "fee" (fixstr 0xa3 'f' 'e' 'e').
        assert_eq!(&packed[1..5], b"\xa3fee");
    }

    #[test]
    fn box_ref_with_zero_app_index_omits_i_field() {
        let packed = pack_sorted_map(&[(
            "apbx",
            Field::Boxes(vec![(0, b"abc".to_vec())]),
        )]);
        // outer map: 1 entry (apbx). array of 1. inner map: 1 entry (n only).
        assert_eq!(packed[0], 0x81); // fixmap, 1 entry
        // skip "apbx" fixstr key (0xa4 + 4 bytes)
        let after_key = 1 + 5;
        assert_eq!(packed[after_key], 0x91); // fixarray, 1 entry
        assert_eq!(packed[after_key + 1], 0x81); // inner fixmap, 1 entry (i omitted)
    }

    #[test]
    fn abi_dynamic_bytes_prefixes_length() {
        let encoded = encode_abi_dynamic_bytes(&[0u8; 40]);
        assert_eq!(encoded.len(), 42);
        assert_eq!(encoded[0], 0x00);
        assert_eq!(encoded[1], 40);
    }
}
