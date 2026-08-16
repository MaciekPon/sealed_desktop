//! Tauri command surface — thin wrappers over `crypto`/`dek`/`db`/`chain`/
//! `ohttp`/`indexer`, tying them together via the session in
//! [`crate::state::AppState`]. This is the only layer the frontend talks
//! to; no secret material crosses into JS.

pub mod alias;
pub mod auth;
pub mod contacts;
pub mod credits;
pub mod keys;
pub mod messaging;
pub mod settings;
pub mod username;
pub mod wallet;
