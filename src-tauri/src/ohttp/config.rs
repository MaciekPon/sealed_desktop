//! Parsed OHTTP gateway key configuration (RFC 9458 §3), ported from
//! `remote/ohttp/ohttp_config.dart`.
//!
//! Binary format:
//! `keyId(1) | kemId(2 BE) | publicKey(variable) | symmetric_algorithms_length(2 BE) | [kdfId(2 BE) | aeadId(2 BE)]*`
//!
//! We use the first symmetric algorithm pair, same as the Dart source.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OhttpConfigError {
    #[error("OHTTP config too short")]
    TooShort,
    #[error("OHTTP config too short for KEM {kem_id:#06x} (need {need} bytes, got {got})")]
    TooShortForKem { kem_id: u16, need: usize, got: usize },
    #[error("OHTTP config: no symmetric algorithms present")]
    NoSymmetricAlgorithms,
    #[error("unsupported KEM ID: {0:#06x}")]
    UnsupportedKem(u16),
}

#[derive(Clone)]
pub struct OhttpConfig {
    pub key_id: u8,
    pub kem_id: u16,
    pub kdf_id: u16,
    pub aead_id: u16,
    pub public_key: Vec<u8>,
}

impl OhttpConfig {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, OhttpConfigError> {
        if bytes.len() < 7 {
            return Err(OhttpConfigError::TooShort);
        }

        let mut offset = 0usize;
        let key_id = bytes[offset];
        offset += 1;

        let kem_id = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        offset += 2;

        let pub_key_size = kem_public_key_size(kem_id)?;
        if bytes.len() < offset + pub_key_size + 4 {
            return Err(OhttpConfigError::TooShortForKem {
                kem_id,
                need: offset + pub_key_size + 4,
                got: bytes.len(),
            });
        }

        let public_key = bytes[offset..offset + pub_key_size].to_vec();
        offset += pub_key_size;

        let sym_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        offset += 2;

        if sym_len < 4 || bytes.len() < offset + 4 {
            return Err(OhttpConfigError::NoSymmetricAlgorithms);
        }

        let kdf_id = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        let aead_id = u16::from_be_bytes([bytes[offset + 2], bytes[offset + 3]]);

        Ok(Self {
            key_id,
            kem_id,
            kdf_id,
            aead_id,
            public_key,
        })
    }
}

fn kem_public_key_size(kem_id: u16) -> Result<usize, OhttpConfigError> {
    match kem_id {
        0x0020 => Ok(32), // DHKEM(X25519, HKDF-SHA256)
        0x0021 => Ok(32), // DHKEM(X25519, HKDF-SHA512)
        0x0010 => Ok(65), // DHKEM(P-256, HKDF-SHA256)
        0x0011 => Ok(97), // DHKEM(P-384, HKDF-SHA384)
        0x0012 => Ok(133), // DHKEM(P-521, HKDF-SHA512)
        other => Err(OhttpConfigError::UnsupportedKem(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_x25519_config() {
        let mut bytes = vec![0x01u8];
        bytes.extend_from_slice(&[0x00, 0x20]);
        bytes.extend(std::iter::repeat_n(0xABu8, 32));
        bytes.extend_from_slice(&[0x00, 0x04]);
        bytes.extend_from_slice(&[0x00, 0x01]);
        bytes.extend_from_slice(&[0x00, 0x01]);

        let config = OhttpConfig::from_bytes(&bytes).unwrap();
        assert_eq!(config.key_id, 0x01);
        assert_eq!(config.kem_id, 0x0020);
        assert_eq!(config.kdf_id, 0x0001);
        assert_eq!(config.aead_id, 0x0001);
        assert_eq!(config.public_key.len(), 32);
        assert_eq!(config.public_key[0], 0xAB);
    }

    #[test]
    fn rejects_too_short_input() {
        assert!(matches!(OhttpConfig::from_bytes(&[0x01, 0x00]), Err(OhttpConfigError::TooShort)));
    }

    #[test]
    fn rejects_unsupported_kem_id() {
        let mut bytes = vec![0x01u8];
        bytes.extend_from_slice(&[0xFF, 0xFF]);
        bytes.extend(std::iter::repeat_n(0u8, 32));
        assert!(matches!(OhttpConfig::from_bytes(&bytes), Err(OhttpConfigError::UnsupportedKem(0xFFFF))));
    }

    #[test]
    fn rejects_when_no_symmetric_algorithms() {
        // NOTE: the Dart source's own equivalent test for this ("throws
        // when no symmetric algorithms") builds exactly 37 bytes here,
        // which — same as this port — actually trips the *too-short*
        // check first (needs offset+pubKeySize+4 = 39 bytes to even reach
        // the symLen field), not the symLen==0 check; it only "passes"
        // there because it merely asserts *some* FormatException. This
        // test pads to the full 39 bytes required so it genuinely
        // exercises the `NoSymmetricAlgorithms` branch.
        let mut bytes = vec![0x01u8];
        bytes.extend_from_slice(&[0x00, 0x20]);
        bytes.extend(std::iter::repeat_n(0u8, 32));
        bytes.extend_from_slice(&[0x00, 0x00]); // symLen = 0
        bytes.extend_from_slice(&[0x00, 0x00]); // padding to reach the 39-byte length floor
        assert!(matches!(OhttpConfig::from_bytes(&bytes), Err(OhttpConfigError::NoSymmetricAlgorithms)));
    }

    #[test]
    fn rejects_too_short_for_kem_before_checking_symmetric_algorithms() {
        // The exact 37-byte input from the Dart test — confirms this port
        // matches Dart's actual (if unintentional) precedence: the
        // overall-length check fires before the symLen check ever runs.
        let mut bytes = vec![0x01u8];
        bytes.extend_from_slice(&[0x00, 0x20]);
        bytes.extend(std::iter::repeat_n(0u8, 32));
        bytes.extend_from_slice(&[0x00, 0x00]);
        assert!(matches!(OhttpConfig::from_bytes(&bytes), Err(OhttpConfigError::TooShortForKem { .. })));
    }
}
