//! Bio format validation, ported from `features/identity/bio_validator.dart`.
//! Mirrors `programs/sealed/src/lib/bio.ts` and the on-chain `setBio` check
//! (`BIO_MAX = 160`) — keep in sync.
//!
//! Rules:
//!   - length <= 160 BYTES of UTF-8 (multibyte chars count as encoded
//!     width — an emoji is 4 bytes, not 1 char)
//!   - empty allowed (clears the bio on-chain)
//!   - `\n` allowed (multiline bios); all other control characters rejected
//!     (client-side policy only — the contract validates length, nothing
//!     else)

pub const MAX_BYTES: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BioError {
    TooLong,
    ControlChars,
}

impl std::fmt::Display for BioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            BioError::TooLong => "bio must be at most 160 bytes",
            BioError::ControlChars => "bio contains control characters",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for BioError {}

/// Validate a candidate bio. Empty is always valid (clears the on-chain
/// bio). Caller should trim first, same as the Dart validator.
pub fn validate_bio(bio: &str) -> Result<(), BioError> {
    if bio.is_empty() {
        return Ok(());
    }

    for c in bio.chars() {
        let cu = c as u32;
        let is_control = (cu < 0x20 && c != '\n') || cu == 0x7f;
        if is_control {
            return Err(BioError::ControlChars);
        }
    }

    if bio.as_bytes().len() > MAX_BYTES {
        return Err(BioError::TooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_empty_and_normal_text() {
        assert!(validate_bio("").is_ok());
        assert!(validate_bio("Hello, I like Rust.").is_ok());
    }

    #[test]
    fn accepts_newlines() {
        assert!(validate_bio("line one\nline two").is_ok());
    }

    #[test]
    fn rejects_control_chars() {
        assert_eq!(validate_bio("bad\ttab"), Err(BioError::ControlChars));
        assert_eq!(validate_bio("bad\x7fdel"), Err(BioError::ControlChars));
    }

    #[test]
    fn rejects_over_160_utf8_bytes() {
        let long = "a".repeat(161);
        assert_eq!(validate_bio(&long), Err(BioError::TooLong));
        let exactly_160 = "a".repeat(160);
        assert!(validate_bio(&exactly_160).is_ok());
    }

    #[test]
    fn counts_multibyte_chars_by_encoded_width() {
        // 40 emoji * 4 bytes = 160 bytes exactly -> still valid.
        let emoji_160 = "😀".repeat(40);
        assert!(validate_bio(&emoji_160).is_ok());
        // 41 emoji * 4 bytes = 164 bytes -> too long.
        let emoji_164 = "😀".repeat(41);
        assert_eq!(validate_bio(&emoji_164), Err(BioError::TooLong));
    }
}
