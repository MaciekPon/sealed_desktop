// Mirrors sealed-desktop/src-tauri/src/commands/contacts.rs.

export interface ContactProfile {
  walletAddress: string;
  username: string | null;
  /** base64 */
  encryptionPubkey: string;
  /** base64 */
  scanPubkey: string;
  /** base64 */
  pqPublicKey: string | null;
  /** base64 */
  pqPubkeyHash: string | null;
  /** Unix seconds. */
  createdAt: number;
  /** Deliberately added (e.g. via "Start Chat"/"Add to contacts") — Chats tab, not Spam. Ignored by `contacts.save()`; change via `contacts.addToContacts`/`removeFromContacts`. */
  isContact: boolean;
  /** Manually blocked — Spam tab. Blocking also clears `isContact`; unblocking does not restore it. */
  isBlocked: boolean;
}

/** Whatever subset of a contact's keys is currently cached — missing keys are `null`. */
export interface ContactKeys {
  /** base64 */
  pqPublicKey: string | null;
  /** base64 */
  pqSharedSecret: string | null;
  /** base64 */
  encryptionPubkey: string | null;
  /** base64 */
  scanPubkey: string | null;
  username: string | null;
}

/** Any subset of keys to persist — omitted/undefined fields leave the existing stored value untouched. */
export interface ContactKeysUpdate {
  pqPublicKey?: string;
  pqSharedSecret?: string;
  encryptionPubkey?: string;
  scanPubkey?: string;
}
