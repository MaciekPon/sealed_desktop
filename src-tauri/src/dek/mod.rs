//! PIN-gated secret vault, backed by IOTA Stronghold, replacing the
//! `flutter_secure_storage` + hand-rolled AES-GCM DEK-wrap dance in
//! `sealed_app/lib/local/dek_manager.dart` + `services/pin_service.dart`.
//!
//! Desktop simplification (see the plan): the mobile app has a "device
//! secret" phase that lets the DB open with no PIN right after wallet
//! creation, before the mandatory PIN-setup screen. Stronghold requires a
//! password from the very first write, and desktop has no OS keystore
//! equivalent to back that pre-PIN phase with real protection, so on
//! desktop the PIN is set in the same step as wallet creation — there is no
//! device-wrapped phase. The Stronghold snapshot password *is*
//! `Argon2id(pin, salt)`; there is no separate DEK-wrap layer, since
//! Stronghold's own snapshot encryption already provides that.
//!
//! This module intentionally does not go through the JS-facing
//! `tauri-plugin-stronghold` commands (registered in `lib.rs` for parity
//! with the plan and left available for incidental use) — it talks to the
//! `iota-stronghold` crate directly so secret material (DEK, mnemonic, PQ
//! keys) never has to cross into JS at all; the frontend only ever calls
//! high-level Tauri commands built on top of this module.

use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use iota_stronghold::{Client, KeyProvider, SnapshotPath, Stronghold as IotaStronghold};
use rand08::RngCore;
use thiserror::Error;
use zeroize::Zeroizing;

/// Fixed client name inside the snapshot — this app only ever needs one.
const CLIENT_PATH: &[u8] = b"sealed";
/// Written once at vault creation, checked on every unlock. Distinguishes
/// "wrong PIN" from other Stronghold errors, since a wrong password can
/// otherwise still produce a superficially-opened-but-garbage client.
const CANARY_KEY: &[u8] = b"__canary__";
const CANARY_VALUE: &[u8] = b"sealed-desktop-vault-v1";
const DEK_KEY: &[u8] = b"dek";

const SALT_LEN: usize = 16;
const KEK_LEN: usize = 32;
const DEK_LEN: usize = 32;

// Argon2id tuning — same parameters as the mobile PinService (64 MiB, 3
// iterations, parallelism 1). Not required to match bit-for-bit (this state
// is purely local to a device, never compared across mobile/desktop), but
// there's no reason to deviate from an already-reasoned-about choice.
const ARGON2_MEMORY_KIB: u32 = 64 * 1024;
const ARGON2_ITERATIONS: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;

#[derive(Debug, Error)]
pub enum DekError {
    #[error("PIN must be exactly 6 digits")]
    InvalidPin,
    #[error("incorrect PIN")]
    PinIncorrect,
    #[error("vault already exists at this path")]
    AlreadyExists,
    #[error("vault not found")]
    NotFound,
    #[error("vault error: {0}")]
    Vault(String),
    #[error("io error: {0}")]
    Io(String),
}

pub type DekResult<T> = Result<T, DekError>;

fn validate_pin(pin: &str) -> DekResult<()> {
    if pin.len() == 6 && pin.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(DekError::InvalidPin)
    }
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand08::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

/// Argon2id(pin, salt) -> 32-byte Stronghold vault password.
fn derive_kek(pin: &str, salt: &[u8]) -> [u8; KEK_LEN] {
    let params = Params::new(ARGON2_MEMORY_KIB, ARGON2_ITERATIONS, ARGON2_PARALLELISM, Some(KEK_LEN))
        .expect("static Argon2 params are valid");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; KEK_LEN];
    argon2
        .hash_password_into(pin.as_bytes(), salt, &mut out)
        .expect("Argon2id hashing cannot fail for these static params");
    out
}

fn snapshot_file(app_dir: &Path) -> PathBuf {
    app_dir.join("vault.stronghold")
}

fn salt_file(app_dir: &Path) -> PathBuf {
    app_dir.join("vault.salt")
}

fn key_provider(kek: [u8; KEK_LEN]) -> DekResult<KeyProvider> {
    KeyProvider::try_from(Zeroizing::new(kek.to_vec())).map_err(|e| DekError::Vault(e.to_string()))
}

/// A PIN-unlocked handle onto the Stronghold-backed secret vault.
pub struct Vault {
    stronghold: IotaStronghold,
    snapshot_path: SnapshotPath,
    keyprovider: KeyProvider,
}

// Manual, content-free Debug impl: only needed so `Result<Vault, _>::unwrap_err()`
// type-checks in tests; deliberately does not print any field (none of the
// inner Stronghold/KeyProvider handles are cryptographic material by
// themselves, but there's no reason to reflect them either).
impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault").finish_non_exhaustive()
    }
}

impl Vault {
    /// True if a vault already exists at `app_dir` (returning-user case).
    pub fn exists(app_dir: &Path) -> bool {
        snapshot_file(app_dir).exists()
    }

    /// Irreversibly delete the vault snapshot + salt file, e.g. as part of
    /// a duress/logout wipe. Best-effort: a missing file is not an error.
    pub fn wipe(app_dir: &Path) {
        let _ = std::fs::remove_file(snapshot_file(app_dir));
        let _ = std::fs::remove_file(salt_file(app_dir));
    }

    /// First-run bootstrap: PIN is mandatory at wallet-creation time on
    /// desktop (see module docs), so this both creates the vault and
    /// generates the DEK in one step. Returns the fresh DEK — callers pass
    /// it straight to the SQLCipher DB open call; it is also stored in the
    /// vault under [`DEK_KEY`] for later sessions.
    pub fn create(app_dir: &Path, pin: &str) -> DekResult<(Self, [u8; DEK_LEN])> {
        validate_pin(pin)?;
        if Self::exists(app_dir) {
            return Err(DekError::AlreadyExists);
        }
        std::fs::create_dir_all(app_dir).map_err(|e| DekError::Io(e.to_string()))?;

        let salt = random_bytes::<SALT_LEN>();
        std::fs::write(salt_file(app_dir), salt).map_err(|e| DekError::Io(e.to_string()))?;

        let kek = derive_kek(pin, &salt);
        let keyprovider = key_provider(kek)?;
        let snapshot_path = SnapshotPath::from_path(snapshot_file(app_dir));
        let stronghold = IotaStronghold::default();

        let client = stronghold
            .create_client(CLIENT_PATH)
            .map_err(|e| DekError::Vault(e.to_string()))?;

        let dek = random_bytes::<DEK_LEN>();
        client
            .store()
            .insert(DEK_KEY.to_vec(), dek.to_vec(), None)
            .map_err(|e| DekError::Vault(e.to_string()))?;
        client
            .store()
            .insert(CANARY_KEY.to_vec(), CANARY_VALUE.to_vec(), None)
            .map_err(|e| DekError::Vault(e.to_string()))?;

        stronghold
            .commit_with_keyprovider(&snapshot_path, &keyprovider)
            .map_err(|e| DekError::Vault(e.to_string()))?;

        Ok((
            Self {
                stronghold,
                snapshot_path,
                keyprovider,
            },
            dek,
        ))
    }

    /// Unlock an existing vault with the user's PIN. Mirrors
    /// `PinService.verifyAndUnwrap`: wrong PIN is reported as
    /// [`DekError::PinIncorrect`], distinct from other failures.
    pub fn unlock(app_dir: &Path, pin: &str) -> DekResult<Self> {
        validate_pin(pin)?;
        if !Self::exists(app_dir) {
            return Err(DekError::NotFound);
        }
        let salt = std::fs::read(salt_file(app_dir)).map_err(|e| DekError::Io(e.to_string()))?;
        let kek = derive_kek(pin, &salt);
        let keyprovider = key_provider(kek)?;
        let snapshot_path = SnapshotPath::from_path(snapshot_file(app_dir));
        let stronghold = IotaStronghold::default();

        // A wrong password fails the whole-snapshot AEAD decrypt here.
        stronghold
            .load_snapshot(&keyprovider, &snapshot_path)
            .map_err(|_| DekError::PinIncorrect)?;

        let client = stronghold
            .load_client(CLIENT_PATH)
            .map_err(|e| DekError::Vault(e.to_string()))?;
        let canary = client
            .store()
            .get(CANARY_KEY)
            .map_err(|e| DekError::Vault(e.to_string()))?;
        if canary.as_deref() != Some(CANARY_VALUE) {
            return Err(DekError::PinIncorrect);
        }

        Ok(Self {
            stronghold,
            snapshot_path,
            keyprovider,
        })
    }

    fn client(&self) -> DekResult<Client> {
        self.stronghold
            .get_client(CLIENT_PATH)
            .map_err(|e| DekError::Vault(e.to_string()))
    }

    /// The database encryption key, for opening the SQLCipher DB.
    pub fn dek(&self) -> DekResult<[u8; DEK_LEN]> {
        let bytes = self
            .client()?
            .store()
            .get(DEK_KEY)
            .map_err(|e| DekError::Vault(e.to_string()))?
            .ok_or_else(|| DekError::Vault("dek record missing".into()))?;
        bytes
            .try_into()
            .map_err(|_| DekError::Vault("dek record has unexpected length".into()))
    }

    /// Read an arbitrary secret record (e.g. wallet mnemonic, PQ private
    /// key) written previously via [`Self::set_secret`].
    pub fn get_secret(&self, key: &str) -> DekResult<Option<Vec<u8>>> {
        self.client()?
            .store()
            .get(key.as_bytes())
            .map_err(|e| DekError::Vault(e.to_string()))
    }

    /// Write an arbitrary secret record and persist the snapshot to disk.
    pub fn set_secret(&self, key: &str, value: &[u8]) -> DekResult<()> {
        self.client()?
            .store()
            .insert(key.as_bytes().to_vec(), value.to_vec(), None)
            .map_err(|e| DekError::Vault(e.to_string()))?;
        self.save()
    }

    fn save(&self) -> DekResult<()> {
        self.stronghold
            .commit_with_keyprovider(&self.snapshot_path, &self.keyprovider)
            .map_err(|e| DekError::Vault(e.to_string()))
    }

    /// Change the PIN. Mirrors `PinService.changePin`, but simpler:
    /// `commit_with_keyprovider` re-persists the whole already-decrypted
    /// in-memory snapshot state under a new key, so there's no need to
    /// manually read and rewrite every record. Re-verifies `old_pin` from
    /// scratch (via [`Self::unlock`]) rather than trusting an
    /// already-open handle, so a caller can't rewrap the vault without
    /// re-proving knowledge of the current PIN.
    pub fn change_pin(app_dir: &Path, old_pin: &str, new_pin: &str) -> DekResult<Self> {
        validate_pin(new_pin)?;
        let vault = Self::unlock(app_dir, old_pin)?;

        let new_salt = random_bytes::<SALT_LEN>();
        let new_kek = derive_kek(new_pin, &new_salt);
        let new_keyprovider = key_provider(new_kek)?;

        vault
            .stronghold
            .commit_with_keyprovider(&vault.snapshot_path, &new_keyprovider)
            .map_err(|e| DekError::Vault(e.to_string()))?;
        // Only overwrite the salt file after the re-encrypted snapshot has
        // been committed successfully, so a crash/error in between still
        // leaves the old (pin, salt, snapshot) combination consistent.
        std::fs::write(salt_file(app_dir), new_salt).map_err(|e| DekError::Io(e.to_string()))?;

        Ok(Self {
            stronghold: vault.stronghold,
            snapshot_path: vault.snapshot_path,
            keyprovider: new_keyprovider,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-dek-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn create_then_unlock_round_trip() {
        let dir = temp_dir("create-unlock");
        let (vault, dek) = Vault::create(&dir, "123456").unwrap();
        assert_eq!(vault.dek().unwrap(), dek);

        let reopened = Vault::unlock(&dir, "123456").unwrap();
        assert_eq!(reopened.dek().unwrap(), dek);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_pin_is_rejected() {
        let dir = temp_dir("wrong-pin");
        Vault::create(&dir, "123456").unwrap();
        let err = Vault::unlock(&dir, "654321").unwrap_err();
        assert!(matches!(err, DekError::PinIncorrect));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_pin_format_is_rejected() {
        let dir = temp_dir("invalid-pin-format");
        assert!(matches!(Vault::create(&dir, "12345").unwrap_err(), DekError::InvalidPin));
        assert!(matches!(Vault::create(&dir, "abcdef").unwrap_err(), DekError::InvalidPin));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cannot_create_twice() {
        let dir = temp_dir("create-twice");
        Vault::create(&dir, "123456").unwrap();
        assert!(matches!(Vault::create(&dir, "123456").unwrap_err(), DekError::AlreadyExists));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn secret_round_trip() {
        let dir = temp_dir("secret-round-trip");
        let (vault, _dek) = Vault::create(&dir, "123456").unwrap();
        vault.set_secret("wallet_mnemonic", b"twelve word mnemonic goes here").unwrap();
        assert_eq!(
            vault.get_secret("wallet_mnemonic").unwrap().as_deref(),
            Some(b"twelve word mnemonic goes here".as_ref())
        );
        assert_eq!(vault.get_secret("does_not_exist").unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn change_pin_then_old_pin_fails_new_pin_works() {
        let dir = temp_dir("change-pin");
        let (vault, dek) = Vault::create(&dir, "111111").unwrap();
        vault.set_secret("wallet_mnemonic", b"some secret bytes").unwrap();
        drop(vault);

        let changed = Vault::change_pin(&dir, "111111", "222222").unwrap();
        assert_eq!(changed.dek().unwrap(), dek);
        assert_eq!(
            changed.get_secret("wallet_mnemonic").unwrap().as_deref(),
            Some(b"some secret bytes".as_ref())
        );
        drop(changed);

        assert!(matches!(Vault::unlock(&dir, "111111").unwrap_err(), DekError::PinIncorrect));
        let reopened = Vault::unlock(&dir, "222222").unwrap();
        assert_eq!(reopened.dek().unwrap(), dek);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unlock_nonexistent_vault_errors() {
        let dir = temp_dir("nonexistent");
        assert!(matches!(Vault::unlock(&dir, "123456").unwrap_err(), DekError::NotFound));
    }
}
