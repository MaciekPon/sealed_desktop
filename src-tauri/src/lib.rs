// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri::Manager;

mod alias;
mod chain;
mod commands;
mod constants;
mod contacts;
mod credits;
mod crypto;
mod db;
mod dek;
mod indexer;
mod keys;
mod messages;
mod messaging;
mod ohttp;
mod settings;
mod state;
mod sync;
mod sync_state;
mod termination;
mod username;
mod zk;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_local_data_dir()
                .expect("could not resolve app local data path");
            std::fs::create_dir_all(&app_dir)?;

            // Registered for parity with the plan (using tauri-plugin-stronghold
            // as the secure-storage backbone) and left available for incidental
            // JS-side use — but `dek::Vault` talks to the underlying
            // `iota-stronghold` crate directly (see its module doc comment), so
            // this plugin's own commands are not on the path for any secret
            // material in this app.
            let salt_path = app_dir.join("stronghold-salt.txt");
            app.handle()
                .plugin(tauri_plugin_stronghold::Builder::with_argon2(&salt_path).build())?;

            app.manage(AppState::new(app_dir));
            sync::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::has_existing_account,
            commands::auth::is_unlocked,
            commands::auth::create_account,
            commands::auth::restore_account,
            commands::auth::unlock_account,
            commands::auth::lock_account,
            commands::auth::change_pin,
            commands::username::claim_username,
            commands::username::release_username,
            commands::username::resolve_username,
            commands::username::check_username_available,
            commands::username::search_usernames,
            commands::credits::get_credits,
            commands::credits::estimate_credit_cost,
            commands::credits::redeem_code,
            commands::keys::ensure_keys_published,
            commands::wallet::get_wallet_balance,
            commands::settings::get_app_settings,
            commands::settings::set_auto_sync_enabled,
            commands::settings::is_termination_configured,
            commands::settings::set_termination_code,
            commands::settings::disable_termination_code,
            commands::settings::verify_termination_code,
            commands::settings::verify_pin,
            commands::settings::get_seed_phrase_for_backup,
            commands::settings::log_out,
            commands::contacts::save_contact,
            commands::contacts::get_contact,
            commands::contacts::get_all_contacts,
            commands::contacts::delete_contact,
            commands::contacts::clear_contacts,
            commands::contacts::search_contact,
            commands::contacts::search_contacts,
            commands::contacts::get_contact_keys,
            commands::contacts::save_contact_keys,
            commands::contacts::resolve_contact_keys,
            commands::contacts::add_to_contacts,
            commands::contacts::remove_from_contacts,
            commands::contacts::block_contact,
            commands::contacts::unblock_contact,
            commands::messaging::send_message,
            commands::messaging::sync_messages,
            commands::messaging::force_resync,
            commands::messaging::get_conversation,
            commands::messaging::get_all_conversations,
            commands::messaging::mark_conversation_as_read,
            commands::messaging::get_unread_count,
            commands::messaging::delete_conversation,
            commands::alias::create_invite,
            commands::alias::list_pending_invites,
            commands::alias::dismiss_pending_invite,
            commands::alias::delete_pending_invite,
            commands::alias::accept_invite,
            commands::alias::complete_invite,
            commands::alias::send_alias_message,
            commands::alias::get_alias_contacts,
            commands::alias::get_alias_conversations,
            commands::alias::get_alias_conversation,
            commands::alias::mark_alias_conversation_read,
            commands::alias::rename_alias_contact,
            commands::alias::delete_alias_contact,
            commands::alias::create_invite_for_contact,
            commands::alias::list_incoming_invites,
            commands::alias::accept_incoming_invite,
            commands::alias::decline_incoming_invite,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
