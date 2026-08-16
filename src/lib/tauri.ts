/**
 * Typed wrappers around `@tauri-apps/api/core`'s `invoke()`, one per Rust
 * command module under `src-tauri/src/commands/`. This is the only file
 * that should ever import `invoke` directly — every other frontend module
 * should go through these functions instead of calling `invoke()` itself,
 * so the command surface stays in one place and the string command names
 * never get typo'd at multiple call sites.
 *
 * Wire format note: Tauri renames a command's *argument names* to
 * camelCase automatically (so a Rust `old_pin: String` parameter is sent
 * as `oldPin` from JS), but does NOT touch the field names *inside* a
 * struct/enum return or argument type — those follow whatever that type's
 * own `serde` attributes say. Every DTO struct on the Rust side has
 * `#[serde(rename_all = "camelCase")]` precisely so the two rules agree
 * and everything is camelCase end-to-end here.
 */

import { invoke } from "./invoke";
import type {
  AccountInfo,
  AliasAcceptResult,
  AliasContact,
  AliasConversationPreview,
  AliasIncomingInvite,
  AliasInvite,
  AliasMessage,
  AliasPendingInvite,
  AppSettings,
  ContactKeys,
  ContactKeysUpdate,
  ContactProfile,
  ConversationPreview,
  CreditCostInfo,
  DecryptedMessage,
  NewAccountInfo,
  UnlockOutcome,
  UsernameSearchHit,
} from "../models";

export const auth = {
  hasExistingAccount: () => invoke<boolean>("has_existing_account"),
  isUnlocked: () => invoke<boolean>("is_unlocked"),
  createAccount: (pin: string) => invoke<NewAccountInfo>("create_account", { pin }),
  restoreAccount: (pin: string, mnemonic: string) => invoke<NewAccountInfo>("restore_account", { pin, mnemonic }),
  unlockAccount: (pin: string) => invoke<UnlockOutcome>("unlock_account", { pin }),
  lockAccount: () => invoke<void>("lock_account"),
  changePin: (oldPin: string, newPin: string) => invoke<void>("change_pin", { oldPin, newPin }),
};

export const username = {
  claim: (name: string, oldName?: string) => invoke<string>("claim_username", { name, oldName: oldName ?? null }),
  release: (oldName?: string) => invoke<string>("release_username", { oldName: oldName ?? null }),
  resolve: (name: string) => invoke<string>("resolve_username", { name }),
  checkAvailable: (name: string) => invoke<boolean>("check_username_available", { name }),
  search: (query: string, limit: number) => invoke<UsernameSearchHit[]>("search_usernames", { query, limit }),
};

export const credits = {
  get: () => invoke<number>("get_credits"),
  estimateCost: () => invoke<CreditCostInfo>("estimate_credit_cost"),
  redeem: (code: string, username?: string) => invoke<string>("redeem_code", { code, username: username ?? null }),
};

export const keys = {
  /** Best-effort; returns `true` only if a publish txn was actually submitted. */
  ensurePublished: () => invoke<boolean>("ensure_keys_published"),
};

export const settings = {
  get: () => invoke<AppSettings>("get_app_settings"),
  setAutoSyncEnabled: (enabled: boolean) => invoke<void>("set_auto_sync_enabled", { enabled }),
  isTerminationConfigured: () => invoke<boolean>("is_termination_configured"),
  setTerminationCode: (code: string) => invoke<void>("set_termination_code", { code }),
  disableTerminationCode: () => invoke<void>("disable_termination_code"),
  verifyTerminationCode: (code: string) => invoke<boolean>("verify_termination_code", { code }),
  verifyPin: (pin: string) => invoke<boolean>("verify_pin", { pin }),
  getSeedPhraseForBackup: () => invoke<string>("get_seed_phrase_for_backup"),
  logOut: () => invoke<void>("log_out"),
};

export const contacts = {
  save: (profile: ContactProfile) => invoke<void>("save_contact", { profile }),
  get: (walletAddress: string) => invoke<ContactProfile | null>("get_contact", { walletAddress }),
  getAll: () => invoke<ContactProfile[]>("get_all_contacts"),
  delete: (walletAddress: string) => invoke<void>("delete_contact", { walletAddress }),
  clear: () => invoke<void>("clear_contacts"),
  search: (query: string) => invoke<ContactProfile | null>("search_contact", { query }),
  searchMany: (query: string, limit: number) => invoke<ContactProfile[]>("search_contacts", { query, limit }),
  getKeys: (walletAddress: string) => invoke<ContactKeys>("get_contact_keys", { walletAddress }),
  saveKeys: (walletAddress: string, update: ContactKeysUpdate) => invoke<void>("save_contact_keys", { walletAddress, update }),
  resolveKeys: (walletAddress: string) => invoke<ContactKeys>("resolve_contact_keys", { walletAddress }),
  addToContacts: (walletAddress: string) => invoke<void>("add_to_contacts", { walletAddress }),
  removeFromContacts: (walletAddress: string) => invoke<void>("remove_from_contacts", { walletAddress }),
  block: (walletAddress: string) => invoke<void>("block_contact", { walletAddress }),
  unblock: (walletAddress: string) => invoke<void>("unblock_contact", { walletAddress }),
};

export const messaging = {
  send: (recipientWallet: string, plaintext: string, recipientUsername?: string) =>
    invoke<string>("send_message", { recipientWallet, recipientUsername: recipientUsername ?? null, plaintext }),
  sync: (fullSync: boolean) => invoke<number>("sync_messages", { fullSync }),
  forceResync: () => invoke<number>("force_resync"),
  getConversation: (contactWallet: string) => invoke<DecryptedMessage[]>("get_conversation", { contactWallet }),
  getAllConversations: () => invoke<ConversationPreview[]>("get_all_conversations"),
  markConversationAsRead: (contactWallet: string) => invoke<void>("mark_conversation_as_read", { contactWallet }),
  getUnreadCount: (contactWallet: string) => invoke<number>("get_unread_count", { contactWallet }),
  deleteConversation: (contactWallet: string) => invoke<number>("delete_conversation", { contactWallet }),
};

export const wallet = {
  /** microAlgos. */
  getBalance: () => invoke<number>("get_wallet_balance"),
};

export const alias = {
  createInvite: (label?: string) => invoke<AliasInvite>("create_invite", { label: label ?? null }),
  listPendingInvites: () => invoke<AliasPendingInvite[]>("list_pending_invites"),
  dismissPendingInvite: (inviteRef: string) => invoke<void>("dismiss_pending_invite", { inviteRef }),
  deletePendingInvite: (inviteRef: string) => invoke<void>("delete_pending_invite", { inviteRef }),
  acceptInvite: (envelopeBase64: string, label?: string) =>
    invoke<AliasAcceptResult>("accept_invite", { envelopeBase64, label: label ?? null }),
  completeInvite: (envelopeBase64: string) => invoke<AliasContact>("complete_invite", { envelopeBase64 }),
  sendMessage: (contactId: string, plaintext: string) => invoke<string>("send_alias_message", { contactId, plaintext }),
  getContacts: () => invoke<AliasContact[]>("get_alias_contacts"),
  getConversations: () => invoke<AliasConversationPreview[]>("get_alias_conversations"),
  getConversation: (contactId: string) => invoke<AliasMessage[]>("get_alias_conversation", { contactId }),
  markConversationAsRead: (contactId: string) => invoke<void>("mark_alias_conversation_read", { contactId }),
  rename: (contactId: string, label: string) => invoke<void>("rename_alias_contact", { contactId, label }),
  delete: (contactId: string) => invoke<void>("delete_alias_contact", { contactId }),
  createInviteForContact: (recipientWallet: string, label?: string) =>
    invoke<AliasPendingInvite>("create_invite_for_contact", { recipientWallet, label: label ?? null }),
  listIncomingInvites: () => invoke<AliasIncomingInvite[]>("list_incoming_invites"),
  acceptIncomingInvite: (inviteRef: string, label?: string) =>
    invoke<AliasContact>("accept_incoming_invite", { inviteRef, label: label ?? null }),
  declineIncomingInvite: (inviteRef: string) => invoke<void>("decline_incoming_invite", { inviteRef }),
};

export type { AccountInfo, UnlockOutcome };
