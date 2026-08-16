import { useMemo, useState } from "react";
import { useContacts, useResolveContactKeys, useSaveContact, useAddToContacts } from "../../queries/contacts";
import { useConversations } from "../../queries/messaging";
import { useAcceptIncomingInvite, useAliasContacts, useDeclineIncomingInvite, useIncomingInvites } from "../../queries/alias";
import { useChatUiStore } from "../../stores/chatUiStore";
import { avatarColor, formatWalletAddress, initials, isValidAlgorandAddress, truncateWalletAddress } from "../../lib/format";
import { username as usernameApi } from "../../lib/tauri";
import "./chat.css";

type Tab = "chats" | "alias" | "spam";

/**
 * Left panel: a chat list — one row per conversation, newest first, exactly
 * mirroring mobile's `getConversations`/`ChatPreview` (message-driven, not a
 * contacts-cache/address-book listing). This matters: a wallet that has
 * messaged you shows up here immediately, whether or not you've ever
 * manually added them as a contact — a purely `contacts_cache`-driven list
 * (the earlier version of this component) would silently hide any message
 * from someone you hadn't already added, which is exactly the mobile-vs-
 * desktop parity bug this was rewritten to fix (2026-08-07).
 *
 * The search box doubles as "start a new chat": if nothing in the
 * conversation list or the local contact cache matches, it offers to
 * resolve the typed wallet address or username and start a fresh thread.
 *
 * Chats/Spam split the same conversation list by one predicate —
 * `isBlocked` (from `contacts_cache.is_blocked`, `false` if no cached row
 * exists). Mirrors `message_repository.dart`'s "the sole Spam-tab
 * predicate" comment on the same query. A third Alias tab (mirrors
 * mobile's `ChatsTab.{chats, aliasChats, spam}`) lists established alias
 * contacts plus any invites delivered via a regular DM still awaiting an
 * Accept/Decline decision (Phase 7h) — entirely separate data, not part of
 * the message-driven conversation list above.
 */
export function ContactsSidebar() {
  const collapsed = useChatUiStore((s) => s.sidebarCollapsed);
  const selectedWallet = useChatUiStore((s) => s.selectedWallet);
  const selectedAliasContactId = useChatUiStore((s) => s.selectedAliasContactId);
  const selectContact = useChatUiStore((s) => s.selectContact);
  const selectAliasContact = useChatUiStore((s) => s.selectAliasContact);
  const toggleSidebar = useChatUiStore((s) => s.toggleSidebar);
  const openSettings = useChatUiStore((s) => s.openSettings);
  const openContactProfile = useChatUiStore((s) => s.openContactProfile);
  const openAlias = useChatUiStore((s) => s.openAlias);

  const { data: contacts = [] } = useContacts();
  const { data: conversations = [] } = useConversations();
  const { data: aliasContacts = [] } = useAliasContacts();
  const { data: incomingInvites = [] } = useIncomingInvites();
  const saveContact = useSaveContact();
  const resolveKeys = useResolveContactKeys();
  const addToContacts = useAddToContacts();
  const acceptIncomingInvite = useAcceptIncomingInvite();
  const declineIncomingInvite = useDeclineIncomingInvite();

  const [tab, setTab] = useState<Tab>("chats");
  const [query, setQuery] = useState("");
  const [newChatBusy, setNewChatBusy] = useState(false);
  const [newChatError, setNewChatError] = useState<string | null>(null);

  // `contacts_cache` (via `useContacts`) is the proven-correct source for a
  // known contact's name — it's what the sidebar used exclusively before
  // this file's conversation-driven rewrite. Overriding the conversation
  // preview's own `contactUsername` (which the backend already tries to
  // fill from the same cache, joined in SQL) with this client-side lookup
  // is deliberate redundancy: it keeps the display name correct even if
  // the two ever disagree, without needing another live round-trip.
  const usernameByWallet = useMemo(() => {
    const map = new Map<string, string>();
    for (const c of contacts) {
      if (c.username) map.set(c.walletAddress, c.username);
    }
    return map;
  }, [contacts]);

  const filteredConversations = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return conversations;
    return conversations.filter((c) => c.contactUsername?.toLowerCase().includes(q) || c.contactWallet.toLowerCase().includes(q));
  }, [conversations, query]);

  const chatsConversations = useMemo(() => filteredConversations.filter((c) => !c.isBlocked), [filteredConversations]);
  const spamConversations = useMemo(() => filteredConversations.filter((c) => c.isBlocked), [filteredConversations]);
  // The alias tab renders its own data (aliasContacts/incomingInvites), not
  // the message-driven conversation list — empty array here, never used by
  // that branch, just keeps this a total function of `tab`.
  const visibleConversations = tab === "chats" ? chatsConversations : tab === "spam" ? spamConversations : [];

  // "Start Chat" should only offer to create a new thread when the query
  // matches literally nothing already reachable — neither an existing
  // conversation (someone who's already messaged you, cache row or not)
  // nor a manually-cached contact with no messages yet.
  const matchesExistingContact = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return false;
    return contacts.some((c) => c.username?.toLowerCase().includes(q) || c.walletAddress.toLowerCase().includes(q));
  }, [contacts, query]);
  const showStartChat = query.trim().length > 0 && filteredConversations.length === 0 && !matchesExistingContact;

  async function startNewChat() {
    const trimmed = query.trim();
    if (!trimmed) return;
    setNewChatBusy(true);
    setNewChatError(null);
    try {
      let walletAddress = trimmed.toUpperCase();
      if (!isValidAlgorandAddress(walletAddress)) {
        walletAddress = await usernameApi.resolve(trimmed);
      }
      const keys = await resolveKeys.mutateAsync(walletAddress);
      if (!keys.encryptionPubkey || !keys.scanPubkey) {
        throw new Error("Could not resolve this contact's keys.");
      }
      await saveContact.mutateAsync({
        walletAddress,
        username: keys.username,
        encryptionPubkey: keys.encryptionPubkey,
        scanPubkey: keys.scanPubkey,
        pqPublicKey: keys.pqPublicKey,
        pqPubkeyHash: null,
        createdAt: Math.floor(Date.now() / 1000),
        isContact: false, // ignored by save_contact — addToContacts below sets it
        isBlocked: false,
      });
      // Deliberately starting a chat is exactly the "manually added" case —
      // flag it so it lands in Chats, not Spam.
      await addToContacts.mutateAsync(walletAddress);
      selectContact(walletAddress);
      setQuery("");
    } catch (e) {
      setNewChatError(String(e));
    } finally {
      setNewChatBusy(false);
    }
  }

  return (
    <aside className={`sidebar ${collapsed ? "sidebar--collapsed" : ""}`}>
      <div className="sidebar__header">
        <h2 className="sidebar__title">Contacts</h2>
        <div className="sidebar__header-actions">
          <button className="sidebar__icon-btn" onClick={openAlias} aria-label="New alias chat">
            ⊞
          </button>
          <button className="sidebar__icon-btn" onClick={openSettings} aria-label="Settings">
            ⚙
          </button>
          <button className="sidebar__icon-btn" onClick={toggleSidebar} aria-label="Collapse sidebar">
            ⟨
          </button>
        </div>
      </div>

      <div className="sidebar__tabs">
        <button className={`sidebar__tab ${tab === "chats" ? "sidebar__tab--active" : ""}`} onClick={() => setTab("chats")}>
          Chats
        </button>
        <button className={`sidebar__tab ${tab === "alias" ? "sidebar__tab--active" : ""}`} onClick={() => setTab("alias")}>
          Alias{incomingInvites.length > 0 ? ` (${incomingInvites.length})` : ""}
        </button>
        <button className={`sidebar__tab ${tab === "spam" ? "sidebar__tab--active" : ""}`} onClick={() => setTab("spam")}>
          Spam{spamConversations.length > 0 ? ` (${spamConversations.length})` : ""}
        </button>
      </div>

      <div className="sidebar__search-row">
        <input
          className="sidebar__search-input"
          placeholder="Search, or paste an address / username"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && showStartChat) startNewChat();
          }}
        />
      </div>

      {showStartChat && (
        <div className="sidebar__new-chat">
          <button onClick={startNewChat} disabled={newChatBusy}>
            {newChatBusy ? "Resolving…" : "Start Chat"}
          </button>
        </div>
      )}
      {newChatError && <p className="sidebar__new-chat-hint">{newChatError}</p>}

      <div className="sidebar__list">
        {tab !== "alias" && visibleConversations.length === 0 && !showStartChat && (
          <p className="sidebar__empty">
            {tab === "chats" ? "No chats yet — search a wallet address or username to start one." : "No spam."}
          </p>
        )}
        {tab === "alias" && aliasContacts.length === 0 && incomingInvites.length === 0 && (
          <p className="sidebar__empty">No alias chats yet — start one from a contact's info screen.</p>
        )}
        {tab === "alias" && incomingInvites.length > 0 && (
          <div>
            <div className="sidebar__group-label">Invitations</div>
            {incomingInvites.map((inv) => (
              <div key={inv.inviteRef} className="sidebar__row">
                <span className="sidebar__avatar sidebar__avatar--dm">A</span>
                <span className="sidebar__row-text">
                  <span className="sidebar__row-name">{inv.peerUsername ?? formatWalletAddress(inv.peerWallet)}</span>
                  <span className="sidebar__row-address">wants to start an alias chat</span>
                </span>
                <button
                  className="btn btn--text"
                  disabled={acceptIncomingInvite.isPending}
                  onClick={() => acceptIncomingInvite.mutate({ inviteRef: inv.inviteRef })}
                >
                  Accept
                </button>
                <button
                  className="btn btn--text"
                  disabled={declineIncomingInvite.isPending}
                  onClick={() => declineIncomingInvite.mutate(inv.inviteRef)}
                >
                  Decline
                </button>
              </div>
            ))}
          </div>
        )}
        {tab === "alias" && aliasContacts.length > 0 && (
          <div>
            <div className="sidebar__group-label">Established</div>
            {aliasContacts.map((c) => (
              <div key={c.contactId} className={`sidebar__row ${selectedAliasContactId === c.contactId ? "sidebar__row--selected" : ""}`}>
                <button className="sidebar__row-main" onClick={() => selectAliasContact(c.contactId)}>
                  <span className="sidebar__avatar sidebar__avatar--dm">A</span>
                  <span className="sidebar__row-text">
                    <span className="sidebar__row-name">{c.label ?? "Untitled alias chat"}</span>
                    <span className="sidebar__row-address">{c.contactId.slice(0, 12)}…</span>
                  </span>
                </button>
              </div>
            ))}
          </div>
        )}
        {visibleConversations.map((c) => {
          const displayName = usernameByWallet.get(c.contactWallet) ?? c.contactUsername ?? null;
          return (
          <div key={c.contactWallet} className={`sidebar__row ${selectedWallet === c.contactWallet ? "sidebar__row--selected" : ""}`}>
            <button className="sidebar__row-main" onClick={() => selectContact(c.contactWallet)}>
              {displayName ? (
                <span className="sidebar__avatar" style={{ background: avatarColor(c.contactWallet) }}>
                  {initials(displayName)}
                </span>
              ) : (
                <span className="sidebar__avatar sidebar__avatar--dm">DM</span>
              )}
              <span className="sidebar__row-text">
                <span className="sidebar__row-name">{displayName ?? formatWalletAddress(c.contactWallet)}</span>
                <span className="sidebar__row-address">
                  {displayName ? truncateWalletAddress(c.contactWallet) : c.lastMessagePreview}
                </span>
              </span>
            </button>
            <button className="sidebar__row-info-btn" onClick={() => openContactProfile(c.contactWallet)} aria-label="Contact info">
              ⓘ
            </button>
          </div>
          );
        })}
      </div>
    </aside>
  );
}
