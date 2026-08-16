//! App-wide preferences, mirroring `services/app_settings_service.dart`.
//!
//! Desktop only keeps `auto_sync_enabled`. The Dart service's other two
//! flags don't carry over: `preferredSyncLayer` has exactly one real value
//! (`SyncLayer.blockchain` — `auto` just falls back to it) so there is
//! nothing for a desktop toggle to select between, and `targetedPushEnabled`
//! gates an OS push-token registration flow that doesn't exist on desktop
//! (background poll + native toast replaces push entirely — see the plan).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub auto_sync_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { auto_sync_enabled: true }
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
    fn wipe_resets_to_default() {
        let dir = temp_dir("wipe");
        set_auto_sync_enabled(&dir, false).unwrap();
        wipe(&dir);
        assert!(load(&dir).auto_sync_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
