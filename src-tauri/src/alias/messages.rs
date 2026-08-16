//! CRUD for `alias_messages`, mirroring the style of the top-level
//! `messages.rs` (which owns the separate `messages` table for wallet DMs).
//! Simpler than its wallet-pair counterpart: `contact_id` is already the
//! grouping key, no self-join over sender/recipient pairs needed.

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct AliasMessage {
    pub id: String,
    pub contact_id: String,
    pub content: String,
    pub timestamp: i64,
    pub is_outgoing: bool,
}

fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<AliasMessage> {
    let is_outgoing: i64 = row.get("is_outgoing")?;
    Ok(AliasMessage {
        id: row.get("id")?,
        contact_id: row.get("contact_id")?,
        content: row.get("content")?,
        timestamp: row.get("timestamp")?,
        is_outgoing: is_outgoing != 0,
    })
}

/// Outgoing messages start read (they're our own); incoming start unread —
/// same rule as the wallet-DM `messages::save_message`.
pub fn save_alias_message(conn: &Connection, msg: &AliasMessage) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO alias_messages (id, contact_id, content, timestamp, is_outgoing, is_read) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![msg.id, msg.contact_id, msg.content, msg.timestamp, msg.is_outgoing as i64],
    )?;
    Ok(())
}

pub fn has_alias_message(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT 1 FROM alias_messages WHERE id = ?1 LIMIT 1", params![id], |_| Ok(())).optional().map(|r| r.is_some())
}

pub fn get_alias_conversation(conn: &Connection, contact_id: &str) -> rusqlite::Result<Vec<AliasMessage>> {
    let mut stmt = conn.prepare(
        "SELECT id, contact_id, content, timestamp, is_outgoing FROM alias_messages \
         WHERE contact_id = ?1 ORDER BY timestamp DESC",
    )?;
    let rows = stmt.query_map(params![contact_id], row_to_message)?;
    rows.collect()
}

pub fn mark_alias_conversation_read(conn: &Connection, contact_id: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE alias_messages SET is_read = 1 WHERE contact_id = ?1 AND is_read = 0", params![contact_id])?;
    Ok(())
}

pub fn get_alias_unread_count(conn: &Connection, contact_id: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM alias_messages WHERE contact_id = ?1 AND is_outgoing = 0 AND is_read = 0",
        params![contact_id],
        |row| row.get(0),
    )
}

pub struct AliasConversationPreview {
    pub contact_id: String,
    pub label: Option<String>,
    pub last_message_preview: String,
    pub last_message_timestamp: i64,
    pub is_last_message_outgoing: bool,
    pub unread_count: i64,
    pub message_count: i64,
}

/// One row per alias contact that has at least one message, newest first.
/// Contacts with zero messages are omitted — the caller's "chat list" merge
/// only needs to show conversations that have actually started.
pub fn get_alias_conversations(conn: &Connection) -> rusqlite::Result<Vec<AliasConversationPreview>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
          c.contact_id AS contact_id,
          c.label AS label,
          latest.content AS last_message_preview,
          latest.timestamp AS last_message_timestamp,
          latest.is_outgoing AS is_last_message_outgoing,
          (SELECT COUNT(*) FROM alias_messages m2 WHERE m2.contact_id = c.contact_id AND m2.is_outgoing = 0 AND m2.is_read = 0) AS unread_count,
          (SELECT COUNT(*) FROM alias_messages m3 WHERE m3.contact_id = c.contact_id) AS message_count
        FROM alias_contacts c
        INNER JOIN alias_messages latest ON latest.contact_id = c.contact_id
        WHERE latest.timestamp = (SELECT MAX(m4.timestamp) FROM alias_messages m4 WHERE m4.contact_id = c.contact_id)
        ORDER BY latest.timestamp DESC
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let is_last_message_outgoing: i64 = row.get("is_last_message_outgoing")?;
        Ok(AliasConversationPreview {
            contact_id: row.get("contact_id")?,
            label: row.get("label")?,
            last_message_preview: row.get("last_message_preview")?,
            last_message_timestamp: row.get("last_message_timestamp")?,
            is_last_message_outgoing: is_last_message_outgoing != 0,
            unread_count: row.get("unread_count")?,
            message_count: row.get("message_count")?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alias::contacts as alias_contacts;
    use crate::alias::onboarding;
    use crate::db::Db;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-alias-messages-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&dir.join("test.db"), &[7u8; 32]).unwrap()
    }

    fn seeded_contact(conn: &Connection, label: &str) -> String {
        let created = onboarding::create_invitation_envelope().unwrap();
        let accepted = onboarding::accept_invitation_from_envelope(&created.envelope_bytes).unwrap();
        alias_contacts::insert_accepted_contact(conn, &accepted, Some(label), None, 1000).unwrap();
        accepted.contact_id
    }

    fn sample(id: &str, contact_id: &str, ts: i64, outgoing: bool) -> AliasMessage {
        AliasMessage { id: id.to_string(), contact_id: contact_id.to_string(), content: format!("hi {id}"), timestamp: ts, is_outgoing: outgoing }
    }

    #[test]
    fn save_then_get_round_trip_and_read_flag_matches_outgoing() {
        let db = temp_db("save-get");
        let conn = db.connection();
        let cid = seeded_contact(conn, "Bob");

        save_alias_message(conn, &sample("m1", &cid, 100, true)).unwrap();
        save_alias_message(conn, &sample("m2", &cid, 200, false)).unwrap();

        assert!(has_alias_message(conn, "m1").unwrap());
        assert!(!has_alias_message(conn, "nope").unwrap());

        assert_eq!(get_alias_unread_count(conn, &cid).unwrap(), 1);
        mark_alias_conversation_read(conn, &cid).unwrap();
        assert_eq!(get_alias_unread_count(conn, &cid).unwrap(), 0);

        let convo = get_alias_conversation(conn, &cid).unwrap();
        assert_eq!(convo.len(), 2);
        assert_eq!(convo[0].id, "m2"); // newest first
    }

    #[test]
    fn conversations_preview_groups_by_contact_and_picks_latest() {
        let db = temp_db("conversations");
        let conn = db.connection();
        let cid_a = seeded_contact(conn, "Alice");
        let cid_b = seeded_contact(conn, "Bob");

        save_alias_message(conn, &sample("a1", &cid_a, 100, true)).unwrap();
        save_alias_message(conn, &sample("a2", &cid_a, 300, false)).unwrap();
        save_alias_message(conn, &sample("b1", &cid_b, 200, true)).unwrap();

        let previews = get_alias_conversations(conn).unwrap();
        assert_eq!(previews.len(), 2);
        assert_eq!(previews[0].contact_id, cid_a); // newest last-message first
        assert_eq!(previews[0].message_count, 2);
        assert_eq!(previews[0].last_message_timestamp, 300);
        assert_eq!(previews[1].contact_id, cid_b);
        assert_eq!(previews[1].message_count, 1);
    }

    #[test]
    fn contact_with_no_messages_is_omitted_from_conversations() {
        let db = temp_db("no-messages");
        let conn = db.connection();
        seeded_contact(conn, "Ghost");
        assert!(get_alias_conversations(conn).unwrap().is_empty());
    }
}
