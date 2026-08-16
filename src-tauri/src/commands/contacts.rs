//! Contact commands: cache CRUD/search plus the lazy on-chain/indexer key
//! resolver. Mirrors `local/repositories/contact_repository.dart`.
//!
//! The plain CRUD/search delegates straight to `crate::contacts` (pure
//! `rusqlite::Connection` functions, unit-tested there against an in-memory
//! `Db`). Only the lazy resolve path lives here, since it needs the active
//! session's chain/indexer clients — ports `_resolveProfile` /
//! `_fetchAndVerifyPqPubkey` / `_deriveProfileFromWallet`.

use sha2::{Digest, Sha256};
use tauri::State;

use crate::contacts::{self, ContactKeys, ContactProfile};
use crate::state::AppState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactProfileDto {
    pub wallet_address: String,
    pub username: Option<String>,
    pub encryption_pubkey: String, // base64
    pub scan_pubkey: String,       // base64
    pub pq_public_key: Option<String>, // base64
    pub pq_pubkey_hash: Option<String>, // base64
    pub created_at: i64,
    /// Read-only in practice: `save_contact` ignores this field on write
    /// (see `ContactProfile::is_contact`'s doc comment) — use
    /// `add_to_contacts`/`remove_from_contacts` to change it.
    pub is_contact: bool,
    pub is_blocked: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactKeysDto {
    pub pq_public_key: Option<String>,    // base64
    pub pq_shared_secret: Option<String>, // base64
    pub encryption_pubkey: Option<String>, // base64
    pub scan_pubkey: Option<String>,      // base64
    pub username: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactKeysUpdateDto {
    pub pq_public_key: Option<String>,
    pub pq_shared_secret: Option<String>,
    pub encryption_pubkey: Option<String>,
    pub scan_pubkey: Option<String>,
}

fn b64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).map_err(|e| e.to_string())
}

fn b64_decode_32(s: &str) -> Result<[u8; 32], String> {
    b64_decode(s)?.try_into().map_err(|_| "expected 32 bytes".to_string())
}

fn profile_to_dto(p: &ContactProfile) -> ContactProfileDto {
    ContactProfileDto {
        wallet_address: p.wallet_address.clone(),
        username: p.username.clone(),
        encryption_pubkey: b64_encode(&p.encryption_pubkey),
        scan_pubkey: b64_encode(&p.scan_pubkey),
        pq_public_key: p.pq_public_key.as_deref().map(b64_encode),
        pq_pubkey_hash: p.pq_pubkey_hash.map(|h| b64_encode(&h)),
        created_at: p.created_at,
        is_contact: p.is_contact,
        is_blocked: p.is_blocked,
    }
}

fn dto_to_profile(dto: &ContactProfileDto) -> Result<ContactProfile, String> {
    Ok(ContactProfile {
        wallet_address: dto.wallet_address.clone(),
        username: dto.username.clone(),
        encryption_pubkey: b64_decode_32(&dto.encryption_pubkey)?,
        scan_pubkey: b64_decode_32(&dto.scan_pubkey)?,
        pq_public_key: dto.pq_public_key.as_deref().map(b64_decode).transpose()?,
        pq_pubkey_hash: dto.pq_pubkey_hash.as_deref().map(b64_decode_32).transpose()?,
        created_at: dto.created_at,
        // Ignored by `contacts::save_contact` on write regardless of what's
        // set here — see `ContactProfile::is_contact`'s doc comment.
        is_contact: dto.is_contact,
        is_blocked: dto.is_blocked,
    })
}

fn keys_to_dto(k: &ContactKeys) -> ContactKeysDto {
    ContactKeysDto {
        pq_public_key: k.pq_public_key.as_deref().map(b64_encode),
        pq_shared_secret: k.pq_shared_secret.as_deref().map(b64_encode),
        encryption_pubkey: k.encryption_pubkey.map(|k| b64_encode(&k)),
        scan_pubkey: k.scan_pubkey.map(|k| b64_encode(&k)),
        username: k.username.clone(),
    }
}

#[tauri::command]
pub async fn save_contact(state: State<'_, AppState>, profile: ContactProfileDto) -> Result<(), String> {
    let profile = dto_to_profile(&profile)?;
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::save_contact(session.db.connection(), &profile).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_contact(state: State<'_, AppState>, wallet_address: String) -> Result<Option<ContactProfileDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::get_contact(session.db.connection(), &wallet_address)
        .map(|opt| opt.as_ref().map(profile_to_dto))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_all_contacts(state: State<'_, AppState>) -> Result<Vec<ContactProfileDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::get_all_contacts(session.db.connection())
        .map(|rows| rows.iter().map(profile_to_dto).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_contact(state: State<'_, AppState>, wallet_address: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::delete_contact(session.db.connection(), &wallet_address).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_contacts(state: State<'_, AppState>) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::clear(session.db.connection()).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_contact(state: State<'_, AppState>, query: String) -> Result<Option<ContactProfileDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::search_contact(session.db.connection(), &query)
        .map(|opt| opt.as_ref().map(profile_to_dto))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_contacts(state: State<'_, AppState>, query: String, limit: u32) -> Result<Vec<ContactProfileDto>, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::search_contacts(session.db.connection(), &query, limit)
        .map(|rows| rows.iter().map(profile_to_dto).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_contact_keys(state: State<'_, AppState>, wallet_address: String) -> Result<ContactKeysDto, String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::get_contact_keys(session.db.connection(), &wallet_address).map(|k| keys_to_dto(&k)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_contact_keys(state: State<'_, AppState>, wallet_address: String, update: ContactKeysUpdateDto) -> Result<(), String> {
    let update = crate::contacts::ContactKeysUpdate {
        pq_public_key: update.pq_public_key.as_deref().map(b64_decode).transpose()?,
        pq_shared_secret: update.pq_shared_secret.as_deref().map(b64_decode).transpose()?,
        encryption_pubkey: update.encryption_pubkey.as_deref().map(b64_decode_32).transpose()?,
        scan_pubkey: update.scan_pubkey.as_deref().map(b64_decode_32).transpose()?,
    };
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::save_contact_keys(session.db.connection(), &wallet_address, &update).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

/// Flag `wallet_address` as a deliberately-added contact (Chats tab, not
/// Spam). If no cached row exists yet — the peer was never messaged and
/// never explicitly added before — lazily resolves their profile from
/// chain/indexer (or the classical wallet-derived fallback) and creates one
/// first. Mirrors `addToContacts`'s fallback branch in
/// `contact_repository.dart`.
#[tauri::command]
pub async fn add_to_contacts(state: State<'_, AppState>, wallet_address: String) -> Result<(), String> {
    add_to_contacts_impl(&state, &wallet_address).await
}

pub(crate) async fn add_to_contacts_impl(state: &AppState, wallet_address: &str) -> Result<(), String> {
    let updated = {
        let session_guard = state.session.lock().await;
        let session = session_guard.as_ref().ok_or("not unlocked")?;
        contacts::set_is_contact(session.db.connection(), wallet_address, true).map_err(|e| e.to_string())?
    };

    // If a row already existed, the flag write above is done — nothing else
    // to do. Otherwise resolve-and-create, each step below in its own lock
    // scope: `Session` holds the Stronghold `Vault` (`!Sync`), so the guard
    // must never be held across an `.await` while also being touched again
    // afterward — otherwise the `!Sync` `Vault` poisons this fn's future as
    // `!Send`, which `#[tauri::command]` requires. Mirrors how
    // `resolve_contact_keys` above avoids the same trap.
    let new_profile = if updated == 0 {
        let resolved = {
            let session_guard = state.session.lock().await;
            let session = session_guard.as_ref().ok_or("not unlocked")?;
            resolve_profile(&session.chain_client, &session.indexer_client, wallet_address).await
        }
        .ok_or_else(|| "could not resolve this contact's profile".to_string())?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Some(ContactProfile {
            wallet_address: wallet_address.to_string(),
            username: resolved.username,
            encryption_pubkey: resolved.encryption_pubkey,
            scan_pubkey: resolved.scan_pubkey,
            pq_public_key: resolved.pq_public_key,
            pq_pubkey_hash: None,
            created_at: now,
            is_contact: false,
            is_blocked: false,
        })
    } else {
        None
    };

    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    if let Some(profile) = &new_profile {
        contacts::save_contact(session.db.connection(), profile).map_err(|e| e.to_string())?;
        contacts::set_is_contact(session.db.connection(), wallet_address, true).map_err(|e| e.to_string())?;
    }
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

/// Un-flag `wallet_address` as a contact (moves it out of Chats). No-op if
/// no cached row exists.
#[tauri::command]
pub async fn remove_from_contacts(state: State<'_, AppState>, wallet_address: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::set_is_contact(session.db.connection(), &wallet_address, false).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

/// Block `wallet_address` (Spam tab; also clears `is_contact`). No-op if no
/// cached row exists — see `contacts::set_is_blocked`.
#[tauri::command]
pub async fn block_contact(state: State<'_, AppState>, wallet_address: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::set_is_blocked(session.db.connection(), &wallet_address, true).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

/// Unblock `wallet_address`. Deliberately does **not** restore `is_contact`
/// — the user must explicitly re-add via `add_to_contacts`.
#[tauri::command]
pub async fn unblock_contact(state: State<'_, AppState>, wallet_address: String) -> Result<(), String> {
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    contacts::set_is_blocked(session.db.connection(), &wallet_address, false).map_err(|e| e.to_string())?;
    session.db.save(&session.dek).map_err(|e| e.to_string())
}

/// Resolve whatever key material for `wallet_address` isn't already
/// cached, via the chain (published keys) then the indexer (PQ pubkey,
/// verified against the on-chain hash anchor), falling back to deriving
/// classical X25519 keys from the Ed25519 wallet address for unregistered
/// users. Pure resolver — does not persist; the caller decides whether to
/// feed the result into `save_contact_keys`.
#[tauri::command]
pub async fn resolve_contact_keys(state: State<'_, AppState>, wallet_address: String) -> Result<ContactKeysDto, String> {
    // Only borrow the specific fields the resolver needs (chain/indexer
    // clients, cached keys), not `&Session` as a whole: `Session` also
    // holds the Stronghold `Vault`, which isn't `Sync`, so any `async fn`
    // taking `&Session` across an `.await` makes the returned future
    // non-`Send` — which `#[tauri::command]` requires.
    let session_guard = state.session.lock().await;
    let session = session_guard.as_ref().ok_or("not unlocked")?;
    let cached = contacts::get_contact_keys(session.db.connection(), &wallet_address).map_err(|e| e.to_string())?;
    let chain_client = &session.chain_client;
    let indexer_client = &session.indexer_client;

    resolve_contact_keys_impl(chain_client, indexer_client, &wallet_address, cached).await.map(|k| keys_to_dto(&k))
}

/// A subset of an on-chain/derived profile relevant to key resolution
/// only — deliberately narrower than `chain::client::UserProfile` (no
/// username/pq_pubkey_hash carried forward, since the caller only wants
/// key material here).
struct ResolvedKeys {
    encryption_pubkey: [u8; 32],
    scan_pubkey: [u8; 32],
    pq_public_key: Option<Vec<u8>>,
    /// `None` for the wallet-derived fallback (unregistered wallets have no
    /// username by definition) as well as for a registered wallet that
    /// simply hasn't claimed one.
    username: Option<String>,
}

/// `pub(crate)`, not private: `messaging.rs`'s `send_message` needs the
/// same lazy resolve behavior mobile's `sendMessage` gets for free from
/// `ContactRepository.getContactKeys` doing the resolve inline — rather
/// than duplicate this logic, it calls straight into this function too.
pub(crate) async fn resolve_contact_keys_impl(
    chain_client: &crate::chain::client::SealedChainClient,
    indexer_client: &crate::indexer::client::IndexerClient,
    wallet_address: &str,
    cached: ContactKeys,
) -> Result<ContactKeys, String> {
    let has_classical_cached = cached.encryption_pubkey.is_some() && cached.scan_pubkey.is_some();
    if cached.pq_public_key.is_some() && has_classical_cached && cached.username.is_some() {
        return Ok(cached);
    }

    let Some(resolved) = resolve_profile(chain_client, indexer_client, wallet_address).await else {
        return Ok(cached);
    };

    Ok(ContactKeys {
        pq_public_key: cached.pq_public_key.or(resolved.pq_public_key),
        pq_shared_secret: cached.pq_shared_secret,
        encryption_pubkey: cached.encryption_pubkey.or(Some(resolved.encryption_pubkey)),
        scan_pubkey: cached.scan_pubkey.or(Some(resolved.scan_pubkey)),
        username: cached.username.or(resolved.username),
    })
}

/// Resolution order: on-chain published keys (verifying the PQ pubkey
/// against its on-chain hash anchor via the indexer, if any), then a
/// classical-only fallback derived from the Ed25519 wallet pubkey for
/// wallets with no on-chain registration. `None` only on unrecoverable
/// errors (chain read failure, malformed wallet address).
async fn resolve_profile(
    chain_client: &crate::chain::client::SealedChainClient,
    indexer_client: &crate::indexer::client::IndexerClient,
    wallet_address: &str,
) -> Option<ResolvedKeys> {
    let on_chain = chain_client.get_user_by_wallet(wallet_address).await.ok()?;

    if let Some(profile) = on_chain {
        let pq_public_key = match profile.pq_pubkey_hash {
            Some(anchor_hash) => fetch_and_verify_pq_pubkey(indexer_client, wallet_address, &anchor_hash).await,
            None => None,
        };
        return Some(ResolvedKeys {
            encryption_pubkey: profile.encryption_pubkey,
            scan_pubkey: profile.scan_pubkey,
            pq_public_key,
            username: profile.username,
        });
    }

    derive_profile_from_wallet(wallet_address)
}

async fn fetch_and_verify_pq_pubkey(
    indexer_client: &crate::indexer::client::IndexerClient,
    wallet_address: &str,
    anchor_hash: &[u8; 32],
) -> Option<Vec<u8>> {
    let resp = indexer_client.get_pq_public_key(wallet_address).await.ok()?;
    let actual_hash: [u8; 32] = Sha256::digest(&resp.pq_pubkey).into();
    if crate::crypto::constant_time_equals(&actual_hash, anchor_hash) {
        Some(resp.pq_pubkey)
    } else {
        None
    }
}

fn derive_profile_from_wallet(wallet_address: &str) -> Option<ResolvedKeys> {
    let pubkey = crate::chain::address::decode_address(wallet_address)?;
    let derived = crate::crypto::x25519::ed25519_public_key_to_x25519(&pubkey)?;
    Some(ResolvedKeys { encryption_pubkey: derived, scan_pubkey: derived, pq_public_key: None, username: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dto_round_trip_preserves_bytes() {
        let profile = ContactProfile {
            wallet_address: "WALLET1".to_string(),
            username: Some("alice".to_string()),
            encryption_pubkey: [1u8; 32],
            scan_pubkey: [2u8; 32],
            pq_public_key: Some(vec![3u8; 800]),
            pq_pubkey_hash: Some([4u8; 32]),
            created_at: 1_000,
            is_contact: true,
            is_blocked: false,
        };
        let dto = profile_to_dto(&profile);
        let round_tripped = dto_to_profile(&dto).unwrap();
        assert_eq!(round_tripped.wallet_address, profile.wallet_address);
        assert_eq!(round_tripped.encryption_pubkey, profile.encryption_pubkey);
        assert_eq!(round_tripped.pq_public_key, profile.pq_public_key);
        assert_eq!(round_tripped.pq_pubkey_hash, profile.pq_pubkey_hash);
        assert_eq!(round_tripped.is_contact, profile.is_contact);
        assert_eq!(round_tripped.is_blocked, profile.is_blocked);
    }

    /// `derive_profile_from_wallet` is the classical-only fallback used
    /// when `resolve_profile` finds no on-chain registration for a
    /// wallet — this exercises it directly and hermetically (no network,
    /// no session), rather than through `resolve_contact_keys_impl`, which
    /// would need a live chain/indexer to reach that branch.
    #[test]
    fn derive_profile_from_wallet_sets_matching_encryption_and_scan_pubkeys() {
        let wallet_address = crate::chain::address::encode_address(&[9u8; 32]);
        let derived = derive_profile_from_wallet(&wallet_address).expect("valid wallet address must derive a fallback key");
        assert_eq!(derived.encryption_pubkey, derived.scan_pubkey);
        assert!(derived.pq_public_key.is_none());
    }

    #[test]
    fn derive_profile_from_wallet_rejects_malformed_address() {
        assert!(derive_profile_from_wallet("not-a-valid-address").is_none());
    }
}
