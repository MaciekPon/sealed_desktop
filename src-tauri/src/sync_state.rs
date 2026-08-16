//! Last-sync-time tracking, ports `local/sync_state.dart`. Storage unit is
//! Unix **milliseconds** (matching `DateTime.millisecondsSinceEpoch`),
//! even though the messaging sync flow converts it to seconds before
//! passing it to the chain client — that conversion happens in
//! `messaging.rs`, mirroring `MessageService._syncMessages` exactly.

use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncStateError {
    #[error("sync_state row not found")]
    NotFound,
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub type SyncStateResult<T> = Result<T, SyncStateError>;

/// Unix milliseconds of the last successful sync, or 0 if never synced.
pub fn last_sync_time_millis(conn: &Connection) -> SyncStateResult<i64> {
    let value: Option<i64> =
        conn.query_row("SELECT last_sync_timestamp FROM sync_state WHERE key = 'global'", [], |row| row.get(0))?;
    Ok(value.unwrap_or(0))
}

pub fn update_last_sync_time_millis(conn: &Connection, millis: i64) -> SyncStateResult<()> {
    let rows = conn.execute("UPDATE sync_state SET last_sync_timestamp = ?1 WHERE key = 'global'", params![millis])?;
    if rows == 0 {
        return Err(SyncStateError::NotFound);
    }
    Ok(())
}

pub fn reset(conn: &Connection) -> SyncStateResult<()> {
    let rows = conn.execute("UPDATE sync_state SET last_sync_timestamp = 0, last_processed_slot = 0 WHERE key = 'global'", [])?;
    if rows == 0 {
        return Err(SyncStateError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-syncstate-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&dir.join("test.db"), &[7u8; 32]).unwrap()
    }

    #[test]
    fn seeded_row_starts_at_zero() {
        let db = temp_db("seeded");
        assert_eq!(last_sync_time_millis(db.connection()).unwrap(), 0);
    }

    #[test]
    fn update_then_read_round_trip() {
        let db = temp_db("update");
        let conn = db.connection();
        update_last_sync_time_millis(conn, 123_456).unwrap();
        assert_eq!(last_sync_time_millis(conn).unwrap(), 123_456);
    }

    #[test]
    fn reset_zeroes_out() {
        let db = temp_db("reset");
        let conn = db.connection();
        update_last_sync_time_millis(conn, 999).unwrap();
        reset(conn).unwrap();
        assert_eq!(last_sync_time_millis(conn).unwrap(), 0);
    }
}
