//! App-wide preferences, mirroring `services/app_settings_service.dart`.
//!
//! Desktop keeps `auto_sync_enabled` and `notifications_enabled`. The Dart
//! service's other flags don't carry over: `preferredSyncLayer` has exactly
//! one real value (`SyncLayer.blockchain` — `auto` just falls back to it)
//! so there is nothing for a desktop toggle to select between, and
//! `targetedPushEnabled` gates an OS push-*token-registration* flow that
//! doesn't exist on desktop (no APNs/FCM equivalent). `notifications_enabled`
//! is desktop-native, not a port of that flag: it only gates whether a
//! native OS toast is shown for a new message — the background poll/sync
//! itself (and the `messages-updated` event the UI relies on for badges/chat
//! updates) always keeps running regardless, per explicit product decision
//! (2026-09-01): a user should be able to turn off the toast without losing
//! sync. See `sync/mod.rs::tick` for where this is actually checked.
//!
//! `minimize_to_tray_enabled` (2026-09-01, desktop-only — has no mobile
//! equivalent at all, nothing to mirror): when true, closing the main
//! window hides it instead of quitting the process, leaving the tray icon
//! as the only way back in (or "Quit" from the tray menu to actually exit).
//! Defaults to `true` per explicit product decision — most users expect a
//! chat app to keep running in the background. See `tray.rs` for where this
//! is checked (the `WindowEvent::CloseRequested` handler).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub auto_sync_enabled: bool,
    #[serde(default = "default_true")]
    pub notifications_enabled: bool,
    #[serde(default = "default_true")]
    pub minimize_to_tray_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { auto_sync_enabled: true, notifications_enabled: true, minimize_to_tray_enabled: true }
    }
}

fn default_true() -> bool {
    true
}

fn settings_file(app_dir: &Path) -> PathBuf {
    app_dir.join("app_settings.json")
}

/// Load current settings, falling back to defaults if unset or corrupt.
pub fn load(app_dir: &Path) -> AppSettings {
    std::fs::read_to_string(settings_file(app_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app_dir: &Path, settings: &AppSettings) -> std::io::Result<()> {
    std::fs::create_dir_all(app_dir)?;
    let json = serde_json::to_string(settings).expect("AppSettings serializes infallibly");
    std::fs::write(settings_file(app_dir), json)
}

pub fn set_auto_sync_enabled(app_dir: &Path, enabled: bool) -> std::io::Result<()> {
    let mut settings = load(app_dir);
    settings.auto_sync_enabled = enabled;
    save(app_dir, &settings)
}

pub fn set_notifications_enabled(app_dir: &Path, enabled: bool) -> std::io::Result<()> {
    let mut settings = load(app_dir);
    settings.notifications_enabled = enabled;
    save(app_dir, &settings)
}

pub fn set_minimize_to_tray_enabled(app_dir: &Path, enabled: bool) -> std::io::Result<()> {
    let mut settings = load(app_dir);
    settings.minimize_to_tray_enabled = enabled;
    save(app_dir, &settings)
}

/// Irreversibly delete the settings file, e.g. as part of a duress/logout
/// wipe. Best-effort: a missing file is not an error.
pub fn wipe(app_dir: &Path) {
    let _ = std::fs::remove_file(settings_file(app_dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-settings-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_to_auto_sync_enabled() {
        let dir = temp_dir("defaults");
        assert!(load(&dir).auto_sync_enabled);
        assert!(load(&dir).notifications_enabled);
        assert!(load(&dir).minimize_to_tray_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_auto_sync_persists() {
        let dir = temp_dir("persists");
        set_auto_sync_enabled(&dir, false).unwrap();
        assert!(!load(&dir).auto_sync_enabled);
        set_auto_sync_enabled(&dir, true).unwrap();
        assert!(load(&dir).auto_sync_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_notifications_persists_independently_of_auto_sync() {
        let dir = temp_dir("notif-persists");
        set_auto_sync_enabled(&dir, true).unwrap();
        set_notifications_enabled(&dir, false).unwrap();
        let settings = load(&dir);
        assert!(!settings.notifications_enabled);
        assert!(settings.auto_sync_enabled, "turning off notifications must not touch auto-sync");
        set_notifications_enabled(&dir, true).unwrap();
        assert!(load(&dir).notifications_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_minimize_to_tray_persists() {
        let dir = temp_dir("tray-persists");
        set_minimize_to_tray_enabled(&dir, false).unwrap();
        assert!(!load(&dir).minimize_to_tray_enabled);
        set_minimize_to_tray_enabled(&dir, true).unwrap();
        assert!(load(&dir).minimize_to_tray_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wipe_resets_to_default() {
        let dir = temp_dir("wipe");
        set_auto_sync_enabled(&dir, false).unwrap();
        set_notifications_enabled(&dir, false).unwrap();
        set_minimize_to_tray_enabled(&dir, false).unwrap();
        wipe(&dir);
        assert!(load(&dir).auto_sync_enabled);
        assert!(load(&dir).notifications_enabled);
        assert!(load(&dir).minimize_to_tray_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
