//! System tray icon + close-to-tray behavior (2026-09-01, desktop-only —
//! nothing on mobile to mirror).
//!
//! Two independent pieces:
//! 1. A tray icon with a "Show Sealed" / "Quit" menu, always present while
//!    the app is running (regardless of the setting below) — it's the only
//!    way back into a minimized-to-tray window, and a clean way to fully
//!    quit either way.
//! 2. Intercepting the main window's close button: if
//!    `settings::AppSettings::minimize_to_tray_enabled` is on (the
//!    default), closing the window hides it instead of quitting the
//!    process — sync/notifications keep running in the background exactly
//!    as before, since nothing about the process itself changes, only the
//!    window's visibility. If the setting is off, the close button behaves
//!    like any ordinary desktop app: closing the last window quits.
//!
//! Reads `AppState.app_dir` fresh on every close attempt (not cached at
//! startup) so toggling the setting in Settings takes effect immediately,
//! without needing a restart.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, WindowEvent};

use crate::settings;
use crate::state::AppState;

const MAIN_WINDOW_LABEL: &str = "main";

pub fn setup(app: &App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "Show Sealed", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().cloned().expect("bundle.icon is configured in tauri.conf.json"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Sealed")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click the icon itself (not the menu, which has its own
            // handler above) also just shows the window — the common case,
            // matches how most tray apps behave.
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let app_handle = app.handle().clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app_dir = &app_handle.state::<AppState>().app_dir;
                if settings::load(app_dir).minimize_to_tray_enabled {
                    api.prevent_close();
                    if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
                        let _ = window.hide();
                    }
                }
                // Setting is off: don't call `prevent_close()` — the window
                // (the app's only window) closes normally, which is Tauri's
                // default "quit when the last window closes" behavior.
            }
        });
    }

    Ok(())
}

/// Also called from `sync::notify_new_messages` (Windows only, see its doc
/// comment) so clicking the new-message toast brings the window back too.
pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
