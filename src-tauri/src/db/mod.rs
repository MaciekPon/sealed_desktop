//! Local encrypted database, ported from `sealed_app/lib/local/database.dart`.
//!
//! Mobile uses SQLCipher (page-level transparent encryption). Building
//! SQLCipher's Rust bindings on this machine requires compiling OpenSSL from
//! source, which needs a fuller Perl toolchain than Git-for-Windows ships
//! (missing CPAN modules block `Configure`) — see the plan for the decision
//! to avoid that dependency. Instead: plain, unencrypted-in-memory SQLite
//! (`rusqlite`'s `bundled` feature — pure C, no OpenSSL), with the *whole
//! serialized database* encrypted at rest using the same AES-256-GCM
//! primitive already used everywhere else ([`crate::crypto::aead`]), keyed
//! by the DEK from the [`crate::dek`] vault. The database only ever exists
//! as plaintext in-process memory (via SQLite's serialize/deserialize C
//! API) — never as a plaintext file on disk.
//!
//! This is a fresh desktop install with no prior schema history to migrate
//! from (unlike mobile's v1->v14 ALTER-table ladder), so the schema below is
//! the *target* shape only (equivalent to mobile's v14), applied as a single
//! migration. `PRAGMA user_version` still gates it so future desktop-only
//! schema changes have somewhere to hook in.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, MAIN_DB};
use thiserror::Error;

use crate::crypto::aead;
use crate::crypto::CryptoError;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type DbResult<T> = Result<T, DbError>;

const CURRENT_SCHEMA_VERSION: i64 = 4;

/// A decrypted-in-memory handle onto the local database. Callers are
/// expected to hold this behind a mutex (wired up in the Tauri command
/// layer) and call [`Db::save`] after any write that must survive a
/// restart.
pub struct Db {
    conn: Connection,
    path: PathBuf,
}

impl Db {
    /// Open (decrypting, if the file already exists) or create the local
    /// database at `path`, using `dek` as the AES-256-GCM key for the
    /// at-rest encrypted file.
    pub fn open(path: &Path, dek: &[u8; 32]) -> DbResult<Self> {
        let mut conn = Connection::open_in_memory()?;

        if path.exists() {
            let encrypted = std::fs::read(path)?;
            let plaintext = aead::decrypt_combined(dek, &encrypted)?;
            let len = plaintext.len();
            conn.deserialize_read_exact(MAIN_DB, Cursor::new(plaintext), len, false)?;
        }

        apply_migrations(&conn)?;

        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// Serialize the current in-memory database, encrypt it, and write it
    /// to disk. Writes to a sibling temp file and renames over the target,
    /// so a crash mid-write can't corrupt the previously-saved copy.
    pub fn save(&self, dek: &[u8; 32]) -> DbResult<()> {
        let data = self.conn.serialize(MAIN_DB)?;
        let encrypted = aead::encrypt_combined(dek, &data)?;

        let tmp_path = self.path.with_extension("tmp");
        std::fs::write(&tmp_path, &encrypted)?;
        std::fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Needed by transactional multi-statement writes (`conn.transaction()`
    /// requires `&mut Connection`) — e.g. alias-contact promotion/deletion.
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

fn apply_migrations(conn: &Connection) -> DbResult<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version < 1 {
        create_schema_v1(conn)?;
    }
    if version < 2 {
        create_schema_v2(conn)?;
    }
    if version < 3 {
        create_schema_v3(conn)?;
    }
    if version < 4 {
        create_schema_v4(conn)?;
    }
    conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
    Ok(())
}

/// Target schema, equivalent to mobile's `local/database.dart` after its
/// v1->v14 migration ladder (see `_onCreate`).
fn create_schema_v1(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        r#"
        -- ================================================================
        -- MESSAGES TABLE
        -- ================================================================
        CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            sender_wallet TEXT NOT NULL,
            sender_username TEXT,
            recipient_wallet TEXT NOT NULL,
            recipient_username TEXT,
            content TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            is_outgoing INTEGER NOT NULL,
            is_read INTEGER NOT NULL DEFAULT 1,
            on_chain_pubkey TEXT,
            created_at INTEGER DEFAULT (strftime('%s', 'now')),
            legacy_origin INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX idx_messages_conversation
        ON messages(recipient_wallet, sender_wallet, timestamp DESC);

        -- ================================================================
        -- SYNC STATE TABLE
        -- ================================================================
        CREATE TABLE sync_state (
            key TEXT PRIMARY KEY,
            last_sync_timestamp INTEGER,
            last_processed_slot INTEGER
        );

        INSERT INTO sync_state (key, last_sync_timestamp, last_processed_slot)
        VALUES ('global', 0, 0);

        -- ================================================================
        -- INDEXER STATE TABLE
        -- ================================================================
        CREATE TABLE indexer_state (
            key TEXT PRIMARY KEY,
            view_key_registered INTEGER,
            push_token TEXT,
            last_indexed_sync INTEGER
        );

        -- ================================================================
        -- USER PROFILE TABLE (current logged-in user)
        -- ================================================================
        CREATE TABLE user_profile (
            wallet_address TEXT PRIMARY KEY,
            username TEXT,
            display_name TEXT,
            encryption_pubkey BLOB NOT NULL,
            scan_pubkey BLOB NOT NULL,
            pq_public_key BLOB,
            pq_pubkey_hash BLOB,
            created_at INTEGER NOT NULL,
            last_login INTEGER
        );

        -- ================================================================
        -- CONTACTS CACHE TABLE (cached peer profiles)
        -- ================================================================
        CREATE TABLE contacts_cache (
            wallet_address TEXT PRIMARY KEY,
            username TEXT,
            display_name TEXT,
            encryption_pubkey BLOB NOT NULL,
            scan_pubkey BLOB NOT NULL,
            created_at INTEGER NOT NULL,
            cached_at INTEGER DEFAULT (strftime('%s', 'now')),
            pq_public_key BLOB,
            pq_shared_secret BLOB,
            pq_pubkey_hash BLOB
        );

        CREATE INDEX idx_contacts_username ON contacts_cache(username);

        -- ================================================================
        -- MIGRATION STATE TABLE (kept for schema parity; mobile uses this
        -- to track its legacy-app reconcile step, which has no desktop
        -- equivalent — expected to stay empty here)
        -- ================================================================
        CREATE TABLE migration_state (
            key TEXT PRIMARY KEY,
            started_at INTEGER,
            completed_at INTEGER,
            rows_touched INTEGER,
            notes TEXT
        );
        "#,
    )?;
    Ok(())
}

/// Two-layer contacts model (SPEC parity with `contact_repository.dart`'s
/// `is_contact`/`is_blocked` columns on its `contacts` table): distinguishes
/// a peer the user deliberately added from a row the key-cache auto-created
/// while decrypting a message, and adds manual block/unblock. Both default
/// to 0 so every row `create_schema_v1` could already contain (via
/// `save_contact`) keeps behaving as "not a contact, not blocked".
fn create_schema_v2(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE contacts_cache ADD COLUMN is_contact INTEGER NOT NULL DEFAULT 0;
        ALTER TABLE contacts_cache ADD COLUMN is_blocked INTEGER NOT NULL DEFAULT 0;
        "#,
    )?;
    Ok(())
}

/// Alias-chat (Phase 7f): drops the dead pre-design tables that earlier
/// versions of `create_schema_v1` created (`alias_chats`/`alias_messages`
/// targeted a channel_id-keyed design that was dropped from `sealed_app`
/// itself back in its own migration v15; `ephemeral_wallets` and the old
/// "Spec H" `contacts`/`contact_messages` shape were never read or written
/// by any Rust code — confirmed dead) and replaces them with tables shaped
/// for the actual live Dart design (`alias_onboarding_service.dart`'s local
/// QR/paste handshake, reusing the normal `sendMessage` chain call).
///
/// Deliberately separate from `contacts_cache` (Phase 4, wallet contacts) —
/// not unified into one table the way current Dart does — to avoid touching
/// that already-stable, tested table/module. Renamed `contacts`/
/// `contact_messages` to `alias_contacts`/`alias_messages` specifically so
/// the name can never be confused with `contacts_cache` or with Dart's
/// differently-shaped unified `contacts` table.
fn create_schema_v3(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        r#"
        DROP TABLE IF EXISTS alias_chats;
        DROP TABLE IF EXISTS alias_messages;
        DROP TABLE IF EXISTS ephemeral_wallets;
        DROP TABLE IF EXISTS contact_messages;
        DROP TABLE IF EXISTS contacts;

        -- ================================================================
        -- ALIAS PENDING INVITES (creator-side only — an acceptor promotes
        -- straight to alias_contacts in one step, it never has a pending
        -- row of its own)
        -- ================================================================
        CREATE TABLE alias_pending_invites (
            invite_ref           TEXT PRIMARY KEY,
            label                 TEXT,
            my_x25519_enc_sk      BLOB NOT NULL,
            my_x25519_enc_pub     BLOB NOT NULL,
            my_x25519_scan_sk     BLOB NOT NULL,
            my_x25519_scan_pub    BLOB NOT NULL,
            my_mlkem_sk           BLOB NOT NULL,
            my_mlkem_pub          BLOB NOT NULL,
            created_at            INTEGER NOT NULL,
            dismissed             INTEGER NOT NULL DEFAULT 0
        );

        -- ================================================================
        -- ALIAS CONTACTS (established, either role). `my_mlkem_sk` is only
        -- ever set on creator-side rows; `peer_mlkem_pub` only on acceptor-
        -- side rows — the PQ handshake is intentionally one-directional.
        -- ================================================================
        CREATE TABLE alias_contacts (
            contact_id             TEXT PRIMARY KEY,
            label                  TEXT,
            is_creator             INTEGER NOT NULL,
            my_x25519_enc_sk       BLOB NOT NULL,
            my_x25519_enc_pub      BLOB NOT NULL,
            my_x25519_scan_sk      BLOB NOT NULL,
            my_x25519_scan_pub     BLOB NOT NULL,
            my_mlkem_sk            BLOB,
            peer_x25519_enc_pub    BLOB NOT NULL,
            peer_x25519_scan_pub   BLOB NOT NULL,
            peer_mlkem_pub         BLOB,
            pq_shared_secret       BLOB NOT NULL,
            created_at             INTEGER NOT NULL,
            established_at         INTEGER NOT NULL
        );

        -- ================================================================
        -- ALIAS MESSAGES. No self-copy ciphertext exists on-chain for alias
        -- sends (unlike normal DMs) — only received history survives a
        -- force_resync/reinstall; sent history is local-only, matching the
        -- live Dart behavior exactly.
        -- ================================================================
        CREATE TABLE alias_messages (
            id             TEXT PRIMARY KEY,
            contact_id     TEXT NOT NULL REFERENCES alias_contacts(contact_id),
            content        TEXT NOT NULL,
            timestamp      INTEGER NOT NULL,
            is_outgoing    INTEGER NOT NULL,
            is_read        INTEGER NOT NULL DEFAULT 1,
            on_chain_ref   TEXT,
            created_at     INTEGER DEFAULT (strftime('%s', 'now'))
        );
        CREATE INDEX idx_alias_messages_contact ON alias_messages(contact_id, timestamp DESC);
        "#,
    )?;
    Ok(())
}

/// Alias-chat (Phase 7h): supports delivering an invite/accept envelope as
/// a regular DM to an already-known wallet contact, instead of only via
/// QR/paste. `peer_wallet` on both existing alias tables is **local-only UI
/// convenience metadata** — it is never transmitted on-chain and never
/// embedded in `InviteEnvelope`/`AcceptEnvelope` (those wire structs in
/// `alias/envelope.rs` are untouched), so it does not weaken the alias
/// identity's on-chain unlinkability from the regular wallet; only someone
/// who already has read access to this device's local encrypted DB could
/// ever observe the mapping. `alias_incoming_invites` is the new
/// receiver-side "awaiting my decision" state that didn't exist before —
/// the old QR/paste `accept_invite` command completes synchronously in one
/// user action, but an invite arriving passively via background sync needs
/// somewhere to sit until the user explicitly Accepts/Declines.
fn create_schema_v4(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE alias_pending_invites ADD COLUMN peer_wallet TEXT;
        ALTER TABLE alias_contacts        ADD COLUMN peer_wallet TEXT;

        CREATE TABLE alias_incoming_invites (
            invite_ref     TEXT PRIMARY KEY,
            peer_wallet    TEXT NOT NULL,
            peer_username  TEXT,
            envelope_bytes BLOB NOT NULL,
            received_at    INTEGER NOT NULL,
            status         TEXT NOT NULL DEFAULT 'pending'
        );
        CREATE INDEX idx_alias_incoming_invites_status ON alias_incoming_invites(status, received_at DESC);
        "#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-db-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("{name}.db"))
    }

    #[test]
    fn create_then_reopen_round_trip() {
        let path = temp_path("round-trip");
        let _ = std::fs::remove_file(&path);
        let dek = [1u8; 32];

        {
            let db = Db::open(&path, &dek).unwrap();
            db.connection()
                .execute(
                    "INSERT INTO messages (id, sender_wallet, recipient_wallet, content, timestamp, is_outgoing) \
                     VALUES ('m1', 'alice', 'bob', 'hello', 1000, 1)",
                    [],
                )
                .unwrap();
            db.save(&dek).unwrap();
        }

        let reopened = Db::open(&path, &dek).unwrap();
        let content: String = reopened
            .connection()
            .query_row("SELECT content FROM messages WHERE id = 'm1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(content, "hello");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wrong_dek_fails_to_decrypt() {
        let path = temp_path("wrong-dek");
        let _ = std::fs::remove_file(&path);
        let dek = [2u8; 32];
        {
            let db = Db::open(&path, &dek).unwrap();
            db.save(&dek).unwrap();
        }
        let other_dek = [3u8; 32];
        assert!(Db::open(&path, &other_dek).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fresh_db_has_expected_tables() {
        let path = temp_path("fresh-schema");
        let _ = std::fs::remove_file(&path);
        let dek = [4u8; 32];
        let db = Db::open(&path, &dek).unwrap();

        let mut stmt = db
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();

        for expected in [
            "messages",
            "sync_state",
            "indexer_state",
            "user_profile",
            "contacts_cache",
            "migration_state",
            "alias_pending_invites",
            "alias_contacts",
            "alias_messages",
            "alias_incoming_invites",
        ] {
            assert!(tables.iter().any(|t| t == expected), "missing table {expected}");
        }
        for gone in ["alias_chats", "ephemeral_wallets", "contacts", "contact_messages"] {
            assert!(!tables.iter().any(|t| t == gone), "dead table {gone} should not exist in a fresh schema");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sync_state_seeded_with_global_row() {
        let path = temp_path("sync-state-seed");
        let _ = std::fs::remove_file(&path);
        let dek = [5u8; 32];
        let db = Db::open(&path, &dek).unwrap();
        let count: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM sync_state WHERE key = 'global'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fresh_db_gets_v2_columns_with_default_zero() {
        let path = temp_path("fresh-v2-columns");
        let _ = std::fs::remove_file(&path);
        let db = Db::open(&path, &[6u8; 32]).unwrap();
        db.connection()
            .execute(
                "INSERT INTO contacts_cache (wallet_address, encryption_pubkey, scan_pubkey, created_at) \
                 VALUES ('W1', X'00', X'00', 1000)",
                [],
            )
            .unwrap();
        let (is_contact, is_blocked): (i64, i64) = db
            .connection()
            .query_row("SELECT is_contact, is_blocked FROM contacts_cache WHERE wallet_address = 'W1'", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!((is_contact, is_blocked), (0, 0));
        let _ = std::fs::remove_file(&path);
    }

    /// Regression guard for the actual upgrade path: an existing on-disk
    /// database created before `is_contact`/`is_blocked` existed (real
    /// scenario for anyone who already has a `sealed_messages.db`, not just
    /// a hypothetical) must gain the new columns via `ALTER TABLE`, without
    /// losing the row already in it, when reopened.
    #[test]
    fn existing_v1_database_migrates_to_v2_in_place() {
        let path = temp_path("migrate-v1-to-v2");
        let _ = std::fs::remove_file(&path);
        let dek = [8u8; 32];

        {
            // Build a v1-shaped database directly (bypassing `apply_migrations`,
            // which would already include v2) to simulate a real pre-existing
            // install, then stamp it at schema version 1.
            let conn = Connection::open_in_memory().unwrap();
            create_schema_v1(&conn).unwrap();
            conn.execute(
                "INSERT INTO contacts_cache (wallet_address, username, encryption_pubkey, scan_pubkey, created_at) \
                 VALUES ('W1', 'alice', X'01', X'02', 1000)",
                [],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1i64).unwrap();
            let data = conn.serialize(MAIN_DB).unwrap();
            let encrypted = aead::encrypt_combined(&dek, &data).unwrap();
            std::fs::write(&path, encrypted).unwrap();
        }

        // Reopening runs `apply_migrations` against a version-1 database —
        // must add the columns without disturbing the existing row.
        let db = Db::open(&path, &dek).unwrap();
        let (username, is_contact, is_blocked): (String, i64, i64) = db
            .connection()
            .query_row("SELECT username, is_contact, is_blocked FROM contacts_cache WHERE wallet_address = 'W1'", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(username, "alice");
        assert_eq!((is_contact, is_blocked), (0, 0));
        let _ = std::fs::remove_file(&path);
    }

    /// Regression guard for the Phase 7f schema rework: an existing on-disk
    /// database created before `create_schema_v1` dropped the dead
    /// `alias_chats`/`alias_messages`/`ephemeral_wallets`/`contacts`/
    /// `contact_messages` tables must have them replaced by the new
    /// `alias_pending_invites`/`alias_contacts`/`alias_messages` shape on
    /// reopen, without erroring even though the old tables held junk rows.
    #[test]
    fn existing_v2_database_migrates_to_v3_in_place() {
        let path = temp_path("migrate-v2-to-v3");
        let _ = std::fs::remove_file(&path);
        let dek = [9u8; 32];

        {
            // Build a v2-shaped database with the OLD (now-removed from
            // create_schema_v1) dead tables present, simulating a real
            // pre-Phase-7f install.
            let conn = Connection::open_in_memory().unwrap();
            create_schema_v1(&conn).unwrap();
            create_schema_v2(&conn).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE alias_chats (channel_id TEXT PRIMARY KEY, alias TEXT NOT NULL);
                CREATE TABLE ephemeral_wallets (channel_id TEXT PRIMARY KEY, address TEXT NOT NULL);
                CREATE TABLE contacts (contact_id TEXT PRIMARY KEY, alias_label TEXT NOT NULL);
                CREATE TABLE contact_messages (id TEXT PRIMARY KEY, contact_id TEXT NOT NULL);
                CREATE TABLE alias_messages (id TEXT PRIMARY KEY, channel_id TEXT NOT NULL);
                "#,
            )
            .unwrap();
            conn.execute("INSERT INTO alias_chats (channel_id, alias) VALUES ('c1', 'old-junk')", [])
                .unwrap();
            conn.pragma_update(None, "user_version", 2i64).unwrap();
            let data = conn.serialize(MAIN_DB).unwrap();
            let encrypted = aead::encrypt_combined(&dek, &data).unwrap();
            std::fs::write(&path, encrypted).unwrap();
        }

        let db = Db::open(&path, &dek).unwrap();
        let mut stmt = db
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt.query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();

        for gone in ["alias_chats", "ephemeral_wallets", "contacts", "contact_messages"] {
            assert!(!tables.iter().any(|t| t == gone), "dead table {gone} should have been dropped");
        }
        for expected in ["alias_pending_invites", "alias_contacts", "alias_messages"] {
            assert!(tables.iter().any(|t| t == expected), "missing new table {expected}");
        }
        let _ = std::fs::remove_file(&path);
    }

    /// Regression guard for Phase 7h: an existing on-disk database created
    /// before `peer_wallet`/`alias_incoming_invites` existed must gain them
    /// on reopen, without losing an already-established alias contact row.
    #[test]
    fn existing_v3_database_migrates_to_v4_in_place() {
        let path = temp_path("migrate-v3-to-v4");
        let _ = std::fs::remove_file(&path);
        let dek = [10u8; 32];

        {
            let conn = Connection::open_in_memory().unwrap();
            create_schema_v1(&conn).unwrap();
            create_schema_v2(&conn).unwrap();
            create_schema_v3(&conn).unwrap();
            conn.execute(
                "INSERT INTO alias_contacts \
                 (contact_id, is_creator, my_x25519_enc_sk, my_x25519_enc_pub, my_x25519_scan_sk, my_x25519_scan_pub, \
                  peer_x25519_enc_pub, peer_x25519_scan_pub, pq_shared_secret, created_at, established_at) \
                 VALUES ('c1', 1, X'00', X'00', X'00', X'00', X'00', X'00', X'00', 1000, 1000)",
                [],
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 3i64).unwrap();
            let data = conn.serialize(MAIN_DB).unwrap();
            let encrypted = aead::encrypt_combined(&dek, &data).unwrap();
            std::fs::write(&path, encrypted).unwrap();
        }

        let db = Db::open(&path, &dek).unwrap();
        let mut stmt = db
            .connection()
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt.query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        assert!(tables.iter().any(|t| t == "alias_incoming_invites"), "missing new table alias_incoming_invites");

        let peer_wallet: Option<String> =
            db.connection().query_row("SELECT peer_wallet FROM alias_contacts WHERE contact_id = 'c1'", [], |row| row.get(0)).unwrap();
        assert_eq!(peer_wallet, None);
        let _ = std::fs::remove_file(&path);
    }
}
