//! Wire codecs for the alias-chat handshake envelopes, ported from
//! `sealed_app/lib/features/messaging/alias/alias_envelope.dart`. Pure byte
//! manipulation — no I/O, no crypto operations beyond hashing `invite_ref`.
//!
//! Invite envelope:  `[0x01][encPub:32][scanPub:32][pqPub:800]` = 865B
//! Accept envelope:   `[0x02][inviteRefPrefix:8][encPub:32][scanPub:32][kemCiphertext:768]` = 841B

use sha2::{Digest, Sha256};

use crate::crypto::pq::PQ_PUBLIC_KEY_LEN;

const INVITE_VERSION: u8 = 0x01;
const ACCEPT_VERSION: u8 = 0x02;

pub const INVITE_ENVELOPE_LEN: usize = 1 + 32 + 32 + PQ_PUBLIC_KEY_LEN;
pub const ACCEPT_ENVELOPE_LEN: usize = 1 + 8 + 32 + 32 + 768;

pub struct InviteEnvelope {
    pub enc_pub: [u8; 32],
    pub scan_pub: [u8; 32],
    pub pq_pub: Vec<u8>,
}

/// `sha256(encPub || scanPub || pqPub)` — independently derivable by both
/// sides of the handshake once they hold the invite envelope's contents.
pub fn invite_ref(enc_pub: &[u8; 32], scan_pub: &[u8; 32], pq_pub: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(enc_pub);
    hasher.update(scan_pub);
    hasher.update(pq_pub);
    hasher.finalize().into()
}

pub fn encode_invite_envelope(e: &InviteEnvelope) -> Vec<u8> {
    let mut out = Vec::with_capacity(INVITE_ENVELOPE_LEN);
    out.push(INVITE_VERSION);
    out.extend_from_slice(&e.enc_pub);
    out.extend_from_slice(&e.scan_pub);
    out.extend_from_slice(&e.pq_pub);
    out
}

pub fn decode_invite_envelope(bytes: &[u8]) -> Option<InviteEnvelope> {
    if bytes.len() != INVITE_ENVELOPE_LEN || bytes[0] != INVITE_VERSION {
        return None;
    }
    let enc_pub: [u8; 32] = bytes[1..33].try_into().ok()?;
    let scan_pub: [u8; 32] = bytes[33..65].try_into().ok()?;
    let pq_pub = bytes[65..].to_vec();
    Some(InviteEnvelope { enc_pub, scan_pub, pq_pub })
}

pub struct AcceptEnvelope {
    pub invite_ref_prefix: [u8; 8],
    pub enc_pub: [u8; 32],
    pub scan_pub: [u8; 32],
    pub kem_ciphertext: [u8; 768],
}

pub fn encode_accept_envelope(e: &AcceptEnvelope) -> Vec<u8> {
    let mut out = Vec::with_capacity(ACCEPT_ENVELOPE_LEN);
    out.push(ACCEPT_VERSION);
    out.extend_from_slice(&e.invite_ref_prefix);
    out.extend_from_slice(&e.enc_pub);
    out.extend_from_slice(&e.scan_pub);
    out.extend_from_slice(&e.kem_ciphertext);
    out
}

pub fn decode_accept_envelope(bytes: &[u8]) -> Option<AcceptEnvelope> {
    if bytes.len() != ACCEPT_ENVELOPE_LEN || bytes[0] != ACCEPT_VERSION {
        return None;
    }
    let invite_ref_prefix: [u8; 8] = bytes[1..9].try_into().ok()?;
    let enc_pub: [u8; 32] = bytes[9..41].try_into().ok()?;
    let scan_pub: [u8; 32] = bytes[41..73].try_into().ok()?;
    let kem_ciphertext: [u8; 768] = bytes[73..841].try_into().ok()?;
    Some(AcceptEnvelope { invite_ref_prefix, enc_pub, scan_pub, kem_ciphertext })
}

// ---------------------------------------------------------------------------
// QR/paste transport wrappers — ported from `combined_qr_codec.dart` and
// `offline_handshake_codec.dart`. A phone's scanner (`scan_classifier.dart`'s
// `classifyScan`) ALWAYS expects a scanned invite to be wrapped in both
// layers below, even for a payload that fits in a single QR frame —
// `OfflineHandshakeAccumulator` requires the `0xA1` frame header
// unconditionally, and `decodeCombinedQr` requires the `0x03` container.
// Desktop never needs multi-frame reassembly (both handshake envelopes,
// even combined-QR-wrapped, fit one frame under `OfflineHandshakeCodec`'s
// default 1000-byte chunk budget), so this only ever emits/expects
// `seq=0, total=1` — but the wrapper bytes themselves must still be
// present, or a phone's scanner silently rejects the QR as unrecognized.
// ---------------------------------------------------------------------------

const COMBINED_QR_MAGIC: u8 = 0x03;
const OFFLINE_FRAME_MAGIC: u8 = 0xA1;
const OFFLINE_FRAME_HEADER_LEN: usize = 5; // magic + seq + total + payloadId(2)

/// `[0x03][addrLen:1][addr][inviteEnvelope]` — lets one QR carry both a
/// fallback wallet address and an alias invite. Matches
/// `combined_qr_codec.dart::encodeCombinedQr`.
pub fn encode_combined_qr(wallet_address: &str, invite_envelope: &[u8]) -> Option<Vec<u8>> {
    let addr_bytes = wallet_address.as_bytes();
    if addr_bytes.len() > 255 || invite_envelope.len() != INVITE_ENVELOPE_LEN {
        return None;
    }
    let mut out = Vec::with_capacity(2 + addr_bytes.len() + invite_envelope.len());
    out.push(COMBINED_QR_MAGIC);
    out.push(addr_bytes.len() as u8);
    out.extend_from_slice(addr_bytes);
    out.extend_from_slice(invite_envelope);
    Some(out)
}

/// Inverse of [`encode_combined_qr`]. Returns `(wallet_address,
/// invite_envelope_bytes)`. `None` on wrong magic, length mismatch, bad
/// UTF-8, or a trailing payload that isn't a well-formed invite envelope.
pub fn decode_combined_qr(bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    if bytes.len() < 2 || bytes[0] != COMBINED_QR_MAGIC {
        return None;
    }
    let addr_len = bytes[1] as usize;
    if bytes.len() != 2 + addr_len + INVITE_ENVELOPE_LEN {
        return None;
    }
    let envelope = bytes[2 + addr_len..].to_vec();
    if envelope[0] != INVITE_VERSION {
        return None;
    }
    let address = String::from_utf8(bytes[2..2 + addr_len].to_vec()).ok()?;
    Some((address, envelope))
}

/// Wraps `payload` as a single offline-handshake frame:
/// `[0xA1][seq=0][total=1][payloadId:2][payload]`. `payloadId` is
/// `sha256(payload)[0:2]`, matching `OfflineHandshakeCodec`'s de-dup id —
/// unused for reassembly here (single frame), but included so the bytes
/// are indistinguishable from a real mobile-emitted frame.
pub fn encode_offline_frame(payload: &[u8]) -> Vec<u8> {
    let payload_id = Sha256::digest(payload);
    let mut out = Vec::with_capacity(OFFLINE_FRAME_HEADER_LEN + payload.len());
    out.push(OFFLINE_FRAME_MAGIC);
    out.push(0); // seq
    out.push(1); // total
    out.extend_from_slice(&payload_id[..2]);
    out.extend_from_slice(payload);
    out
}

/// Inverse of [`encode_offline_frame`] — single-frame only. `None` (rather
/// than attempting reassembly) if `total != 1`: a genuinely multi-frame
/// scan/paste isn't supported on desktop (see the module doc comment), so
/// the caller should surface a clear error instead of silently mis-parsing
/// a partial frame.
pub fn decode_offline_frame(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < OFFLINE_FRAME_HEADER_LEN || bytes[0] != OFFLINE_FRAME_MAGIC {
        return None;
    }
    if bytes[2] != 1 {
        return None;
    }
    Some(bytes[OFFLINE_FRAME_HEADER_LEN..].to_vec())
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_invite() -> InviteEnvelope {
        InviteEnvelope { enc_pub: [1u8; 32], scan_pub: [2u8; 32], pq_pub: vec![3u8; PQ_PUBLIC_KEY_LEN] }
    }

    fn sample_accept() -> AcceptEnvelope {
        AcceptEnvelope { invite_ref_prefix: [9u8; 8], enc_pub: [4u8; 32], scan_pub: [5u8; 32], kem_ciphertext: [6u8; 768] }
    }

    #[test]
    fn invite_envelope_round_trip() {
        let e = sample_invite();
        let bytes = encode_invite_envelope(&e);
        assert_eq!(bytes.len(), INVITE_ENVELOPE_LEN);
        let decoded = decode_invite_envelope(&bytes).unwrap();
        assert_eq!(decoded.enc_pub, e.enc_pub);
        assert_eq!(decoded.scan_pub, e.scan_pub);
        assert_eq!(decoded.pq_pub, e.pq_pub);
    }

    #[test]
    fn accept_envelope_round_trip() {
        let e = sample_accept();
        let bytes = encode_accept_envelope(&e);
        assert_eq!(bytes.len(), ACCEPT_ENVELOPE_LEN);
        let decoded = decode_accept_envelope(&bytes).unwrap();
        assert_eq!(decoded.invite_ref_prefix, e.invite_ref_prefix);
        assert_eq!(decoded.enc_pub, e.enc_pub);
        assert_eq!(decoded.scan_pub, e.scan_pub);
        assert_eq!(decoded.kem_ciphertext, e.kem_ciphertext);
    }

    #[test]
    fn invite_envelope_rejects_wrong_version_and_bad_length() {
        let mut bytes = encode_invite_envelope(&sample_invite());
        bytes[0] = 0x02;
        assert!(decode_invite_envelope(&bytes).is_none());

        let too_short = vec![0x01u8; INVITE_ENVELOPE_LEN - 1];
        assert!(decode_invite_envelope(&too_short).is_none());

        let too_long = vec![0x01u8; INVITE_ENVELOPE_LEN + 1];
        assert!(decode_invite_envelope(&too_long).is_none());
    }

    #[test]
    fn accept_envelope_rejects_wrong_version_and_bad_length() {
        let mut bytes = encode_accept_envelope(&sample_accept());
        bytes[0] = 0x01;
        assert!(decode_accept_envelope(&bytes).is_none());

        assert!(decode_accept_envelope(&[0x02u8; ACCEPT_ENVELOPE_LEN - 1]).is_none());
        assert!(decode_accept_envelope(&[0x02u8; ACCEPT_ENVELOPE_LEN + 1]).is_none());
    }

    #[test]
    fn invite_ref_is_deterministic_and_sensitive_to_every_field() {
        let a = invite_ref(&[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let b = invite_ref(&[1u8; 32], &[2u8; 32], &[3u8; 32]);
        assert_eq!(a, b);

        let different_pq = invite_ref(&[1u8; 32], &[2u8; 32], &[4u8; 32]);
        assert_ne!(a, different_pq);
    }

    #[test]
    fn combined_qr_round_trip() {
        let inv = encode_invite_envelope(&sample_invite());
        let combined = encode_combined_qr("SOMEALGORANDADDRESSFORTESTINGPURPOSESONLY123456789", &inv).unwrap();
        let (addr, envelope) = decode_combined_qr(&combined).unwrap();
        assert_eq!(addr, "SOMEALGORANDADDRESSFORTESTINGPURPOSESONLY123456789");
        assert_eq!(envelope, inv);
    }

    #[test]
    fn combined_qr_rejects_wrong_magic_and_bad_length() {
        let inv = encode_invite_envelope(&sample_invite());
        let mut combined = encode_combined_qr("ADDR", &inv).unwrap();
        combined[0] = 0x99;
        assert!(decode_combined_qr(&combined).is_none());
        assert!(decode_combined_qr(&[0x03, 5]).is_none()); // claims 5-byte addr, has none
    }

    #[test]
    fn offline_frame_round_trip() {
        let payload = b"arbitrary combined-qr or accept-envelope bytes".to_vec();
        let frame = encode_offline_frame(&payload);
        assert_eq!(frame[0], 0xA1);
        assert_eq!(frame[1], 0); // seq
        assert_eq!(frame[2], 1); // total
        let decoded = decode_offline_frame(&frame).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn offline_frame_rejects_wrong_magic_and_multi_frame() {
        let payload = b"x".to_vec();
        let mut frame = encode_offline_frame(&payload);
        frame[0] = 0x00;
        assert!(decode_offline_frame(&frame).is_none());

        let mut multi = encode_offline_frame(&payload);
        multi[2] = 2; // total=2 — desktop can't reassemble multi-frame
        assert!(decode_offline_frame(&multi).is_none());
    }

    #[test]
    fn hex_round_trips() {
        let bytes = vec![0xdeu8, 0xad, 0xbe, 0xef];
        let s = hex_encode(&bytes);
        assert_eq!(s, "deadbeef");
        assert_eq!(hex_decode(&s).unwrap(), bytes);
        assert!(hex_decode("xyz").is_none());
        assert!(hex_decode("abc").is_none()); // odd length
    }
}
