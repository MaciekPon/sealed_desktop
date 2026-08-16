// Mirrors sealed-desktop/src-tauri/src/commands/alias.rs.

export interface AliasInvite {
  inviteRef: string;
  /** base64 — show as a QR and/or copyable text; the creator sends this to the other party. */
  envelopeBase64: string;
  /** Unix seconds. */
  createdAt: number;
}

export interface AliasPendingInvite {
  inviteRef: string;
  label: string | null;
  /** Local-only UI metadata — set when this invite was delivered directly
   * to a known contact (`createInviteForContact`), `null` for the QR/paste
   * flow. Never transmitted on-chain. */
  peerWallet: string | null;
  /** Unix seconds. */
  createdAt: number;
  /** Hidden from the pending-invites banner but still completable via `complete_invite`. */
  dismissed: boolean;
}

export interface AliasContact {
  contactId: string;
  label: string | null;
  /** true if this side minted the invite; false if this side accepted one. */
  isCreator: boolean;
  /** Local-only UI metadata — see `AliasPendingInvite.peerWallet`. */
  peerWallet: string | null;
  /** Unix seconds. */
  createdAt: number;
  /** Unix seconds. */
  establishedAt: number;
}

/** An invite delivered directly to a known contact (not QR/paste),
 * awaiting an explicit Accept/Decline decision. */
export interface AliasIncomingInvite {
  inviteRef: string;
  peerWallet: string;
  peerUsername: string | null;
  /** Unix seconds. */
  receivedAt: number;
}

export interface AliasAcceptResult {
  contact: AliasContact;
  /** base64 — the acceptor sends this back to the creator to complete the handshake. */
  replyEnvelopeBase64: string;
}

export interface AliasMessage {
  id: string;
  contactId: string;
  content: string;
  /** Unix seconds. */
  timestamp: number;
  isOutgoing: boolean;
}

export interface AliasConversationPreview {
  contactId: string;
  label: string | null;
  lastMessagePreview: string;
  /** Unix seconds. */
  lastMessageTimestamp: number;
  isLastMessageOutgoing: boolean;
  unreadCount: number;
  messageCount: number;
}
