//! Contact cache repository, ports `local/repositories/contact_repository.dart`
//! (the `contacts_cache` table half of it — pure CRUD/search over
//! `rusqlite::Connection`; the async lazy on-chain/indexer resolve lives in
//! `commands::contacts`, which is the only caller that has a session to
//! borrow a chain/indexer client from).
//!
//! Two fields the Dart model touches are deliberately dropped here:
//! `contacts_cache` has no `alias` column (`UserProfile.toMap()`/`fromMap()`
//! read/write an `'alias'` key that was never added to the schema — see
//! `sealed_app/lib/local/database.dart`'s `_onCreate`/`_onUpgrade`, which
//! only ever add `display_name`; inserting through `ContactRepositoryImpl`
//! on mobile would hit "table contacts_cache has no column named alias" —
//! not reproduced here), and `display_name` itself, which the schema has
//! but neither `toMap` nor `fromMap` ever populate. Both are simply unused
//! by the real CRUD path, so this port only carries the columns that path
//! actually reads and writes.

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone)]
pub struct ContactProfile {
    pub wallet_address: String,
    pub username: Option<String>,
    pub encryption_pubkey: [u8; 32],
    pub scan_pubkey: [u8; 32],
    pub pq_public_key: Option<Vec<u8>>,
    pub pq_pubkey_hash: Option<[u8; 32]>,
    /// Unix seconds.
    pub created_at: i64,
    /// Reflects current DB state on read. **Not honored by [`save_contact`]
    /// on write** — that function always preserves whatever's already
    /// stored (or `false` for a brand-new row) regardless of this field,
    /// mirroring `saveContact`'s explicit preserve-across-REPLACE in
    /// `contact_repository.dart` (a profile refresh must not silently reset
    /// the user's manual-contact flag). Change it only via [`set_is_contact`].
    pub is_contact: bool,
    /// Same write caveat as `is_contact`. Change only via [`set_is_blocked`].
    pub is_blocked: bool,
}

/// Whatever subset of a contact's keys is currently cached. Missing keys
/// are `None`. Mirrors `ContactKeys` (`local/repositories/contact_keys.dart`).
#[derive(Debug, Clone, Default)]
pub struct ContactKeys {
    pub pq_public_key: Option<Vec<u8>>,
    pub pq_shared_secret: Option<Vec<u8>>,
    pub encryption_pubkey: Option<[u8; 32]>,
    pub scan_pubkey: Option<[u8; 32]>,
    pub username: Option<String>,
}

/// Any subset of keys to persist independently — `None` fields leave the
/// existing stored value untouched. Mirrors `saveContactKeys`'s named
/// parameters.
#[derive(Debug, Clone, Default)]
pub struct ContactKeysUpdate {
    pub pq_public_key: Option<Vec<u8>>,
    pub pq_shared_secret: Option<Vec<u8>>,
    pub encryption_pubkey: Option<[u8; 32]>,
    pub scan_pubkey: Option<[u8; 32]>,
}

const PROFILE_COLUMNS: &str =
    "wallet_address, username, encryption_pubkey, scan_pubkey, created_at, pq_public_key, pq_pubkey_hash, is_contact, is_blocked";

fn row_to_profile(row: &rusqlite::Row) -> rusqlite::Result<ContactProfile> {
    let encryption_pubkey: Vec<u8> = row.get("encryption_pubkey")?;
    let scan_pubkey: Vec<u8> = row.get("scan_pubkey")?;
    let pq_pubkey_hash: Option<Vec<u8>> = row.get("pq_pubkey_hash")?;
    Ok(ContactProfile {
        wallet_address: row.get("wallet_address")?,
        username: row.get("username")?,
        encryption_pubkey: encryption_pubkey.try_into().unwrap_or([0u8; 32]),
        scan_pubkey: scan_pubkey.try_into().unwrap_or([0u8; 32]),
        pq_public_key: row.get("pq_public_key")?,
        pq_pubkey_hash: pq_pubkey_hash.and_then(|v| v.try_into().ok()),
        created_at: row.get("created_at")?,
        is_contact: row.get("is_contact")?,
        is_blocked: row.get("is_blocked")?,
    })
}

/// Insert or fully replace the cached profile for `profile.wallet_address`.
/// Matches `ConflictAlgorithm.replace` semantics: columns not passed here
/// (`display_name`, `pq_shared_secret`, `cached_at`) revert to their
/// defaults/NULL on an overwrite, same as the Dart `toMap()`-driven insert —
/// **except** `is_contact`/`is_blocked`, which are read back first and
/// re-applied so a profile refresh never resets them (see the doc comment
/// on [`ContactProfile::is_contact`]).
pub fn save_contact(conn: &Connection, profile: &ContactProfile) -> rusqlite::Result<()> {
    let existing_flags: Option<(bool, bool)> = conn
        .query_row(
            "SELECT is_contact, is_blocked FROM contacts_cache WHERE wallet_address = ?1",
            params![profile.wallet_address],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (is_contact, is_blocked) = existing_flags.unwrap_or((false, false));

    conn.execute(
        "INSERT OR REPLACE INTO contacts_cache \
         (wallet_address, username, encryption_pubkey, scan_pubkey, created_at, pq_public_key, pq_pubkey_hash, is_contact, is_blocked) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            profile.wallet_address,
            profile.username,
            profile.encryption_pubkey.to_vec(),
            profile.scan_pubkey.to_vec(),
            profile.created_at,
            profile.pq_public_key,
            profile.pq_pubkey_hash.map(|h| h.to_vec()),
            is_contact,
            is_blocked,
        ],
    )?;
    Ok(())
}

pub fn get_contact(conn: &Connection, wallet_address: &str) -> rusqlite::Result<Option<ContactProfile>> {
    conn.query_row(
        &format!("SELECT {PROFILE_COLUMNS} FROM contacts_cache WHERE wallet_address = ?1"),
        params![wallet_address],
        row_to_profile,
    )
    .optional()
}

pub fn get_all_contacts(conn: &Connection) -> rusqlite::Result<Vec<ContactProfile>> {
    let mut stmt = conn.prepare(&format!("SELECT {PROFILE_COLUMNS} FROM contacts_cache"))?;
    let rows = stmt.query_map([], row_to_profile)?;
    rows.collect()
}

pub fn delete_contact(conn: &Connection, wallet_address: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM contacts_cache WHERE wallet_address = ?1", params![wallet_address])?;
    Ok(())
}

pub fn clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM contacts_cache", [])?;
    Ok(())
}

/// Exact match: wallet address, then exact username.
pub fn search_contact(conn: &Connection, query: &str) -> rusqlite::Result<Option<ContactProfile>> {
    if let Some(hit) = get_contact(conn, query)? {
        return Ok(Some(hit));
    }
    conn.query_row(
        &format!("SELECT {PROFILE_COLUMNS} FROM contacts_cache WHERE username = ?1"),
        params![query],
        row_to_profile,
    )
    .optional()
}

/// Fuzzy match over username and wallet address, newest-cached first.
pub fn search_contacts(conn: &Connection, query: &str, limit: u32) -> rusqlite::Result<Vec<ContactProfile>> {
    let pattern = format!("%{query}%");
    let mut stmt = conn.prepare(&format!(
        "SELECT {PROFILE_COLUMNS} FROM contacts_cache WHERE username LIKE ?1 OR wallet_address LIKE ?1 \
         ORDER BY cached_at DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![pattern, limit], row_to_profile)?;
    rows.collect()
}

/// Manual-contact flag. `contacts_cache` auto-populates on send/receive;
/// `is_contact = true` marks rows the user deliberately added (e.g. via
/// "Start Chat"). Returns the number of rows updated (0 if no cached row
/// exists yet for `wallet_address`) — callers needing a lazily-created row
/// (see `commands::contacts::add_to_contacts`) check this.
pub fn set_is_contact(conn: &Connection, wallet_address: &str, is_contact: bool) -> rusqlite::Result<usize> {
    conn.execute("UPDATE contacts_cache SET is_contact = ?1 WHERE wallet_address = ?2", params![is_contact, wallet_address])
}

/// Persistent manual block. Blocking also clears `is_contact` (removes from
/// Chats); unblocking deliberately does **not** restore it — the user must
/// explicitly re-add, matching `blockContact`'s semantics in
/// `contact_repository.dart`. No-op if no cached row exists.
pub fn set_is_blocked(conn: &Connection, wallet_address: &str, is_blocked: bool) -> rusqlite::Result<usize> {
    if is_blocked {
        conn.execute(
            "UPDATE contacts_cache SET is_blocked = 1, is_contact = 0 WHERE wallet_address = ?1",
            params![wallet_address],
        )
    } else {
        conn.execute("UPDATE contacts_cache SET is_blocked = 0 WHERE wallet_address = ?1", params![wallet_address])
    }
}

/// Cache-only key lookup — the lazy on-chain/indexer fallback lives in
/// `commands::contacts::resolve_contact_keys`, which has an active session
/// to borrow a chain/indexer client from.
pub fn get_contact_keys(conn: &Connection, wallet_address: &str) -> rusqlite::Result<ContactKeys> {
    let row = conn
        .query_row(
            "SELECT pq_public_key, pq_shared_secret, encryption_pubkey, scan_pubkey, username \
             FROM contacts_cache WHERE wallet_address = ?1",
            params![wallet_address],
            |row| {
                let encryption_pubkey: Option<Vec<u8>> = row.get(2)?;
                let scan_pubkey: Option<Vec<u8>> = row.get(3)?;
                Ok(ContactKeys {
                    pq_public_key: row.get(0)?,
                    pq_shared_secret: row.get(1)?,
                    encryption_pubkey: encryption_pubkey.and_then(|v| v.try_into().ok()),
                    scan_pubkey: scan_pubkey.and_then(|v| v.try_into().ok()),
                    username: row.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(row.unwrap_or_default())
}

/// Save any subset of keys independently — fields left `None` in `update`
/// leave the existing stored value untouched. No-op (and no row created)
/// if `update` is entirely empty or the wallet has no cached row yet,
/// matching `saveContactKeys`'s "update-only" semantics.
pub fn save_contact_keys(conn: &Connection, wallet_address: &str, update: &ContactKeysUpdate) -> rusqlite::Result<()> {
    let mut sets = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(pq_public_key) = &update.pq_public_key {
        sets.push("pq_public_key = ?");
        values.push(Box::new(pq_public_key.clone()));
    }
    if let Some(pq_shared_secret) = &update.pq_shared_secret {
        sets.push("pq_shared_secret = ?");
        values.push(Box::new(pq_shared_secret.clone()));
    }
    if let Some(encryption_pubkey) = &update.encryption_pubkey {
        sets.push("encryption_pubkey = ?");
        values.push(Box::new(encryption_pubkey.to_vec()));
    }
    if let Some(scan_pubkey) = &update.scan_pubkey {
        sets.push("scan_pubkey = ?");
        values.push(Box::new(scan_pubkey.to_vec()));
    }
    if sets.is_empty() {
        return Ok(());
    }

    let sql = format!("UPDATE contacts_cache SET {} WHERE wallet_address = ?", sets.join(", "));
    values.push(Box::new(wallet_address.to_string()));
    let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|b| b.as_ref()).collect();
    conn.execute(&sql, params.as_slice())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-contacts-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&dir.join("test.db"), &[7u8; 32]).unwrap()
    }

    fn sample(wallet: &str) -> ContactProfile {
        ContactProfile {
            wallet_address: wallet.to_string(),
            username: Some("alice".to_string()),
            encryption_pubkey: [1u8; 32],
            scan_pubkey: [2u8; 32],
            pq_public_key: Some(vec![3u8; 800]),
            pq_pubkey_hash: Some([4u8; 32]),
            created_at: 1_000,
            // Ignored by save_contact regardless of what's set here — see
            // the doc comment on `ContactProfile::is_contact`.
            is_contact: false,
            is_blocked: false,
        }
    }

    #[test]
    fn save_then_get_round_trip() {
        let db = temp_db("save-get");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();

        let fetched = get_contact(conn, "WALLET1").unwrap().unwrap();
        assert_eq!(fetched.username.as_deref(), Some("alice"));
        assert_eq!(fetched.encryption_pubkey, [1u8; 32]);
        assert_eq!(fetched.pq_public_key.as_deref(), Some([3u8; 800].as_ref()));

        assert!(get_contact(conn, "NOPE").unwrap().is_none());
    }

    #[test]
    fn replace_on_conflict_resets_omitted_columns() {
        let db = temp_db("replace");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();
        save_contact_keys(
            conn,
            "WALLET1",
            &ContactKeysUpdate { pq_shared_secret: Some(vec![9u8; 32]), ..Default::default() },
        )
        .unwrap();
        assert_eq!(get_contact_keys(conn, "WALLET1").unwrap().pq_shared_secret, Some(vec![9u8; 32]));

        // Re-saving the profile (as ContactRepositoryImpl.saveContact would
        // on a refresh) replaces the whole row — pq_shared_secret, which
        // save_contact never writes, reverts to NULL.
        save_contact(conn, &sample("WALLET1")).unwrap();
        assert_eq!(get_contact_keys(conn, "WALLET1").unwrap().pq_shared_secret, None);
    }

    #[test]
    fn delete_and_clear() {
        let db = temp_db("delete-clear");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();
        save_contact(conn, &sample("WALLET2")).unwrap();

        delete_contact(conn, "WALLET1").unwrap();
        assert!(get_contact(conn, "WALLET1").unwrap().is_none());
        assert!(get_contact(conn, "WALLET2").unwrap().is_some());

        clear(conn).unwrap();
        assert!(get_all_contacts(conn).unwrap().is_empty());
    }

    #[test]
    fn search_contact_exact_wallet_then_username() {
        let db = temp_db("search-exact");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();

        assert_eq!(search_contact(conn, "WALLET1").unwrap().unwrap().wallet_address, "WALLET1");
        assert_eq!(search_contact(conn, "alice").unwrap().unwrap().wallet_address, "WALLET1");
        assert!(search_contact(conn, "nobody").unwrap().is_none());
    }

    #[test]
    fn search_contacts_fuzzy_orders_by_cached_at_desc() {
        let db = temp_db("search-fuzzy");
        let conn = db.connection();
        let mut a = sample("WALLET_ALICE");
        a.username = Some("alice".into());
        let mut b = sample("WALLET_ALICIA");
        b.username = Some("alicia".into());
        save_contact(conn, &a).unwrap();
        // Ensure b gets a strictly later cached_at (column default is a
        // strftime('%s','now') second-granularity timestamp).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        save_contact(conn, &b).unwrap();

        let hits = search_contacts(conn, "alic", 20).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].wallet_address, "WALLET_ALICIA");
        assert_eq!(hits[1].wallet_address, "WALLET_ALICE");
    }

    #[test]
    fn contact_keys_partial_update_leaves_other_fields_untouched() {
        let db = temp_db("keys-partial");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();

        save_contact_keys(
            conn,
            "WALLET1",
            &ContactKeysUpdate { pq_shared_secret: Some(vec![9u8; 32]), ..Default::default() },
        )
        .unwrap();

        let keys = get_contact_keys(conn, "WALLET1").unwrap();
        assert_eq!(keys.pq_shared_secret, Some(vec![9u8; 32]));
        // encryption_pubkey/scan_pubkey/pq_public_key untouched by the
        // partial update.
        assert_eq!(keys.encryption_pubkey, Some([1u8; 32]));
        assert_eq!(keys.scan_pubkey, Some([2u8; 32]));
        assert_eq!(keys.pq_public_key, Some(vec![3u8; 800]));
    }

    #[test]
    fn contact_keys_missing_wallet_returns_all_none() {
        let db = temp_db("keys-missing");
        let conn = db.connection();
        let keys = get_contact_keys(conn, "GHOST").unwrap();
        assert_eq!(keys.pq_public_key, None);
        assert_eq!(keys.pq_shared_secret, None);
        assert_eq!(keys.encryption_pubkey, None);
        assert_eq!(keys.scan_pubkey, None);
    }

    #[test]
    fn fresh_contact_defaults_to_not_a_contact_not_blocked() {
        let db = temp_db("flags-default");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();
        let fetched = get_contact(conn, "WALLET1").unwrap().unwrap();
        assert!(!fetched.is_contact);
        assert!(!fetched.is_blocked);
    }

    #[test]
    fn set_is_contact_round_trips_and_reports_rows_affected() {
        let db = temp_db("set-is-contact");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();

        assert_eq!(set_is_contact(conn, "WALLET1", true).unwrap(), 1);
        assert!(get_contact(conn, "WALLET1").unwrap().unwrap().is_contact);

        assert_eq!(set_is_contact(conn, "WALLET1", false).unwrap(), 1);
        assert!(!get_contact(conn, "WALLET1").unwrap().unwrap().is_contact);

        // No cached row for this wallet — 0 rows affected, exactly what
        // `add_to_contacts`'s lazy-create fallback branches on.
        assert_eq!(set_is_contact(conn, "GHOST", true).unwrap(), 0);
    }

    #[test]
    fn save_contact_preserves_flags_across_replace() {
        let db = temp_db("preserve-flags");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();
        set_is_contact(conn, "WALLET1", true).unwrap();

        // Re-saving the profile (e.g. a chain-refresh) must not reset
        // is_contact back to false, even though `sample()` always passes
        // `is_contact: false` — this is the whole point of the preserve
        // behavior documented on `ContactProfile::is_contact`.
        save_contact(conn, &sample("WALLET1")).unwrap();
        assert!(get_contact(conn, "WALLET1").unwrap().unwrap().is_contact);
    }

    #[test]
    fn blocking_clears_is_contact_and_unblocking_does_not_restore_it() {
        let db = temp_db("block-semantics");
        let conn = db.connection();
        save_contact(conn, &sample("WALLET1")).unwrap();
        set_is_contact(conn, "WALLET1", true).unwrap();

        set_is_blocked(conn, "WALLET1", true).unwrap();
        let blocked = get_contact(conn, "WALLET1").unwrap().unwrap();
        assert!(blocked.is_blocked);
        assert!(!blocked.is_contact, "blocking must clear is_contact");

        set_is_blocked(conn, "WALLET1", false).unwrap();
        let unblocked = get_contact(conn, "WALLET1").unwrap().unwrap();
        assert!(!unblocked.is_blocked);
        assert!(!unblocked.is_contact, "unblocking must NOT restore is_contact");
    }

    #[test]
    fn set_is_blocked_is_a_noop_with_no_cached_row() {
        let db = temp_db("block-noop");
        let conn = db.connection();
        assert_eq!(set_is_blocked(conn, "GHOST", true).unwrap(), 0);
    }
}
