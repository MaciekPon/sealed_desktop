/**
 * In-memory stand-in for the Tauri backend, used only when the app is
 * opened outside a real Tauri webview (see `tauriEnv.ts`) — a pure visual
 * preview path for whenever the compiled binary can't be run locally
 * (e.g. Windows Smart App Control blocking freshly-built, unsigned dev
 * binaries). Every command the frontend currently calls has a handler
 * here; nothing here talks to real crypto, chain, or disk — it's demo
 * data plus enough state to make the UI feel alive (create a wallet,
 * click through contacts, send a message and see it appear).
 */

import type {
  AliasContact,
  AliasConversationPreview,
  AliasIncomingInvite,
  AliasMessage,
  AliasPendingInvite,
  AppSettings,
  ContactKeys,
  ContactProfile,
  ConversationPreview,
  DecryptedMessage,
  UnlockOutcome,
} from "../models";

function randomWalletAddress(): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let out = "";
  for (let i = 0; i < 58; i++) out += alphabet[Math.floor(Math.random() * alphabet.length)];
  return out;
}

function fakeBase64(byteLength: number): string {
  const bytes = new Uint8Array(byteLength);
  crypto.getRandomValues(bytes);
  return btoa(String.fromCharCode(...bytes));
}

function randomHex(byteLength: number): string {
  const bytes = new Uint8Array(byteLength);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * The real backend's envelopes are opaque encrypted binary — this mock has
 * no real crypto, so it round-trips the invite ref through a small JSON
 * envelope instead. Good enough to make the QR/paste flow clickable
 * end-to-end without a live Rust backend.
 */
function mockEnvelope(kind: "invite" | "accept", inviteRef: string): string {
  return btoa(JSON.stringify({ kind, inviteRef }));
}

function decodeMockEnvelope(envelopeBase64: string): { kind: string; inviteRef: string } | null {
  try {
    const parsed = JSON.parse(atob(envelopeBase64));
    return typeof parsed?.inviteRef === "string" ? parsed : null;
  } catch {
    return null;
  }
}

const DEMO_MNEMONIC =
  "alpha bravo charlie delta echo foxtrot golf hotel india juliett kilo lima " +
  "mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray";

type MockContact = ContactProfile;

const nowSeconds = () => Math.floor(Date.now() / 1000);

function seedContacts(): MockContact[] {
  return [
    { walletAddress: randomWalletAddress(), username: "Andrew", encryptionPubkey: fakeBase64(32), scanPubkey: fakeBase64(32), pqPublicKey: null, pqPubkeyHash: null, createdAt: nowSeconds() - 86400, isContact: true, isBlocked: false },
    { walletAddress: randomWalletAddress(), username: "Andrew 2", encryptionPubkey: fakeBase64(32), scanPubkey: fakeBase64(32), pqPublicKey: null, pqPubkeyHash: null, createdAt: nowSeconds() - 80000, isContact: true, isBlocked: false },
    { walletAddress: randomWalletAddress(), username: "Greg", encryptionPubkey: fakeBase64(32), scanPubkey: fakeBase64(32), pqPublicKey: null, pqPubkeyHash: null, createdAt: nowSeconds() - 70000, isContact: true, isBlocked: false },
    { walletAddress: randomWalletAddress(), username: "Gunter", encryptionPubkey: fakeBase64(32), scanPubkey: fakeBase64(32), pqPublicKey: null, pqPubkeyHash: null, createdAt: nowSeconds() - 60000, isContact: true, isBlocked: false },
    // Unnamed, not manually added — demo data for the Spam tab (mirrors an auto-cached key row from an unsolicited message).
    { walletAddress: randomWalletAddress(), username: null, encryptionPubkey: fakeBase64(32), scanPubkey: fakeBase64(32), pqPublicKey: null, pqPubkeyHash: null, createdAt: nowSeconds() - 50000, isContact: false, isBlocked: false },
    { walletAddress: randomWalletAddress(), username: null, encryptionPubkey: fakeBase64(32), scanPubkey: fakeBase64(32), pqPublicKey: null, pqPubkeyHash: null, createdAt: nowSeconds() - 40000, isContact: false, isBlocked: false },
  ];
}

const state = {
  hasAccount: false,
  unlocked: false,
  walletAddress: "",
  mnemonic: "",
  contacts: [] as MockContact[],
  conversations: new Map<string, DecryptedMessage[]>(),
  autoSyncEnabled: true,
  terminationConfigured: false,
  aliasPendingInvites: [] as AliasPendingInvite[],
  aliasContacts: [] as AliasContact[],
  aliasIncomingInvites: [] as AliasIncomingInvite[],
  aliasConversations: new Map<string, AliasMessage[]>(),
};

function ensureDemoData() {
  if (state.contacts.length > 0) return;
  state.contacts = seedContacts();
  const greg = state.contacts.find((c) => c.username === "Greg")!;
  state.conversations.set(greg.walletAddress, [
    { id: "demo-3", senderWallet: greg.walletAddress, senderUsername: "Greg", recipientWallet: state.walletAddress, recipientUsername: null, content: "Got the files, thanks!", timestamp: nowSeconds() - 300, isOutgoing: false, onChainPubkey: "demo-3" },
    { id: "demo-2", senderWallet: state.walletAddress, senderUsername: null, recipientWallet: greg.walletAddress, recipientUsername: "Greg", content: "Sent them over, let me know if anything's missing.", timestamp: nowSeconds() - 600, isOutgoing: true, onChainPubkey: "demo-2" },
    { id: "demo-1", senderWallet: greg.walletAddress, senderUsername: "Greg", recipientWallet: state.walletAddress, recipientUsername: null, content: "Hey, can you send over the design files?", timestamp: nowSeconds() - 900, isOutgoing: false, onChainPubkey: "demo-1" },
  ]);
}

function conversationPreviews(): ConversationPreview[] {
  const previews: ConversationPreview[] = [];
  for (const [wallet, messages] of state.conversations.entries()) {
    if (messages.length === 0) continue;
    const last = messages[0]; // stored newest-first, matching the real backend's ordering
    const contact = state.contacts.find((c) => c.walletAddress === wallet);
    previews.push({
      contactWallet: wallet,
      contactUsername: contact?.username ?? null,
      lastMessagePreview: last.content,
      lastMessageTimestamp: last.timestamp,
      isLastMessageOutgoing: last.isOutgoing,
      unreadCount: 0,
      messageCount: messages.length,
      isBlocked: contact?.isBlocked ?? false,
    });
  }
  return previews.sort((a, b) => b.lastMessageTimestamp - a.lastMessageTimestamp);
}

function aliasConversationPreviews(): AliasConversationPreview[] {
  const previews: AliasConversationPreview[] = [];
  for (const [contactId, messages] of state.aliasConversations.entries()) {
    if (messages.length === 0) continue;
    const last = messages[0]; // stored newest-first
    const contact = state.aliasContacts.find((c) => c.contactId === contactId);
    previews.push({
      contactId,
      label: contact?.label ?? null,
      lastMessagePreview: last.content,
      lastMessageTimestamp: last.timestamp,
      isLastMessageOutgoing: last.isOutgoing,
      unreadCount: 0,
      messageCount: messages.length,
    });
  }
  return previews.sort((a, b) => b.lastMessageTimestamp - a.lastMessageTimestamp);
}

export async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // Small artificial delay so loading states are visible, same as a real IPC round-trip.
  await new Promise((r) => setTimeout(r, 120));

  switch (cmd) {
    case "has_existing_account":
      return state.hasAccount as T;
    case "is_unlocked":
      return state.unlocked as T;

    case "create_account": {
      state.hasAccount = true;
      state.unlocked = true;
      state.walletAddress = randomWalletAddress();
      state.mnemonic = DEMO_MNEMONIC;
      ensureDemoData();
      return { walletAddress: state.walletAddress, mnemonic: state.mnemonic } as T;
    }
    case "restore_account": {
      state.hasAccount = true;
      state.unlocked = true;
      state.walletAddress = randomWalletAddress();
      state.mnemonic = String(args?.mnemonic ?? DEMO_MNEMONIC);
      ensureDemoData();
      return { walletAddress: state.walletAddress, mnemonic: state.mnemonic } as T;
    }
    case "unlock_account": {
      state.unlocked = true;
      ensureDemoData();
      const outcome: UnlockOutcome = {
        type: "success",
        walletAddress: state.walletAddress,
        encryptionPubkey: fakeBase64(32),
        scanPubkey: fakeBase64(32),
      };
      return outcome as T;
    }
    case "lock_account":
      state.unlocked = false;
      return undefined as T;
    case "change_pin":
      return undefined as T;

    case "get_app_settings":
      return { autoSyncEnabled: state.autoSyncEnabled } satisfies AppSettings as T;
    case "set_auto_sync_enabled":
      state.autoSyncEnabled = Boolean(args?.enabled);
      return undefined as T;

    case "is_termination_configured":
      return state.terminationConfigured as T;
    case "set_termination_code":
      state.terminationConfigured = true;
      return undefined as T;
    case "disable_termination_code":
      state.terminationConfigured = false;
      return undefined as T;
    case "verify_pin":
      return true as T;
    case "get_seed_phrase_for_backup":
      return state.mnemonic as T;
    case "log_out": {
      state.hasAccount = false;
      state.unlocked = false;
      state.walletAddress = "";
      state.mnemonic = "";
      state.contacts = [];
      state.conversations.clear();
      state.terminationConfigured = false;
      return undefined as T;
    }

    case "get_all_contacts":
      return [...state.contacts] as T;
    case "get_contact": {
      const c = state.contacts.find((c) => c.walletAddress === args?.walletAddress) ?? null;
      return c as T;
    }
    case "search_contact": {
      const q = String(args?.query ?? "").toLowerCase();
      const c = state.contacts.find((c) => c.username?.toLowerCase() === q || c.walletAddress === q) ?? null;
      return c as T;
    }
    case "search_contacts": {
      const q = String(args?.query ?? "").toLowerCase();
      return state.contacts.filter((c) => c.username?.toLowerCase().includes(q) || c.walletAddress.toLowerCase().includes(q)) as T;
    }
    case "save_contact": {
      const profile = args?.profile as ContactProfile;
      const idx = state.contacts.findIndex((c) => c.walletAddress === profile.walletAddress);
      if (idx >= 0) state.contacts[idx] = profile;
      else state.contacts.push(profile);
      return undefined as T;
    }
    case "delete_contact": {
      state.contacts = state.contacts.filter((c) => c.walletAddress !== args?.walletAddress);
      return undefined as T;
    }
    case "get_contact_keys":
    case "resolve_contact_keys": {
      const existing = state.contacts.find((c) => c.walletAddress === args?.walletAddress);
      const keys: ContactKeys = {
        pqPublicKey: null,
        pqSharedSecret: null,
        encryptionPubkey: fakeBase64(32),
        scanPubkey: fakeBase64(32),
        username: existing?.username ?? null,
      };
      return keys as T;
    }
    case "save_contact_keys":
      return undefined as T;

    case "add_to_contacts": {
      const walletAddress = String(args?.walletAddress);
      const idx = state.contacts.findIndex((c) => c.walletAddress === walletAddress);
      if (idx >= 0) {
        state.contacts[idx] = { ...state.contacts[idx], isContact: true };
      } else {
        state.contacts.push({
          walletAddress,
          username: null,
          encryptionPubkey: fakeBase64(32),
          scanPubkey: fakeBase64(32),
          pqPublicKey: null,
          pqPubkeyHash: null,
          createdAt: nowSeconds(),
          isContact: true,
          isBlocked: false,
        });
      }
      return undefined as T;
    }
    case "remove_from_contacts": {
      const idx = state.contacts.findIndex((c) => c.walletAddress === args?.walletAddress);
      if (idx >= 0) state.contacts[idx] = { ...state.contacts[idx], isContact: false };
      return undefined as T;
    }
    case "block_contact": {
      const idx = state.contacts.findIndex((c) => c.walletAddress === args?.walletAddress);
      if (idx >= 0) state.contacts[idx] = { ...state.contacts[idx], isBlocked: true, isContact: false };
      return undefined as T;
    }
    case "unblock_contact": {
      const idx = state.contacts.findIndex((c) => c.walletAddress === args?.walletAddress);
      if (idx >= 0) state.contacts[idx] = { ...state.contacts[idx], isBlocked: false };
      return undefined as T;
    }

    case "resolve_username":
      return randomWalletAddress() as T;
    case "check_username_available":
      return true as T;
    case "search_usernames":
      return [] as T;
    case "claim_username":
    case "release_username":
      return "DEMOTXID" as T;

    case "get_credits":
      return 10 as T;
    case "estimate_credit_cost":
      return { current: 10, after: 9 } as T;
    case "get_wallet_balance":
      return 5_000_000 as T; // 5 ALGO
    case "redeem_code":
      return "DEMOTXID" as T;
    case "ensure_keys_published":
      return false as T;

    case "get_all_conversations":
      return conversationPreviews() as T;
    case "get_conversation": {
      const messages = state.conversations.get(String(args?.contactWallet)) ?? [];
      return messages as T;
    }
    case "send_message": {
      const recipientWallet = String(args?.recipientWallet);
      const message: DecryptedMessage = {
        id: `demo-${Date.now()}`,
        senderWallet: state.walletAddress,
        senderUsername: null,
        recipientWallet,
        recipientUsername: (args?.recipientUsername as string | null) ?? null,
        content: String(args?.plaintext ?? ""),
        timestamp: nowSeconds(),
        isOutgoing: true,
        onChainPubkey: `demo-${Date.now()}`,
      };
      const existing = state.conversations.get(recipientWallet) ?? [];
      state.conversations.set(recipientWallet, [message, ...existing]);
      return message.id as T;
    }
    case "sync_messages":
    case "force_resync":
      return 0 as T;
    case "mark_conversation_as_read":
      return undefined as T;
    case "get_unread_count":
      return 0 as T;
    case "delete_conversation": {
      const wallet = String(args?.contactWallet);
      const count = state.conversations.get(wallet)?.length ?? 0;
      state.conversations.delete(wallet);
      return count as T;
    }

    case "create_invite": {
      const inviteRef = randomHex(16);
      const label = (args?.label as string | null) ?? null;
      const createdAt = nowSeconds();
      state.aliasPendingInvites.push({ inviteRef, label, peerWallet: null, createdAt, dismissed: false });
      return { inviteRef, envelopeBase64: mockEnvelope("invite", inviteRef), createdAt } as T;
    }
    case "list_pending_invites":
      return [...state.aliasPendingInvites] as T;
    case "dismiss_pending_invite": {
      const idx = state.aliasPendingInvites.findIndex((p) => p.inviteRef === args?.inviteRef);
      if (idx >= 0) state.aliasPendingInvites[idx] = { ...state.aliasPendingInvites[idx], dismissed: true };
      return undefined as T;
    }
    case "delete_pending_invite": {
      state.aliasPendingInvites = state.aliasPendingInvites.filter((p) => p.inviteRef !== args?.inviteRef);
      return undefined as T;
    }
    case "accept_invite": {
      const decoded = decodeMockEnvelope(String(args?.envelopeBase64 ?? ""));
      if (!decoded) throw new Error("malformed invite envelope");
      const contactId = decoded.inviteRef;
      const label = (args?.label as string | null) ?? null;
      const now = nowSeconds();
      const contact: AliasContact = { contactId, label, isCreator: false, peerWallet: null, createdAt: now, establishedAt: now };
      if (!state.aliasContacts.some((c) => c.contactId === contactId)) state.aliasContacts.push(contact);
      return { contact, replyEnvelopeBase64: mockEnvelope("accept", contactId) } as T;
    }
    case "complete_invite": {
      const decoded = decodeMockEnvelope(String(args?.envelopeBase64 ?? ""));
      if (!decoded) throw new Error("malformed accept envelope");
      const pendingIdx = state.aliasPendingInvites.findIndex((p) => p.inviteRef === decoded.inviteRef);
      if (pendingIdx < 0) throw new Error("no matching pending invite for this reply");
      const [pending] = state.aliasPendingInvites.splice(pendingIdx, 1);
      const now = nowSeconds();
      const contact: AliasContact = {
        contactId: pending.inviteRef,
        label: pending.label,
        isCreator: true,
        peerWallet: pending.peerWallet,
        createdAt: pending.createdAt,
        establishedAt: now,
      };
      state.aliasContacts.push(contact);
      return contact as T;
    }
    case "send_alias_message": {
      const contactId = String(args?.contactId);
      const message: AliasMessage = { id: `alias-demo-${Date.now()}`, contactId, content: String(args?.plaintext ?? ""), timestamp: nowSeconds(), isOutgoing: true };
      const existing = state.aliasConversations.get(contactId) ?? [];
      state.aliasConversations.set(contactId, [message, ...existing]);
      return message.id as T;
    }
    case "get_alias_contacts":
      return [...state.aliasContacts] as T;
    case "get_alias_conversations":
      return aliasConversationPreviews() as T;
    case "get_alias_conversation":
      return (state.aliasConversations.get(String(args?.contactId)) ?? []) as T;
    case "mark_alias_conversation_read":
      return undefined as T;
    case "rename_alias_contact": {
      const idx = state.aliasContacts.findIndex((c) => c.contactId === args?.contactId);
      if (idx >= 0) state.aliasContacts[idx] = { ...state.aliasContacts[idx], label: String(args?.label ?? "") };
      return undefined as T;
    }
    case "delete_alias_contact": {
      const contactId = String(args?.contactId);
      state.aliasContacts = state.aliasContacts.filter((c) => c.contactId !== contactId);
      state.aliasConversations.delete(contactId);
      return undefined as T;
    }

    case "create_invite_for_contact": {
      const recipientWallet = String(args?.recipientWallet);
      const label = (args?.label as string | null) ?? null;
      const inviteRef = randomHex(16);
      const createdAt = nowSeconds();
      const pending: AliasPendingInvite = { inviteRef, label, peerWallet: recipientWallet, createdAt, dismissed: false };
      state.aliasPendingInvites.push(pending);
      return pending as T;
    }
    case "list_incoming_invites":
      return [...state.aliasIncomingInvites] as T;
    case "accept_incoming_invite": {
      const inviteRef = String(args?.inviteRef);
      const idx = state.aliasIncomingInvites.findIndex((i) => i.inviteRef === inviteRef);
      if (idx < 0) throw new Error("unknown incoming invite");
      const [incoming] = state.aliasIncomingInvites.splice(idx, 1);
      const label = (args?.label as string | null) ?? null;
      const now = nowSeconds();
      const contact: AliasContact = { contactId: inviteRef, label, isCreator: false, peerWallet: incoming.peerWallet, createdAt: now, establishedAt: now };
      state.aliasContacts.push(contact);
      return contact as T;
    }
    case "decline_incoming_invite": {
      const inviteRef = String(args?.inviteRef);
      state.aliasIncomingInvites = state.aliasIncomingInvites.filter((i) => i.inviteRef !== inviteRef);
      return undefined as T;
    }

    default:
      console.warn(`[mockBackend] unhandled command "${cmd}" — returning undefined`, args);
      return undefined as T;
  }
}
