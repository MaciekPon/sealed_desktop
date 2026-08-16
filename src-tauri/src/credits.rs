//! Redeem-code normalization and preimage/commitment derivation, ported
//! from `services/credits_service.dart`. Code grammar matches
//! `programs/sealed/src/lib/code.ts`:
//!
//! - alphabet = `0123456789ABCDEFGHJKMNPQRSTVWXYZ` (Crockford, no I/L/O/U)
//! - normalize: uppercase, drop dashes/spaces, `I`->`1`, `L`->`1`, `O`->`0`
//! - preimage = `sha256(utf8(normalized code))[0..16]`
//! - commitment = `sha256(preimage)` (admin-posted, not needed client-side)

use sha2::{Digest, Sha256};
use thiserror::Error;

pub const CODE_ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const CODE_LEN: usize = 16;
pub const PREIMAGE_LEN: usize = 16;

#[derive(Debug, Error)]
pub enum CreditsError {
    #[error("redeem code must be {CODE_LEN} chars after normalize (got {0})")]
    BadCodeLen(usize),
    #[error("invalid char in redeem code: {0}")]
    BadCodeChar(char),
}

/// Normalize a user-entered code: uppercase, strip dashes/spaces, `I`->`1`,
/// `L`->`1`, `O`->`0`.
pub fn normalize_code(code: &str) -> Result<String, CreditsError> {
    let mut s: String = code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    s = s.replace('I', "1").replace('L', "1").replace('O', "0");

    let len = s.chars().count();
    if len != CODE_LEN {
        return Err(CreditsError::BadCodeLen(len));
    }
    for c in s.chars() {
        if !CODE_ALPHABET.contains(c) {
            return Err(CreditsError::BadCodeChar(c));
        }
    }
    Ok(s)
}

/// First 16 bytes of `sha256(utf8(normalize(code)))`.
pub fn preimage_from_code(code: &str) -> Result<[u8; PREIMAGE_LEN], CreditsError> {
    let n = normalize_code(code)?;
    let digest = Sha256::digest(n.as_bytes());
    let mut out = [0u8; PREIMAGE_LEN];
    out.copy_from_slice(&digest[..PREIMAGE_LEN]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_dashes_spaces_and_uppercases() {
        // No I/L/O in this one, so normalization is just strip+uppercase.
        assert_eq!(normalize_code("abcd-efgh jkmn-pqrs").unwrap(), "ABCDEFGHJKMNPQRS");
    }

    #[test]
    fn substitutes_lookalike_characters() {
        // I/L -> 1, O -> 0
        assert_eq!(normalize_code("IIII LLLL OOOO 2222").unwrap(), "1111111100002222");
    }

    #[test]
    fn rejects_stray_characters_after_substitution() {
        // 'U' isn't in the Crockford alphabet and isn't substituted.
        assert!(matches!(normalize_code("ABCDEFGHJKMNPQRU"), Err(CreditsError::BadCodeChar('U'))));
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(matches!(normalize_code("ABCD"), Err(CreditsError::BadCodeLen(4))));
    }

    #[test]
    fn preimage_is_deterministic() {
        let code = "ABCD-EFGH-JKMN-PQRS";
        let p1 = preimage_from_code(code).unwrap();
        let p2 = preimage_from_code(code).unwrap();
        assert_eq!(p1, p2);
        assert_eq!(p1.len(), PREIMAGE_LEN);
    }
}
