//! Settings commands: app preferences, the duress termination code, seed
//! phrase backup, and log out. Mirrors `app_settings_service.dart` +
//! `termination_service.dart` + the relevant slices of
//! `features/settings/screens/settings_screen.dart` and
//! `change_termination_flow.dart`.
//!
//! Desktop drops the "targeted push" toggle entirely (see the plan:
//! background poll + native toast replaces OS push), so only
//! `auto_sync_enabled` survives from `AppSettingsService`.

use tauri::State;

use crate::commands::auth::wipe_device_impl;
use crate::dek::Vault;
use crate::settings::{self, AppSettings};
use crate::state::AppState;
use crate::termination;

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> AppSettings {
    settings::load(&state.app_dir)
}

#[tauri::command]
pub fn set_auto_sync_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    settings::set_auto_sync_enabled(&state.app_dir, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn is_termination_configured(state: State<'_, AppState>) -> bool {
    termination::is_configured(&state.app_dir)
}

#[tauri::command]
pub fn set_termination_code(state: State<'_, AppState>, code: String) -> Result<(), String> {
    termination::set_code(&state.app_dir, &code).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn disable_termination_code(state: State<'_, AppState>) -> Result<(), String> {
    termination::disable(&state.app_dir).map_err(|e| e.to_string())
}

/// Verify a termination code against the configured one. Unlike unlocking
/// the app, a match here does **not** wipe the device — this is the
/// "confirm before changing it" gate in `ChangeTerminationFlow`, reachable
/// only from within Settings while already unlocked. Wipe-on-entry is
/// exclusively a lock-screen behavior, handled in `commands::auth::unlock_account`.
#[tauri::command]
pub fn verify_termination_code(state: State<'_, AppState>, code: String) -> bool {
    termination::matches(&state.app_dir, &code)
}

/// Re-verify the PIN without disturbing the active session — used as the
/// gate step before setting a termination code for the first time, and
/// before revealing the recovery phrase during log-out.
#[tauri::command]
pub fn verify_pin(state: State<'_, AppState>, pin: String) -> bool {
    Vault::unlock(&state.app_dir, &pin).is_ok()
}

/// Recovery phrase for backup — requires an already-unlocked session, since
/// it reads the mnemonic straight out of the session's open vault handle
/// rather than prompting for the PIN again itself (callers should gate the
/// UI with [`verify_pin`] first, mirroring the mobile PIN-confirm screen).
#[tauri::command]
pub async fn get_seed_phrase_for_backup(state: State<'_, AppState>) -> Result<String, String> {
    let session = state.session.lock().await;
    let session = session.as_ref().ok_or("no active session")?;
    let mnemonic_bytes = session
        .vault
        .get_secret("wallet_mnemonic")
        .map_err(|e| e.to_string())?
        .ok_or("wallet_mnemonic missing from vault — data corruption")?;
    String::from_utf8(mnemonic_bytes).map_err(|e| e.to_string())
}

/// Log out. On mobile this is already fully destructive — there is no
/// "sign out, keep local data for next time" concept, since the mnemonic
/// is the only recovery path (see `performLogout` + `WipeService.wipeAll`,
/// which the Settings screen's "Log Out" button and the duress paths both
/// ultimately funnel into). Desktop keeps that same shape: log out wipes
/// the vault, database, termination state, and app settings.
#[tauri::command]
pub async fn log_out(state: State<'_, AppState>) -> Result<(), ()> {
    wipe_device_impl(&state).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::auth::create_account_impl;

    fn temp_state(name: &str) -> AppState {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-settings-cmd-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        AppState::new(dir)
    }

    #[tokio::test]
    async fn app_settings_round_trip() {
        // `State<'_, T>` isn't constructible outside a running Tauri app,
        // so — as with the other command modules — these tests exercise
        // the plain-fn layer the `#[tauri::command]` wrappers delegate to.
        let state = temp_state("app-settings");
        assert!(settings::load(&state.app_dir).auto_sync_enabled);

        settings::set_auto_sync_enabled(&state.app_dir, false).unwrap();
        assert!(!settings::load(&state.app_dir).auto_sync_enabled);

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn termination_code_set_verify_disable() {
        let state = temp_state("termination");
        assert!(!termination::is_configured(&state.app_dir));

        termination::set_code(&state.app_dir, "111222").unwrap();
        assert!(termination::is_configured(&state.app_dir));
        assert!(termination::matches(&state.app_dir, "111222"));
        assert!(!termination::matches(&state.app_dir, "000000"));

        termination::disable(&state.app_dir).unwrap();
        assert!(!termination::is_configured(&state.app_dir));

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn verify_pin_matches_vault_without_disturbing_session() {
        let state = temp_state("verify-pin");
        create_account_impl(&state, "123456").await.unwrap();

        assert!(Vault::unlock(&state.app_dir, "123456").is_ok());
        assert!(Vault::unlock(&state.app_dir, "000000").is_err());
        // The active session (established by create_account_impl) must
        // still be intact — verifying via a fresh Vault::unlock must not
        // touch it.
        assert!(state.session.lock().await.is_some());

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn seed_phrase_readable_from_active_session() {
        let state = temp_state("seed-phrase");
        let created = create_account_impl(&state, "123456").await.unwrap();

        let session = state.session.lock().await;
        let mnemonic_bytes = session
            .as_ref()
            .unwrap()
            .vault
            .get_secret("wallet_mnemonic")
            .unwrap()
            .unwrap();
        let mnemonic = String::from_utf8(mnemonic_bytes).unwrap();
        assert_eq!(mnemonic, created.mnemonic);
        drop(session);

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }

    #[tokio::test]
    async fn log_out_wipes_everything() {
        let state = temp_state("log-out");
        create_account_impl(&state, "123456").await.unwrap();
        termination::set_code(&state.app_dir, "654321").unwrap();
        settings::set_auto_sync_enabled(&state.app_dir, false).unwrap();

        wipe_device_impl(&state).await;

        assert!(state.session.lock().await.is_none());
        assert!(!Vault::exists(&state.app_dir));
        assert!(!termination::is_configured(&state.app_dir));
        assert!(settings::load(&state.app_dir).auto_sync_enabled); // back to default
        assert!(!state.db_path().exists());

        let _ = std::fs::remove_dir_all(&state.app_dir);
    }
}
