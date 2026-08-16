//! Auth commands: wallet creation/restore, PIN unlock/lock/change. The
//! desktop entry point mirrors `features/auth/screens/wallet_setup.dart` +
//! `pin_setup_screen.dart` + `lock_screen.dart`, collapsed into a single
//! mandatory-PIN-at-creation flow (see the plan for why there's no
//! device-wrapped pre-PIN phase on desktop).
//!
//! Each `#[tauri::command]` is a thin wrapper around a plain `&AppState`
//! function below — kept separate so the logic is unit-testable without
//! spinning up a Tauri runtime (`tauri::State` isn't constructible outside
//! one). Commands are `async` because `AppState.session` is a
//! `tokio::sync::Mutex` — required so later command modules can hold the
//! lock across `.await` points (e.g. an in-flight chain/indexer HTTP call),
//! which a `std::sync::MutexGuard` cannot do.

use base64::Engine;
use tauri::State;

use crate::chain::wallet::{AlgorandWallet, WalletError};
use crate::db::Db;
use crate::dek::{DekError, Vault};
use crate::keys::{derive_sealed_keys, SealedKeys};
use crate::state::{AppState, Session};
use crate::termination;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAccountInfo {
    pub wallet_address: String,
    /// Shown to the user exactly once at creation/restore time so they can
    /// back it up — never persisted in plaintext, only inside the vault.
    pub mnemonic: String,
}

// Manual, redacting Debug impl — `mnemonic` is a genuine secret (the
// wallet's full recovery phrase); a derived Debug would print it verbatim
// into any log/panic message that captures this struct.
impl std::fmt::Debug for NewAccountInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewAccountInfo")
            .field("wallet_address", &self.wallet_address)
            .field("mnemonic", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub wallet_address: String,
    pub encryption_pubkey: String, // base64
    pub scan_pubkey: String, // base64
}

/// Result of a PIN-entry attempt on the lock screen. Mirrors the three
/// branches in `lock_screen.dart`'s `_submit`/`_onWrongPin`: a correct PIN,
/// a wrong PIN (with attempts remaining before the device wipes), or the
/// device having just been wiped — either because `pin` matched the
/// configured termination code (duress) or because this was the
/// [`termination::MAX_PIN_ATTEMPTS`]th consecutive wrong PIN. Both wipe
/// paths report the same `Wiped` variant: per the mobile duress design, the
/// caller must not distinguish them in the UI (a distinguishable duress
/// response would defeat the point of a termination code).
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UnlockOutcome {
    Success(AccountInfo),
    WrongPin { attempts_remaining: u32 },
    Wiped,
}

fn account_info(wallet: &AlgorandWallet, keys: &SealedKeys) -> AccountInfo {
    AccountInfo {
        wallet_address: wallet.address.clone(),
        encryption_pubkey: base64::engine::general_purpose::STANDARD.encode(keys.encryption_pubkey),
        scan_pubkey: base64::engine::general_purpose::STANDARD.encode(keys.scan_pubkey),
    }
}

#[tauri::command]
pub fn has_existing_account(state: State<'_, AppState>) -> bool {
    has_existing_account_impl(&state)
}

#[tauri::command]
pub async fn is_unlocked(state: State<'_, AppState>) -> Result<bool, ()> {
    Ok(is_unlocked_impl(&state).await)
}

#[tauri::command]
pub async fn create_account(state: State<'_, AppState>, pin: String) -> Result<NewAccountInfo, String> {
    create_account_impl(&state, &pin).await
}

#[tauri::command]
pub async fn restore_account(state: State<'_, AppState>, pin: String, mnemonic: String) -> Result<NewAccountInfo, String> {
    restore_account_impl(&state, &pin, &mnemonic).await
}

#[tauri::command]
pub async fn unlock_account(state: State<'_, AppState>, pin: String) -> Result<UnlockOutcome, String> {
    unlock_account_impl(&state, &pin).await
}

#[tauri::command]
pub async fn lock_account(state: State<'_, AppState>) -> Result<(), ()> {
    lock_account_impl(&state).await;
    Ok(())
}

#[tauri::command]
pub async fn change_pin(state: State<'_, AppState>, old_pin: String, new_pin: String) -> Result<(), String> {
    change_pin_impl(&state, &old_pin, &new_pin).await
}

// ---------------------------------------------------------------------------
// Plain-`&AppState` implementations (unit-testable)
// ---------------------------------------------------------------------------

/// True if a vault already exists on this device (returning-user case) —
/// callers use this to decide between showing "create/restore" vs. "enter
/// your PIN" on first launch. Sync — only touches the filesystem, no
/// session lock needed.
fn has_existing_account_impl(state: &AppState) -> bool {
    Vault::exists(&state.app_dir)
}

/// True if the session is currently unlocked.
async fn is_unlocked_impl(state: &AppState) -> bool {
    state.session.lock().await.is_some()
}

pub(crate) async fn create_account_impl(state: &AppState, pin: &str) -> Result<NewAccountInfo, String> {
    if Vault::exists(&state.app_dir) {
        return Err("an account already exists on this device".into());
    }

    let wallet = AlgorandWallet::create();
    let mnemonic = wallet.mnemonic().to_string();

    let (vault, dek) = Vault::create(&state.app_dir, pin).map_err(dek_err)?;
    vault.set_secret("wallet_mnemonic", mnemonic.as_bytes()).map_err(dek_err)?;

    let sealed_keys = derive_sealed_keys(&wallet).map_err(|e| e.to_string())?;
    let db = open_and_seed_db(&state.db_path(), &dek, &wallet, &sealed_keys)?;

    let wallet_address = wallet.address.clone();
    let mut session = state.session.lock().await;
    *session = Some(Session::new(vault, dek, db, wallet, sealed_keys));

    Ok(NewAccountInfo { wallet_address, mnemonic })
}

async fn restore_account_impl(state: &AppState, pin: &str, mnemonic: &str) -> Result<NewAccountInfo, String> {
    if Vault::exists(&state.app_dir) {
        return Err("an account already exists on this device".into());
    }

    let wallet = AlgorandWallet::restore(mnemonic).map_err(wallet_err)?;

    let (vault, dek) = Vault::create(&state.app_dir, pin).map_err(dek_err)?;
    vault.set_secret("wallet_mnemonic", mnemonic.as_bytes()).map_err(dek_err)?;

    let sealed_keys = derive_sealed_keys(&wallet).map_err(|e| e.to_string())?;
    let db = open_and_seed_db(&state.db_path(), &dek, &wallet, &sealed_keys)?;

    let wallet_address = wallet.address.clone();
    let mut session = state.session.lock().await;
    *session = Some(Session::new(vault, dek, db, wallet, sealed_keys));

    Ok(NewAccountInfo { wallet_address, mnemonic: mnemonic.to_string() })
}

async fn unlock_account_impl(state: &AppState, pin: &str) -> Result<UnlockOutcome, String> {
    // Duress branch first, and unconditionally: checking the termination
    // code must happen before ever touching the PIN vault, so a match
    // wipes the device without the real DEK ever entering memory. This
    // mirrors `LockScreen._submit` checking `term.matches` ahead of
    // `pin.verifyAndUnwrap`.
    if termination::matches(&state.app_dir, pin) {
        wipe_device_impl(state).await;
        return Ok(UnlockOutcome::Wiped);
    }

    let vault = match Vault::unlock(&state.app_dir, pin) {
        Ok(v) => v,
        Err(DekError::PinIncorrect) => {
            let attempts = termination::record_failed_attempt(&state.app_dir).map_err(|e| e.to_string())?;
            if attempts >= termination::MAX_PIN_ATTEMPTS {
                wipe_device_impl(state).await;
                return Ok(UnlockOutcome::Wiped);
            }
            return Ok(UnlockOutcome::WrongPin {
                attempts_remaining: termination::MAX_PIN_ATTEMPTS - attempts,
            });
        }
        Err(e) => return Err(dek_err(e)),
    };
    termination::reset_attempts(&state.app_dir).map_err(|e| e.to_string())?;

    let dek = vault.dek().map_err(dek_err)?;

    let mnemonic_bytes = vault
        .get_secret("wallet_mnemonic")
        .map_err(dek_err)?
        .ok_or("wallet_mnemonic missing from vault — data corruption")?;
    let mnemonic = String::from_utf8(mnemonic_bytes).map_err(|e| e.to_string())?;
    let wallet = AlgorandWallet::restore(&mnemonic).map_err(wallet_err)?;

    let sealed_keys = derive_sealed_keys(&wallet).map_err(|e| e.to_string())?;
    let db = Db::open(&state.db_path(), &dek).map_err(|e| e.to_string())?;

    let info = account_info(&wallet, &sealed_keys);
    let mut session = state.session.lock().await;
    *session = Some(Session::new(vault, dek, db, wallet, sealed_keys));

    Ok(UnlockOutcome::Success(info))
}

async fn lock_account_impl(state: &AppState) {
    *state.session.lock().await = None;
}

/// The panic switch: irreversibly erase the vault, database, termination
/// state, and app settings, then drop the active session. Best-effort —
/// mirrors `WipeService.wipeAll`'s ordering (session/logout-equivalent
/// state first, then the files it depended on) but every step runs
/// unconditionally rather than bailing out on the first failure, since a
/// duress wipe must not stop halfway because one file was already gone.
pub(crate) async fn wipe_device_impl(state: &AppState) {
    *state.session.lock().await = None;
    let _ = std::fs::remove_file(state.db_path());
    Vault::wipe(&state.app_dir);
    termination::wipe_all(&state.app_dir);
    crate::settings::wipe(&state.app_dir);
}

async fn change_pin_impl(state: &AppState, old_pin: &str, new_pin: &str) -> Result<(), String> {
    let new_vault = Vault::change_pin(&state.app_dir, old_pin, new_pin).map_err(dek_err)?;

    let mut session = state.session.lock().await;
    if let Some(active) = session.as_mut() {
        active.vault = new_vault;
    }
    Ok(())
}

fn open_and_seed_db(path: &std::path::Path, dek: &[u8; 32], wallet: &AlgorandWallet, keys: &SealedKeys) -> Result<Db, String> {
    let db = Db::open(path, dek).map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    db.connection()
        .execute(
            "INSERT INTO user_profile (wallet_address, encryption_pubkey, scan_pubkey, created_at, last_login) \
             VALUES (?1, ?2, ?3, ?4, ?4)",
            rusqlite::params![wallet.address, keys.encryption_pubkey.to_vec(), keys.scan_pubkey.to_vec(), now],
        )
        .map_err(|e| e.to_string())?;
    db.save(dek).map_err(|e| e.to_string())?;
    Ok(db)
}

fn dek_err(e: DekError) -> String {
    e.to_string()
}

fn wallet_err(e: WalletError) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state(name: &str) -> AppState {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-auth-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        AppState::new(dir)
    }

    #[tokio::test]
    async fn create_then_unlock_round_trip() {
        let state = temp_state("create-unlock");
        assert!(!has_existing_account_impl(&state));

        let created = create_account_impl(&state, "123456").await.unwrap();
        assert_eq!(created.mnemonic.split_whitespace().count(), 24);
        assert!(is_unlocked_impl(&state).await);
        assert!(has_existing_account_impl(&state));

        lock_account_impl(&state).await;
        assert!(!is_unlocked_impl(&state).await);

        let outcome = unlock_account_impl(&state, "123456").await.unwrap();
        let UnlockOutcome::Success(unlocked) = outcome else {
            panic!("expected UnlockOutcome::Success, got {outcome:?}");
        };
        assert_eq!(unlocked.wallet_address, created.wallet_address);
        assert!(is_unlocked_impl(&state).await);

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn wrong_pin_is_rejected_on_unlock() {
        let state = temp_state("wrong-pin");
        create_account_impl(&state, "111111").await.unwrap();
        lock_account_impl(&state).await;

        let outcome = unlock_account_impl(&state, "222222").await.unwrap();
        assert!(
            matches!(outcome, UnlockOutcome::WrongPin { attempts_remaining } if attempts_remaining == termination::MAX_PIN_ATTEMPTS - 1)
        );
        // Wrong PIN must not establish a session.
        assert!(!is_unlocked_impl(&state).await);

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn fifth_consecutive_wrong_pin_wipes_the_device() {
        let state = temp_state("five-strikes");
        create_account_impl(&state, "111111").await.unwrap();
        lock_account_impl(&state).await;

        for attempt in 1..termination::MAX_PIN_ATTEMPTS {
            let outcome = unlock_account_impl(&state, "999999").await.unwrap();
            assert!(
                matches!(outcome, UnlockOutcome::WrongPin { attempts_remaining } if attempts_remaining == termination::MAX_PIN_ATTEMPTS - attempt)
            );
        }
        assert!(has_existing_account_impl(&state));

        let outcome = unlock_account_impl(&state, "999999").await.unwrap();
        assert!(matches!(outcome, UnlockOutcome::Wiped));
        assert!(!has_existing_account_impl(&state));

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn termination_code_wipes_the_device_without_ever_unlocking() {
        let state = temp_state("duress-wipe");
        create_account_impl(&state, "111111").await.unwrap();
        crate::termination::set_code(&state.app_dir, "654321").unwrap();
        lock_account_impl(&state).await;

        let outcome = unlock_account_impl(&state, "654321").await.unwrap();
        assert!(matches!(outcome, UnlockOutcome::Wiped));
        assert!(!has_existing_account_impl(&state));
        assert!(!is_unlocked_impl(&state).await);

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn correct_pin_after_wrong_attempts_resets_the_counter() {
        let state = temp_state("reset-attempts");
        create_account_impl(&state, "111111").await.unwrap();
        lock_account_impl(&state).await;

        unlock_account_impl(&state, "999999").await.unwrap();
        unlock_account_impl(&state, "999999").await.unwrap();
        assert_eq!(termination::attempt_count(&state.app_dir), 2);

        let outcome = unlock_account_impl(&state, "111111").await.unwrap();
        assert!(matches!(outcome, UnlockOutcome::Success(_)));
        assert_eq!(termination::attempt_count(&state.app_dir), 0);

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn cannot_create_account_twice() {
        let state = temp_state("create-twice");
        create_account_impl(&state, "123456").await.unwrap();
        let err = create_account_impl(&state, "654321").await.unwrap_err();
        assert!(err.contains("already exists"));

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn restore_account_recovers_same_address() {
        let creator_state = temp_state("restore-creator");
        let created = create_account_impl(&creator_state, "123456").await.unwrap();
        let _ = std::fs::remove_dir_all(&creator_state.app_dir);

        let restorer_state = temp_state("restore-target");
        let restored = restore_account_impl(&restorer_state, "654321", &created.mnemonic).await.unwrap();
        assert_eq!(restored.wallet_address, created.wallet_address);

        let _ = std::fs::remove_dir_all(&restorer_state.app_dir);
    }

    #[tokio::test]
    async fn change_pin_then_old_pin_fails_new_pin_works() {
        let state = temp_state("change-pin");
        let created = create_account_impl(&state, "111111").await.unwrap();
        lock_account_impl(&state).await;

        change_pin_impl(&state, "111111", "222222").await.unwrap();

        assert!(matches!(
            unlock_account_impl(&state, "111111").await.unwrap(),
            UnlockOutcome::WrongPin { .. }
        ));
        termination::reset_attempts(&state.app_dir).unwrap();
        let outcome = unlock_account_impl(&state, "222222").await.unwrap();
        let UnlockOutcome::Success(unlocked) = outcome else {
            panic!("expected UnlockOutcome::Success, got {outcome:?}");
        };
        assert_eq!(unlocked.wallet_address, created.wallet_address);

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn unlocked_session_can_read_seeded_user_profile_row() {
        let state = temp_state("seeded-profile");
        let created = create_account_impl(&state, "123456").await.unwrap();

        let session_guard = state.session.lock().await;
        let session = session_guard.as_ref().unwrap();
        let wallet_address: String = session
            .db
            .connection()
            .query_row("SELECT wallet_address FROM user_profile LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(wallet_address, created.wallet_address);
        drop(session_guard);

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }
}
