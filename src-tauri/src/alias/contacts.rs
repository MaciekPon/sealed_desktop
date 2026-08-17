//! CRUD for `alias_pending_invites` and `alias_contacts`, mirroring the
//! pure-`rusqlite::Connection` style of the top-level `contacts.rs` (which
//! owns the separate, untouched `contacts_cache` table for wallet contacts).

use rusqlite::{params, Connection, OptionalExtension};

use super::onboarding::{AcceptedInvite, CompletedInvite, CreatedInvite};

pub struct PendingInvite {
    pub invite_ref: String,
    pub label: Option<String>,
    /// Local-only UI convenience metadata — the wallet this invite was
    /// delivered to via a regular DM (`commands::alias::create_invite_for_contact`),
    /// `None` for the QR/paste flow. Never transmitted on-chain or embedded
    /// in the wire envelope — see `db/mod.rs`'s `create_schema_v4` doc
    /// comment for why this doesn't weaken alias-identity unlinkability.
    pub peer_wallet: Option<String>,
    pub my_enc_seed: [u8; 32],
    pub my_enc_pub: [u8; 32],
    pub my_scan_seed: [u8; 32],
    pub my_scan_pub: [u8; 32],
    pub my_pq_sk: Vec<u8>,
    // Same reasoning as `AliasContact::my_scan_pub` above — the public half
    // was already sent in the invite envelope; only the secret half
    // (`my_pq_sk`, for decapsulating the eventual accept reply) is read
    // again later.
    #[allow(dead_code)]
    pub my_pq_pub: Vec<u8>,
    pub created_at: i64,
    pub dismissed: bool,
}

fn blob32(row: &rusqlite::Row, idx: &str) -> rusqlite::Result<[u8; 32]> {
    let v: Vec<u8> = row.get(idx)?;
    v.try_into().map_err(|_| rusqlite::Error::InvalidColumnType(0, idx.to_string(), rusqlite::types::Type::Blob))
}

fn row_to_pending_invite(row: &rusqlite::Row) -> rusqlite::Result<PendingInvite> {
    Ok(PendingInvite {
        invite_ref: row.get("invite_ref")?,
        label: row.get("label")?,
        peer_wallet: row.get("peer_wallet")?,
        my_enc_seed: blob32(row, "my_x25519_enc_sk")?,
        my_enc_pub: blob32(row, "my_x25519_enc_pub")?,
        my_scan_seed: blob32(row, "my_x25519_scan_sk")?,
        my_scan_pub: blob32(row, "my_x25519_scan_pub")?,
        my_pq_sk: row.get("my_mlkem_sk")?,
        my_pq_pub: row.get("my_mlkem_pub")?,
        created_at: row.get("created_at")?,
        dismissed: row.get("dismissed")?,
    })
}

const PENDING_COLUMNS: &str = "invite_ref, label, peer_wallet, my_x25519_enc_sk, my_x25519_enc_pub, my_x25519_scan_sk, my_x25519_scan_pub, my_mlkem_sk, my_mlkem_pub, created_at, dismissed";

pub fn save_pending_invite(
    conn: &Connection,
    invite_ref_hex: &str,
    label: Option<&str>,
    peer_wallet: Option<&str>,
    created: &CreatedInvite,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO alias_pending_invites \
         (invite_ref, label, peer_wallet, my_x25519_enc_sk, my_x25519_enc_pub, my_x25519_scan_sk, my_x25519_scan_pub, my_mlkem_sk, my_mlkem_pub, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            invite_ref_hex,
            label,
            peer_wallet,
            created.my_enc_seed.to_vec(),
            created.my_enc_pub.to_vec(),
            created.my_scan_seed.to_vec(),
            created.my_scan_pub.to_vec(),
            created.my_pq_sk,
            created.my_pq_pub,
            now,
        ],
    )?;
    Ok(())
}

/// Linear scan for a non-dismissed pending invite already targeting
/// `wallet_address` — used by `ContactProfile`'s "Invitation pending…"
/// state and by `create_invite_for_contact`'s duplicate-invite guard.
pub fn find_pending_invite_for_wallet(conn: &Connection, wallet_address: &str) -> rusqlite::Result<Option<PendingInvite>> {
    let mut stmt = conn.prepare(&format!("SELECT {PENDING_COLUMNS} FROM alias_pending_invites WHERE peer_wallet = ?1 AND dismissed = 0 ORDER BY created_at DESC LIMIT 1"))?;
    stmt.query_row(params![wallet_address], row_to_pending_invite).optional()
}

pub fn get_pending_invite(conn: &Connection, invite_ref_hex: &str) -> rusqlite::Result<Option<PendingInvite>> {
    conn.query_row(
        &format!("SELECT {PENDING_COLUMNS} FROM alias_pending_invites WHERE invite_ref = ?1"),
        params![invite_ref_hex],
        row_to_pending_invite,
    )
    .optional()
}

pub fn get_all_pending_invites(conn: &Connection) -> rusqlite::Result<Vec<PendingInvite>> {
    let mut stmt = conn.prepare(&format!("SELECT {PENDING_COLUMNS} FROM alias_pending_invites ORDER BY created_at DESC"))?;
    let rows = stmt.query_map([], row_to_pending_invite)?;
    rows.collect()
}

pub fn dismiss_pending_invite(conn: &Connection, invite_ref_hex: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE alias_pending_invites SET dismissed = 1 WHERE invite_ref = ?1", params![invite_ref_hex])?;
    Ok(())
}

pub fn delete_pending_invite(conn: &Connection, invite_ref_hex: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM alias_pending_invites WHERE invite_ref = ?1", params![invite_ref_hex])?;
    Ok(())
}

pub struct AliasContact {
    pub contact_id: String,
    pub label: Option<String>,
    pub is_creator: bool,
    /// See `PendingInvite::peer_wallet`'s doc comment — same local-only,
    /// off-chain convenience metadata, carried forward from the pending
    /// invite (creator side) or passed in directly (acceptor side).
    pub peer_wallet: Option<String>,
    pub my_enc_seed: [u8; 32],
    pub my_scan_seed: [u8; 32],
    // Loaded alongside `my_scan_seed` for a complete row, but nothing needs
    // it back: `check_recipient_tag`/message decrypt only ever use the
    // private seed, and this account's own scan pubkey was already
    // embedded in the invite/accept envelope sent to the peer.
    #[allow(dead_code)]
    pub my_scan_pub: [u8; 32],
    pub peer_enc_pub: [u8; 32],
    pub peer_scan_pub: [u8; 32],
    pub pq_shared_secret: Vec<u8>,
    pub created_at: i64,
    pub established_at: i64,
}

const CONTACT_COLUMNS: &str = "contact_id, label, is_creator, peer_wallet, my_x25519_enc_sk, my_x25519_scan_sk, my_x25519_scan_pub, \
     peer_x25519_enc_pub, peer_x25519_scan_pub, pq_shared_secret, created_at, established_at";

fn row_to_alias_contact(row: &rusqlite::Row) -> rusqlite::Result<AliasContact> {
    Ok(AliasContact {
        contact_id: row.get("contact_id")?,
        label: row.get("label")?,
        is_creator: row.get("is_creator")?,
        peer_wallet: row.get("peer_wallet")?,
        my_enc_seed: blob32(row, "my_x25519_enc_sk")?,
        my_scan_seed: blob32(row, "my_x25519_scan_sk")?,
        my_scan_pub: blob32(row, "my_x25519_scan_pub")?,
        peer_enc_pub: blob32(row, "peer_x25519_enc_pub")?,
        peer_scan_pub: blob32(row, "peer_x25519_scan_pub")?,
        pq_shared_secret: row.get("pq_shared_secret")?,
        created_at: row.get("created_at")?,
        established_at: row.get("established_at")?,
    })
}

/// Linear scan for an established alias contact whose invite originated
/// from `wallet_address` — used by `ContactProfile`'s "Open alias chat"
/// state.
pub fn find_alias_contact_for_wallet(conn: &Connection, wallet_address: &str) -> rusqlite::Result<Option<AliasContact>> {
    let mut stmt = conn.prepare(&format!("SELECT {CONTACT_COLUMNS} FROM alias_contacts WHERE peer_wallet = ?1 ORDER BY established_at DESC LIMIT 1"))?;
    stmt.query_row(params![wallet_address], row_to_alias_contact).optional()
}

/// Creator side: deletes the pending row and inserts the established
/// contact row in one transaction. `my_mlkem_sk` carries over from the
/// pending row (the creator's own PQ secret); `peer_mlkem_pub` stays NULL
/// (the acceptor never has a PQ key of its own).
pub fn promote_creator_pending_to_contact(conn: &mut Connection, pending: &PendingInvite, completed: &CompletedInvite, now: i64) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM alias_pending_invites WHERE invite_ref = ?1", params![pending.invite_ref])?;
    tx.execute(
        "INSERT INTO alias_contacts \
         (contact_id, label, is_creator, peer_wallet, my_x25519_enc_sk, my_x25519_enc_pub, my_x25519_scan_sk, my_x25519_scan_pub, \
          my_mlkem_sk, peer_x25519_enc_pub, peer_x25519_scan_pub, peer_mlkem_pub, pq_shared_secret, created_at, established_at) \
         VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?12, ?13)",
        params![
            pending.invite_ref,
            pending.label,
            pending.peer_wallet,
            pending.my_enc_seed.to_vec(),
            pending.my_enc_pub.to_vec(),
            pending.my_scan_seed.to_vec(),
            pending.my_scan_pub.to_vec(),
            pending.my_pq_sk,
            completed.peer_enc_pub.to_vec(),
            completed.peer_scan_pub.to_vec(),
            completed.pq_shared_secret.to_vec(),
            pending.created_at,
            now,
        ],
    )?;
    tx.commit()
}

/// Acceptor side: inserts a fully-established contact directly (no pending
/// row ever exists for the acceptor). `my_mlkem_sk` stays NULL (acceptor
/// never generates a PQ key); `peer_mlkem_pub` is the creator's PQ public
/// key received in the invite envelope.
pub fn insert_accepted_contact(conn: &Connection, accepted: &AcceptedInvite, label: Option<&str>, peer_wallet: Option<&str>, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO alias_contacts \
         (contact_id, label, is_creator, peer_wallet, my_x25519_enc_sk, my_x25519_enc_pub, my_x25519_scan_sk, my_x25519_scan_pub, \
          my_mlkem_sk, peer_x25519_enc_pub, peer_x25519_scan_pub, peer_mlkem_pub, pq_shared_secret, created_at, established_at) \
         VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12, ?12)",
        params![
            accepted.contact_id,
            label,
            peer_wallet,
            accepted.my_enc_seed.to_vec(),
            accepted.my_enc_pub.to_vec(),
            accepted.my_scan_seed.to_vec(),
            accepted.my_scan_pub.to_vec(),
            accepted.peer_enc_pub.to_vec(),
            accepted.peer_scan_pub.to_vec(),
            accepted.peer_pq_pub,
            accepted.pq_shared_secret.to_vec(),
            now,
        ],
    )?;
    Ok(())
}

pub fn get_alias_contact(conn: &Connection, contact_id: &str) -> rusqlite::Result<Option<AliasContact>> {
    conn.query_row(&format!("SELECT {CONTACT_COLUMNS} FROM alias_contacts WHERE contact_id = ?1"), params![contact_id], row_to_alias_contact).optional()
}

pub fn get_all_alias_contacts(conn: &Connection) -> rusqlite::Result<Vec<AliasContact>> {
    let mut stmt = conn.prepare(&format!("SELECT {CONTACT_COLUMNS} FROM alias_contacts ORDER BY established_at DESC"))?;
    let rows = stmt.query_map([], row_to_alias_contact)?;
    rows.collect()
}

pub fn rename_alias_contact(conn: &Connection, contact_id: &str, label: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE alias_contacts SET label = ?1 WHERE contact_id = ?2", params![label, contact_id])?;
    Ok(())
}

/// Cascade-deletes `alias_messages` first — `db/mod.rs` never sets `PRAGMA
/// foreign_keys = ON`, so the `REFERENCES` clause on `alias_messages` is
/// documentation only; cascade must be explicit.
pub fn delete_alias_contact(conn: &mut Connection, contact_id: &str) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM alias_messages WHERE contact_id = ?1", params![contact_id])?;
    tx.execute("DELETE FROM alias_contacts WHERE contact_id = ?1", params![contact_id])?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alias::onboarding;
    use crate::db::Db;

    fn temp_db(name: &str) -> Db {
        let dir = std::env::temp_dir().join(format!("sealed-desktop-alias-contacts-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Db::open(&dir.join("test.db"), &[7u8; 32]).unwrap()
    }

    #[test]
    fn pending_invite_save_get_dismiss_delete_round_trip() {
        let db = temp_db("pending");
        let conn = db.connection();
        let created = onboarding::create_invitation_envelope().unwrap();
        let invite_ref_hex = crate::alias::envelope::hex_encode(&created.invite_ref);

        save_pending_invite(conn, &invite_ref_hex, Some("Bob"), Some("WALLETB"), &created, 1000).unwrap();
        let fetched = get_pending_invite(conn, &invite_ref_hex).unwrap().unwrap();
        assert_eq!(fetched.label.as_deref(), Some("Bob"));
        assert_eq!(fetched.peer_wallet.as_deref(), Some("WALLETB"));
        assert_eq!(fetched.my_enc_pub, created.my_enc_pub);
        assert!(!fetched.dismissed);
        assert_eq!(find_pending_invite_for_wallet(conn, "WALLETB").unwrap().unwrap().invite_ref, invite_ref_hex);

        assert_eq!(get_all_pending_invites(conn).unwrap().len(), 1);

        dismiss_pending_invite(conn, &invite_ref_hex).unwrap();
        assert!(get_pending_invite(conn, &invite_ref_hex).unwrap().unwrap().dismissed);

        delete_pending_invite(conn, &invite_ref_hex).unwrap();
        assert!(get_pending_invite(conn, &invite_ref_hex).unwrap().is_none());
    }

    #[test]
    fn promote_creator_pending_to_contact_is_transactional() {
        let mut db = temp_db("promote");
        let created = onboarding::create_invitation_envelope().unwrap();
        let invite_ref_hex = crate::alias::envelope::hex_encode(&created.invite_ref);

        save_pending_invite(db.connection(), &invite_ref_hex, None, Some("WALLETB"), &created, 1000).unwrap();
        let accepted = onboarding::accept_invitation_from_envelope(&created.envelope_bytes).unwrap();
        let pending = get_pending_invite(db.connection(), &invite_ref_hex).unwrap().unwrap();
        let completed =
            onboarding::complete_from_accept_envelope(&invite_ref_hex, &created.my_pq_sk, &accepted.accept_envelope_bytes).unwrap();

        promote_creator_pending_to_contact(db.connection_mut(), &pending, &completed, 2000).unwrap();

        assert!(get_pending_invite(db.connection(), &invite_ref_hex).unwrap().is_none());
        let contact = get_alias_contact(db.connection(), &invite_ref_hex).unwrap().unwrap();
        assert!(contact.is_creator);
        assert_eq!(contact.pq_shared_secret, completed.pq_shared_secret.to_vec());
        assert_eq!(contact.peer_enc_pub, accepted.my_enc_pub);
        assert_eq!(contact.peer_wallet.as_deref(), Some("WALLETB"));
        assert_eq!(find_alias_contact_for_wallet(db.connection(), "WALLETB").unwrap().unwrap().contact_id, invite_ref_hex);
    }

    #[test]
    fn insert_accepted_contact_and_rename_and_delete() {
        let mut db = temp_db("accept");
        let created = onboarding::create_invitation_envelope().unwrap();
        let accepted = onboarding::accept_invitation_from_envelope(&created.envelope_bytes).unwrap();

        insert_accepted_contact(db.connection(), &accepted, Some("Alice"), Some("WALLETA"), 1500).unwrap();
        let contact = get_alias_contact(db.connection(), &accepted.contact_id).unwrap().unwrap();
        assert!(!contact.is_creator);
        assert_eq!(contact.label.as_deref(), Some("Alice"));
        assert_eq!(contact.peer_wallet.as_deref(), Some("WALLETA"));

        rename_alias_contact(db.connection(), &accepted.contact_id, "Alice 2").unwrap();
        assert_eq!(get_alias_contact(db.connection(), &accepted.contact_id).unwrap().unwrap().label.as_deref(), Some("Alice 2"));

        assert_eq!(get_all_alias_contacts(db.connection()).unwrap().len(), 1);

        delete_alias_contact(db.connection_mut(), &accepted.contact_id).unwrap();
        assert!(get_alias_contact(db.connection(), &accepted.contact_id).unwrap().is_none());
    }
}
