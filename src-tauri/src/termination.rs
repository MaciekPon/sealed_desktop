//! Duress "termination code" + wrong-PIN attempt tracking. Ports
//! `services/termination_service.dart` + `services/pin_attempt_tracker.dart`.
//!
//! On mobile these live in `flutter_secure_storage`, deliberately *outside*
//! the DEK-wrapped vault: the lock screen checks the termination code
//! before ever touching `PinService`, so a duress wipe never has to bring
//! the real DEK into memory. Our Stronghold vault has the same property
//! only if these records live outside it too — so, like `vault.salt` in
//! `dek/mod.rs`, the termination salt/marker and the attempt counter are
//! plain files next to the snapshot. None of them are secret by
//! themselves: a salt and an HMAC marker over a fixed string don't reveal
//! the termination code any more than a password hash reveals a password.

use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use rand08::RngCore;
use thiserror::Error;

use crate::crypto::constant_time_equals;
use crate::crypto::kdf::hmac_sha256;

/// Hard cap: on the Nth wrong PIN attempt the caller must wipe the device.
/// Matches `PinAttemptTracker.maxAttempts`.
pub const MAX_PIN_ATTEMPTS: u32 = 5;

const SALT_LEN: usize = 16;
const KEK_LEN: usize = 32;
const MARKER_INPUT: &[u8] = b"TERMINATE-v1";

// Same Argon2id tuning as dek/mod.rs — not required to match bit-for-bit
// with anything else, just internally consistent.
const ARGON2_MEMORY_KIB: u32 = 64 * 1024;
const ARGON2_ITERATIONS: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;

#[derive(Debug, Error)]
pub enum TerminationError {
    #[error("termination code must be exactly 6 digits")]
    InvalidCode,
    #[error("io error: {0}")]
    Io(String),
}

pub type TerminationResult<T> = Result<T, TerminationError>;

fn validate_code(code: &str) -> TerminationResult<()> {
    if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(TerminationError::InvalidCode)
    }
}

fn salt_file(app_dir: &Path) -> PathBuf {
    app_dir.join("termination.salt")
}

fn marker_file(app_dir: &Path) -> PathBuf {
    app_dir.join("termination.marker")
}

fn attempts_file(app_dir: &Path) -> PathBuf {
    app_dir.join("pin_attempts.count")
}

fn random_salt() -> [u8; SALT_LEN] {
    let mut buf = [0u8; SALT_LEN];
    rand08::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

fn derive_kek(code: &str, salt: &[u8]) -> [u8; KEK_LEN] {
    let params = Params::new(ARGON2_MEMORY_KIB, ARGON2_ITERATIONS, ARGON2_PARALLELISM, Some(KEK_LEN))
        .expect("static Argon2 params are valid");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; KEK_LEN];
    argon2
        .hash_password_into(code.as_bytes(), salt, &mut out)
        .expect("Argon2id hashing cannot fail for these static params");
    out
}

fn hmac_marker(kek: &[u8; KEK_LEN]) -> [u8; 32] {
    hmac_sha256(kek, MARKER_INPUT)
}

fn io_err(e: std::io::Error) -> TerminationError {
    TerminationError::Io(e.to_string())
}

/// True if a termination code has been configured for this device.
pub fn is_configured(app_dir: &Path) -> bool {
    marker_file(app_dir).exists()
}

/// Set (or replace) the termination code.
pub fn set_code(app_dir: &Path, code: &str) -> TerminationResult<()> {
    validate_code(code)?;
    std::fs::create_dir_all(app_dir).map_err(io_err)?;

    let salt = random_salt();
    let kek = derive_kek(code, &salt);
    let marker = hmac_marker(&kek);

    std::fs::write(salt_file(app_dir), salt).map_err(io_err)?;
    std::fs::write(marker_file(app_dir), marker).map_err(io_err)?;
    Ok(())
}

/// Remove the configured termination code, if any.
pub fn disable(app_dir: &Path) -> TerminationResult<()> {
    let _ = std::fs::remove_file(salt_file(app_dir));
    let _ = std::fs::remove_file(marker_file(app_dir));
    Ok(())
}

/// True if `code` matches the configured termination code. Never errors on
/// a malformed/absent input — this runs on every unlock attempt alongside
/// the real PIN check, so a non-digit or wrong-length entry must just be
/// "not a match", not a thrown error.
pub fn matches(app_dir: &Path, code: &str) -> bool {
    if !is_configured(app_dir) {
        return false;
    }
    if validate_code(code).is_err() {
        return false;
    }
    let Ok(salt) = std::fs::read(salt_file(app_dir)) else {
        return false;
    };
    let Ok(expected) = std::fs::read(marker_file(app_dir)) else {
        return false;
    };
    let kek = derive_kek(code, &salt);
    let candidate = hmac_marker(&kek);
    constant_time_equals(&candidate, &expected)
}

/// Current count of consecutive wrong PIN attempts since the last success.
pub fn attempt_count(app_dir: &Path) -> u32 {
    std::fs::read_to_string(attempts_file(app_dir))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Record a wrong PIN attempt and return the new count.
pub fn record_failed_attempt(app_dir: &Path) -> TerminationResult<u32> {
    std::fs::create_dir_all(app_dir).map_err(io_err)?;
    let next = attempt_count(app_dir) + 1;
    std::fs::write(attempts_file(app_dir), next.to_string()).map_err(io_err)?;
    Ok(next)
}

/// Reset the attempt counter on a successful unlock.
pub fn reset_attempts(app_dir: &Path) -> TerminationResult<()> {
    let _ = std::fs::remove_file(attempts_file(app_dir));
    Ok(())
}

/// Irreversibly delete the termination salt/marker and the attempt
/// counter, e.g. as part of a duress/logout wipe. Best-effort: a missing
/// file is not an error.
pub fn wipe_all(app_dir: &Path) {
    let _ = std::fs::remove_file(salt_file(app_dir));
    let _ = std::fs::remove_file(marker_file(app_dir));
    let _ = std::fs::remove_file(attempts_file(app_dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-termination-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn not_configured_until_set() {
        let dir = temp_dir("not-configured");
        assert!(!is_configured(&dir));
        assert!(!matches(&dir, "123456"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_code_then_matches_round_trip() {
        let dir = temp_dir("set-then-match");
        set_code(&dir, "999999").unwrap();
        assert!(is_configured(&dir));
        assert!(matches(&dir, "999999"));
        assert!(!matches(&dir, "111111"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_code_never_matches_and_never_errors() {
        let dir = temp_dir("malformed");
        set_code(&dir, "999999").unwrap();
        assert!(!matches(&dir, "abcdef"));
        assert!(!matches(&dir, "12345"));
        assert!(!matches(&dir, ""));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_code_format_rejected_on_set() {
        let dir = temp_dir("invalid-set");
        assert!(matches!(set_code(&dir, "12345").unwrap_err(), TerminationError::InvalidCode));
        assert!(matches!(set_code(&dir, "abcdef").unwrap_err(), TerminationError::InvalidCode));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disable_clears_configuration() {
        let dir = temp_dir("disable");
        set_code(&dir, "999999").unwrap();
        disable(&dir).unwrap();
        assert!(!is_configured(&dir));
        assert!(!matches(&dir, "999999"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attempt_counter_records_and_resets() {
        let dir = temp_dir("attempts");
        assert_eq!(attempt_count(&dir), 0);
        assert_eq!(record_failed_attempt(&dir).unwrap(), 1);
        assert_eq!(record_failed_attempt(&dir).unwrap(), 2);
        assert_eq!(attempt_count(&dir), 2);
        reset_attempts(&dir).unwrap();
        assert_eq!(attempt_count(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
