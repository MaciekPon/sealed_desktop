// Mirrors sealed-desktop/src-tauri/src/commands/messaging.rs.

export interface DecryptedMessage {
  id: string;
  senderWallet: string;
  senderUsername: string | null;
  recipientWallet: string;
  recipientUsername: string | null;
  content: string;
  /** Unix seconds. */
  timestamp: number;
  isOutgoing: boolean;
  onChainPubkey: string;
}

export interface ConversationPreview {
  contactWallet: string;
  contactUsername: string | null;
  lastMessagePreview: string;
  /** Unix seconds. */
  lastMessageTimestamp: number;
  isLastMessageOutgoing: boolean;
  unreadCount: number;
  messageCount: number;
  /** The sole Spam-tab predicate — everything else belongs in Chats. */
  isBlocked: boolean;
}
