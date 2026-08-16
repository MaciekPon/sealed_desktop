//! Message cache repository, ports `local/repositories/message_repository.dart`.
//!
//! `purgeSolanaConversations` is not ported: it's a one-shot migration
//! cleanup for pre-Algorand-cutover data, and desktop is a fresh install
//! with no such history (same reasoning as `db/mod.rs` applying the target
//! schema directly instead of mobile's v1->v14 ladder). Dart's `messageExists`
//! (keyed by `id`) has no call site here — every message's `id` and
//! `on_chain_pubkey` are the same value (the tx signature), so `has_message`
//! (keyed by `on_chain_pubkey`) alone covers both.
//!
//! Timestamps are stored as Unix seconds (`INTEGER`), not mobile's
//! ISO-8601 text column — a deliberate desktop-side normalization already
//! established in `db/mod.rs`'s fresh-schema comment; there's no legacy
//! text-timestamp data to stay compatible with.

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct DecryptedMessage {
    pub id: String,
    pub sender_wallet: String,
    pub sender_username: Option<String>,
    pub recipient_wallet: String,
    pub recipient_username: Option<String>,
    pub content: String,
    /// Unix seconds.
    pub timestamp: i64,
    pub is_outgoing: bool,
    pub on_chain_pubkey: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConversationPreview {
    pub contact_wallet: String,
    pub contact_username: Option<String>,
    pub last_message_preview: String,
    pub last_message_timestamp: i64,
    pub is_last_message_outgoing: bool,
    pub unread_count: i64,
    pub message_count: i64,
    /// From `contacts_cache.is_blocked` (`false` if no cached row exists —
    /// mirrors `getConversations`'s `COALESCE(cc.is_blocked, 0)`). This is
    /// the **only** Spam-tab predicate — everything else lands in Chats,
    /// matching `message_repository.dart`'s comment on the same query.
    pub is_blocked: bool,
}

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<DecryptedMessage> {
    let is_outgoing: i64 = row.get("is_outgoing")?;
    Ok(DecryptedMessage {
        id: row.get("id")?,
        sender_wallet: row.get("sender_wallet")?,
        sender_username: row.get("sender_username")?,
        recipient_wallet: row.get("recipient_wallet")?,
        recipient_username: row.get("recipient_username")?,
        content: row.get("content")?,
        timestamp: row.get("timestamp")?,
        is_outgoing: is_outgoing != 0,
        on_chain_pubkey: row.get("on_chain_pubkey")?,
    })
}

/// Insert or fully replace a message. `is_read` follows the Dart source's
/// rule exactly: outgoing messages start read (they're our own), incoming
/// start unread.
pub fn save_message(conn: &Connection, message: &DecryptedMessage) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO messages \
         (id, sender_wallet, sender_username, recipient_wallet, recipient_username, content, timestamp, is_outgoing, is_read, on_chain_pubkey) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)",
        params![
            message.id,
            message.sender_wallet,
            message.sender_username,
            message.recipient_wallet,
            message.recipient_username,
            message.content,
            message.timestamp,
            message.is_outgoing as i64,
            message.on_chain_pubkey,
        ],
    )?;
    Ok(())
}

pub fn has_message(conn: &Connection, tx_signature: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1 FROM messages WHERE on_chain_pubkey = ?1 LIMIT 1",
        params![tx_signature],
        |_| Ok(()),
    )
    .optional()
    .map(|r| r.is_some())
}

pub fn get_conversation_messages(conn: &Connection, wallet_a: &str, wallet_b: &str) -> rusqlite::Result<Vec<DecryptedMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, sender_wallet, sender_username, recipient_wallet, recipient_username, content, timestamp, is_outgoing, on_chain_pubkey \
         FROM messages \
         WHERE (sender_wallet = ?1 AND recipient_wallet = ?2) OR (sender_wallet = ?2 AND recipient_wallet = ?1) \
         ORDER BY timestamp DESC",
    )?;
    let rows = stmt.query_map(params![wallet_a, wallet_b], row_to_message)?;
    rows.collect()
}

pub fn mark_as_read(conn: &Connection, contact_wallet: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE messages SET is_read = 1 WHERE sender_wallet = ?1 AND is_read = 0", params![contact_wallet])?;
    Ok(())
}

pub fn mark_all_as_read(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("UPDATE messages SET is_read = 1 WHERE is_read = 0", [])?;
    Ok(())
}

pub fn get_unread_count(conn: &Connection, contact_wallet: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE sender_wallet = ?1 AND is_outgoing = 0 AND is_read = 0",
        params![contact_wallet],
        |row| row.get(0),
    )
}

pub fn delete_conversation(conn: &Connection, contact_wallet: &str) -> rusqlite::Result<i64> {
    let count = conn.execute("DELETE FROM messages WHERE sender_wallet = ?1 OR recipient_wallet = ?1", params![contact_wallet])?;
    Ok(count as i64)
}

pub fn clear(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM messages", [])?;
    Ok(())
}

/// One row per conversation (grouped by the unordered wallet pair), newest
/// first. Ports the raw-SQL query in `MessageRepositoryImpl.getConversations`
/// verbatim (aside from `timestamp` being an integer column here rather
/// than ISO-8601 text — `MAX()`/`ORDER BY` work the same either way).
pub fn get_conversations(conn: &Connection) -> rusqlite::Result<Vec<ConversationPreview>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
          CASE WHEN latest_message.is_outgoing = 1 THEN latest_message.recipient_wallet ELSE latest_message.sender_wallet END AS contact_wallet,
          -- `contacts_cache` (the manually-curated address book) is the
          -- source of truth for a known contact's display name when
          -- present; the per-message snapshot is only a fallback for
          -- unknown peers with no cached row. Mirrors `getConversations`'s
          -- COALESCE(NULLIF(cc.username, ''), ...) in message_repository.dart
          -- — this makes the Chats list and the Contacts screen agree.
          COALESCE(
            NULLIF(cc.username, ''),
            CASE WHEN latest_message.is_outgoing = 1 THEN latest_message.recipient_username ELSE latest_message.sender_username END
          ) AS contact_username,
          latest_message.content AS last_message_preview,
          latest_message.timestamp AS last_message_timestamp,
          latest_message.is_outgoing AS is_last_message_outgoing,
          (
            SELECT COUNT(*) FROM messages m2
            WHERE m2.is_read = 0 AND m2.is_outgoing = 0
              AND m2.sender_wallet = CASE WHEN latest_message.is_outgoing = 1 THEN latest_message.recipient_wallet ELSE latest_message.sender_wallet END
          ) AS unread_count,
          (
            SELECT COUNT(*) FROM messages m3
            WHERE CASE WHEN m3.sender_wallet < m3.recipient_wallet THEN m3.sender_wallet || ':' || m3.recipient_wallet ELSE m3.recipient_wallet || ':' || m3.sender_wallet END
                = CASE WHEN latest_message.sender_wallet < latest_message.recipient_wallet THEN latest_message.sender_wallet || ':' || latest_message.recipient_wallet ELSE latest_message.recipient_wallet || ':' || latest_message.sender_wallet END
          ) AS message_count,
          COALESCE(cc.is_blocked, 0) AS is_blocked
        FROM messages AS latest_message
        INNER JOIN (
          SELECT
            CASE WHEN sender_wallet < recipient_wallet THEN sender_wallet || ':' || recipient_wallet ELSE recipient_wallet || ':' || sender_wallet END AS conversation_key,
            MAX(timestamp) AS max_timestamp
          FROM messages
          GROUP BY CASE WHEN sender_wallet < recipient_wallet THEN sender_wallet || ':' || recipient_wallet ELSE recipient_wallet || ':' || sender_wallet END
        ) AS grouped_latest
          ON grouped_latest.conversation_key = CASE WHEN latest_message.sender_wallet < latest_message.recipient_wallet THEN latest_message.sender_wallet || ':' || latest_message.recipient_wallet ELSE latest_message.recipient_wallet || ':' || latest_message.sender_wallet END
          AND latest_message.timestamp = grouped_latest.max_timestamp
        LEFT JOIN contacts_cache cc
          ON cc.wallet_address = CASE WHEN latest_message.is_outgoing = 1 THEN latest_message.recipient_wallet ELSE latest_message.sender_wallet END
        ORDER BY latest_message.timestamp DESC
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let is_last_message_outgoing: i64 = row.get("is_last_message_outgoing")?;
        let is_blocked: i64 = row.get("is_blocked")?;
        Ok(ConversationPreview {
            contact_wallet: row.get("contact_wallet")?,
            contact_username: row.get("contact_username")?,
            last_message_preview: row.get("last_message_preview")?,
            last_message_timestamp: row.get("last_message_timestamp")?,
            is_last_message_outgoing: is_last_message_outgoing != 0,
            unread_count: row.get("unread_count")?,
            message_count: row.get("message_count")?,
            is_blocked: is_blocked != 0,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-messages-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&dir.join("test.db"), &[7u8; 32]).unwrap()
    }

    fn sample(id: &str, sender: &str, recipient: &str, ts: i64, outgoing: bool) -> DecryptedMessage {
        DecryptedMessage {
            id: id.to_string(),
            sender_wallet: sender.to_string(),
            sender_username: Some("alice".to_string()),
            recipient_wallet: recipient.to_string(),
            recipient_username: Some("bob".to_string()),
            content: format!("hello from {sender}"),
            timestamp: ts,
            is_outgoing: outgoing,
            on_chain_pubkey: id.to_string(),
        }
    }

    #[test]
    fn save_then_get_round_trip_and_read_flag_matches_outgoing() {
        let db = temp_db("save-get");
        let conn = db.connection();
        save_message(conn, &sample("tx1", "A", "B", 100, true)).unwrap();
        save_message(conn, &sample("tx2", "B", "A", 200, false)).unwrap();

        assert!(has_message(conn, "tx1").unwrap());
        assert!(has_message(conn, "tx2").unwrap());
        assert!(!has_message(conn, "nope").unwrap());

        assert_eq!(get_unread_count(conn, "B").unwrap(), 1); // tx2 is incoming, unread
        mark_as_read(conn, "B").unwrap();
        assert_eq!(get_unread_count(conn, "B").unwrap(), 0);
    }

    #[test]
    fn get_conversation_messages_returns_both_directions_newest_first() {
        let db = temp_db("conversation");
        let conn = db.connection();
        save_message(conn, &sample("tx1", "A", "B", 100, true)).unwrap();
        save_message(conn, &sample("tx2", "B", "A", 200, false)).unwrap();
        save_message(conn, &sample("tx3", "A", "C", 150, true)).unwrap(); // unrelated conversation

        let convo = get_conversation_messages(conn, "A", "B").unwrap();
        assert_eq!(convo.len(), 2);
        assert_eq!(convo[0].id, "tx2"); // newest first
        assert_eq!(convo[1].id, "tx1");
    }

    #[test]
    fn mark_all_as_read_clears_every_unread_message() {
        let db = temp_db("mark-all");
        let conn = db.connection();
        save_message(conn, &sample("tx1", "A", "ME", 100, false)).unwrap();
        save_message(conn, &sample("tx2", "B", "ME", 200, false)).unwrap();
        mark_all_as_read(conn).unwrap();
        assert_eq!(get_unread_count(conn, "A").unwrap(), 0);
        assert_eq!(get_unread_count(conn, "B").unwrap(), 0);
    }

    #[test]
    fn get_conversations_groups_by_unordered_wallet_pair() {
        let db = temp_db("conversations");
        let conn = db.connection();
        save_message(conn, &sample("tx1", "A", "B", 100, true)).unwrap();
        save_message(conn, &sample("tx2", "B", "A", 200, false)).unwrap(); // same conversation, newer
        save_message(conn, &sample("tx3", "A", "C", 150, true)).unwrap();

        let previews = get_conversations(conn).unwrap();
        assert_eq!(previews.len(), 2); // A<->B and A<->C, not 3 separate rows

        let ab = previews.iter().find(|p| p.contact_wallet == "B" || p.last_message_preview.contains('B')).unwrap();
        assert_eq!(ab.message_count, 2);
        assert_eq!(ab.last_message_timestamp, 200);
    }

    #[test]
    fn delete_conversation_and_clear() {
        let db = temp_db("delete-clear");
        let conn = db.connection();
        save_message(conn, &sample("tx1", "A", "B", 100, true)).unwrap();
        save_message(conn, &sample("tx2", "A", "C", 150, true)).unwrap();

        let deleted = delete_conversation(conn, "B").unwrap();
        assert_eq!(deleted, 1);
        assert!(!has_message(conn, "tx1").unwrap());
        assert!(has_message(conn, "tx2").unwrap());

        clear(conn).unwrap();
        assert!(get_conversations(conn).unwrap().is_empty());
    }
}
