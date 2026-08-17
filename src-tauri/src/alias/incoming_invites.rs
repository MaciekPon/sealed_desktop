//! CRUD for `alias_incoming_invites` — the receiver-side "awaiting my
//! decision" state for an alias invite that arrived as a regular DM (Phase
//! 7h), as opposed to the QR/paste flow's `accept_invite` command, which
//! completes synchronously in one user action and never needs this table.
//!
//! Rows are never hard-deleted: `record_incoming_invite` is an idempotent
//! `INSERT OR IGNORE`, and Accept/Decline only flip `status`. This matters
//! because `force_resync` always re-scans from genesis — if a decided
//! invite's row were deleted, the same on-chain transaction would be
//! re-decrypted and re-inserted as a fresh "pending" row on the next full
//! resync, resurrecting an already-decided invite.

use rusqlite::{params, Connection, OptionalExtension};

pub struct IncomingInvite {
    pub invite_ref: String,
    pub peer_wallet: String,
    pub peer_username: Option<String>,
    pub envelope_bytes: Vec<u8>,
    pub received_at: i64,
    // Filtering already happened in SQL (`list_incoming_invites`'s `WHERE
    // status = ?1`), so by the time a row reaches Rust its status is
    // already known from context — no caller re-reads this field. Kept on
    // the struct because it's a real column and useful when debugging.
    #[allow(dead_code)]
    pub status: String,
}

fn row_to_incoming_invite(row: &rusqlite::Row) -> rusqlite::Result<IncomingInvite> {
    Ok(IncomingInvite {
        invite_ref: row.get("invite_ref")?,
        peer_wallet: row.get("peer_wallet")?,
        peer_username: row.get("peer_username")?,
        envelope_bytes: row.get("envelope_bytes")?,
        received_at: row.get("received_at")?,
        status: row.get("status")?,
    })
}

const INCOMING_COLUMNS: &str = "invite_ref, peer_wallet, peer_username, envelope_bytes, received_at, status";

/// Idempotent: a re-delivered/re-synced copy of the same on-chain invite
/// transaction (same `invite_ref_hex`) is silently ignored, whatever its
/// current `status` is — see this module's doc comment. Returns `true` iff
/// a new row was actually inserted (as opposed to an `INSERT OR IGNORE`
/// no-op) — callers use this to decide whether anything actually changed.
pub fn record_incoming_invite(
    conn: &Connection,
    invite_ref_hex: &str,
    peer_wallet: &str,
    peer_username: Option<&str>,
    envelope_bytes: &[u8],
    received_at: i64,
) -> rusqlite::Result<bool> {
    let rows = conn.execute(
        "INSERT OR IGNORE INTO alias_incoming_invites (invite_ref, peer_wallet, peer_username, envelope_bytes, received_at, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'pending')",
        params![invite_ref_hex, peer_wallet, peer_username, envelope_bytes, received_at],
    )?;
    Ok(rows > 0)
}

pub fn get_incoming_invite(conn: &Connection, invite_ref_hex: &str) -> rusqlite::Result<Option<IncomingInvite>> {
    conn.query_row(&format!("SELECT {INCOMING_COLUMNS} FROM alias_incoming_invites WHERE invite_ref = ?1"), params![invite_ref_hex], row_to_incoming_invite).optional()
}

pub fn list_incoming_invites(conn: &Connection, status: &str) -> rusqlite::Result<Vec<IncomingInvite>> {
    let mut stmt = conn.prepare(&format!("SELECT {INCOMING_COLUMNS} FROM alias_incoming_invites WHERE status = ?1 ORDER BY received_at DESC"))?;
    let rows = stmt.query_map(params![status], row_to_incoming_invite)?;
    rows.collect()
}

pub fn set_incoming_invite_status(conn: &Connection, invite_ref_hex: &str, status: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE alias_incoming_invites SET status = ?1 WHERE invite_ref = ?2", params![status, invite_ref_hex])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-alias-incoming-invites-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&dir.join("test.db"), &[7u8; 32]).unwrap()
    }

    #[test]
    fn record_get_list_and_set_status_round_trip() {
        let db = temp_db("round-trip");
        let conn = db.connection();

        record_incoming_invite(conn, "ref1", "WALLETA", Some("alice"), b"envelope-bytes", 1000).unwrap();
        let fetched = get_incoming_invite(conn, "ref1").unwrap().unwrap();
        assert_eq!(fetched.peer_wallet, "WALLETA");
        assert_eq!(fetched.peer_username.as_deref(), Some("alice"));
        assert_eq!(fetched.envelope_bytes, b"envelope-bytes");
        assert_eq!(fetched.status, "pending");

        assert_eq!(list_incoming_invites(conn, "pending").unwrap().len(), 1);
        assert_eq!(list_incoming_invites(conn, "accepted").unwrap().len(), 0);

        set_incoming_invite_status(conn, "ref1", "accepted").unwrap();
        assert_eq!(get_incoming_invite(conn, "ref1").unwrap().unwrap().status, "accepted");
        assert_eq!(list_incoming_invites(conn, "pending").unwrap().len(), 0);
        assert_eq!(list_incoming_invites(conn, "accepted").unwrap().len(), 1);
    }

    /// Regression guard for the resurrection bug this schema is designed to
    /// avoid: recording the same `invite_ref` twice (e.g. a re-synced
    /// on-chain transaction after the first copy was already decided) must
    /// not reset its status back to "pending".
    #[test]
    fn record_incoming_invite_is_idempotent_and_does_not_resurrect_decided_invites() {
        let db = temp_db("idempotent");
        let conn = db.connection();

        assert!(record_incoming_invite(conn, "ref1", "WALLETA", None, b"envelope-bytes", 1000).unwrap());
        set_incoming_invite_status(conn, "ref1", "declined").unwrap();

        // Simulate a force_resync re-delivering the same transaction — must
        // report `false` (nothing new), the caller relies on this to avoid
        // reporting a resurrection as a "new" event too.
        assert!(!record_incoming_invite(conn, "ref1", "WALLETA", None, b"envelope-bytes", 1000).unwrap());

        assert_eq!(get_incoming_invite(conn, "ref1").unwrap().unwrap().status, "declined");
        assert_eq!(list_incoming_invites(conn, "pending").unwrap().len(), 0);
    }
}
