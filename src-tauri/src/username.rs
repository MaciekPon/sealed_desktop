//! Username format validation, ported from `services/username_validator.dart`.
//! Mirrors `programs/username_registry/src/validator.ts` and the on-chain
//! rules — keep in sync if either side changes.
//!
//! Rules: charset `[a-z0-9_]`, length 3..20, first char not `_`/digit, last
//! char not `_`, plus a fixed reserved-names list (off-chain only).

pub const MIN_LEN: usize = 3;
pub const MAX_LEN: usize = 20;

const RESERVED: &[&str] = &["admin", "sealed", "support", "system", "root", "null", "undefined"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsernameError {
    Empty,
    NotAscii,
    BadLen,
    BadChar,
    LeadingUnderscore,
    LeadingDigit,
    TrailingUnderscore,
    Reserved,
}

impl std::fmt::Display for UsernameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            UsernameError::Empty => "username cannot be empty",
            UsernameError::NotAscii => "username must be ASCII",
            UsernameError::BadLen => "username must be 3-20 characters",
            UsernameError::BadChar => "username must contain only a-z, 0-9, or \"_\"",
            UsernameError::LeadingUnderscore => "username cannot start with \"_\"",
            UsernameError::LeadingDigit => "username cannot start with a digit",
            UsernameError::TrailingUnderscore => "username cannot end with \"_\"",
            UsernameError::Reserved => "username is reserved",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for UsernameError {}

/// Validate a candidate username. Caller should lowercase + trim first —
/// this mirrors the Dart validator, which makes the same assumption
/// (charset check requires lowercase input).
pub fn validate_username(name: &str) -> Result<(), UsernameError> {
    if name.is_empty() {
        return Err(UsernameError::Empty);
    }
    if !name.is_ascii() {
        return Err(UsernameError::NotAscii);
    }
    if name.len() < MIN_LEN || name.len() > MAX_LEN {
        return Err(UsernameError::BadLen);
    }

    let bytes = name.as_bytes();
    if bytes[0] == b'_' {
        return Err(UsernameError::LeadingUnderscore);
    }
    if bytes[0].is_ascii_digit() {
        return Err(UsernameError::LeadingDigit);
    }
    if *bytes.last().unwrap() == b'_' {
        return Err(UsernameError::TrailingUnderscore);
    }
    if !bytes.iter().all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
        return Err(UsernameError::BadChar);
    }
    if RESERVED.contains(&name) {
        return Err(UsernameError::Reserved);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names() {
        assert!(validate_username("alice").is_ok());
        assert!(validate_username("a1_bc").is_ok());
        assert!(validate_username("abc123").is_ok());
    }

    #[test]
    fn rejects_each_rule_violation() {
        assert_eq!(validate_username(""), Err(UsernameError::Empty));
        assert_eq!(validate_username("ab"), Err(UsernameError::BadLen));
        assert_eq!(validate_username(&"a".repeat(21)), Err(UsernameError::BadLen));
        assert_eq!(validate_username("_alice"), Err(UsernameError::LeadingUnderscore));
        assert_eq!(validate_username("1alice"), Err(UsernameError::LeadingDigit));
        assert_eq!(validate_username("alice_"), Err(UsernameError::TrailingUnderscore));
        assert_eq!(validate_username("Alice"), Err(UsernameError::BadChar));
        assert_eq!(validate_username("al!ce"), Err(UsernameError::BadChar));
        assert_eq!(validate_username("admin"), Err(UsernameError::Reserved));
    }

    #[test]
    fn rejects_non_ascii() {
        assert_eq!(validate_username("aliße"), Err(UsernameError::NotAscii));
    }
}
