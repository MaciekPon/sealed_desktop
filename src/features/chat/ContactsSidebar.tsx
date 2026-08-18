import { useMemo, useState } from "react";
import { useContacts, useResolveContactKeys, useResolvedUsername, useSaveContact, useAddToContacts } from "../../queries/contacts";
import { useConversations } from "../../queries/messaging";
import { useAliasContacts, useIncomingInvites } from "../../queries/alias";
import { useChatUiStore } from "../../stores/chatUiStore";
import { avatarColor, formatMessageTimestamp, formatWalletAddress, initials, isValidAlgorandAddress } from "../../lib/format";
import { username as usernameApi } from "../../lib/tauri";
import type { ConversationPreview } from "../../models";
import "./chat.css";

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
 * Restyled 2026-08-18 to match a supplied design mockup: Contacts/Alias
 * Chat/Spam are three independently collapsible sections in one scrollable
 * list (each with its own Hide/Show toggle) instead of the previous
 * mutually-exclusive tab-pill switcher. Contacts/Spam still split the same
 * conversation list by one predicate — `isBlocked` (from
 * `contacts_cache.is_blocked`, `false` if no cached row exists), mirroring
 * `message_repository.dart`'s "the sole Spam-tab predicate" comment on the
 * same query. Alias Chat lists established alias contacts plus any invites
 * delivered via a regular DM still awaiting an Accept/Decline decision
 * (Phase 7h) — entirely separate data, not part of the message-driven
 * conversation list above.
 *
 * The hamburger icon opens `NavDrawer` (Chats/Contacts/Files/Settings),
 * matching the mockup's separate navigation screen. The existing
 * icon-button row (settings / collapse) stays alongside it rather than
 * being replaced — same real functionality, just not part of the mockup's
 * visual scope. The QR/paste alias-chat handshake screen (Phase 7f) was
 * removed 2026-08-18 — starting an alias chat is now contact-profile-only
 * (Phase 7h's `createInviteForContact`, in `ContactProfile.tsx`); accepting
 * an already-arrived invite and chatting on an established alias contact
 * both still work exactly as before, unaffected by this removal.
 *
 * The header's separate Settings icon and the sidebar-collapse toggle were
 * also removed 2026-08-18 — Settings is reached through the hamburger menu
 * only now, and the sidebar no longer collapses at all (nothing sets that
 * state anymore, so `ChatWindow`'s dead re-expand affordance was removed
 * alongside it).
 */
export function ContactsSidebar() {
  const selectedWallet = useChatUiStore((s) => s.selectedWallet);
  const selectedAliasContactId = useChatUiStore((s) => s.selectedAliasContactId);
  const selectContact = useChatUiStore((s) => s.selectContact);
  const selectAliasContact = useChatUiStore((s) => s.selectAliasContact);
  const selectIncomingInvite = useChatUiStore((s) => s.selectIncomingInvite);
  const selectedIncomingInviteRef = useChatUiStore((s) => s.selectedIncomingInviteRef);
  const openContactProfile = useChatUiStore((s) => s.openContactProfile);
  const openNavDrawer = useChatUiStore((s) => s.openNavDrawer);

  const { data: contacts = [] } = useContacts();
  const { data: conversations = [] } = useConversations();
  const { data: aliasContacts = [] } = useAliasContacts();
  const { data: incomingInvites = [] } = useIncomingInvites();
  const saveContact = useSaveContact();
  const resolveKeys = useResolveContactKeys();
  const addToContacts = useAddToContacts();

  const [query, setQuery] = useState("");
  const [newChatBusy, setNewChatBusy] = useState(false);
  const [newChatError, setNewChatError] = useState<string | null>(null);

  // Matches the mockup's default state: Contacts open, the other two
  // collapsed until the user asks to see them.
  const [contactsExpanded, setContactsExpanded] = useState(true);
  const [aliasExpanded, setAliasExpanded] = useState(false);
  const [spamExpanded, setSpamExpanded] = useState(false);

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
    <aside className="sidebar">
      <div className="sidebar__header">
        <div className="sidebar__brand">
          <button className="sidebar__icon-btn" onClick={openNavDrawer} aria-label="Menu">
            ☰
          </button>
          <svg width="20" height="20" viewBox="0 0 120 120" fill="none" className="sidebar__brand-mark">
            <path
              d="M52.3082 0.245538C63.0572 -0.751089 73.8681 1.29913 83.5124 6.16081L83.7403 6.27666L83.7445 6.27868C112.098 20.7642 123.378 55.5972 108.978 84.0669C95.3052 111.099 62.7781 122.538 35.3099 111.277L10.4524 119.68L27.3341 89.7793H9.58543C4.35304 81.8393 1.13578 72.7082 0.247669 63.188L0.246328 63.1779C-2.66838 31.3724 20.6188 3.19537 52.3063 0.245538H52.3082ZM66.0651 57.4533L57.1243 73.6179H36.2749L27.3334 89.7813H71.8143C80.7025 89.7813 87.9077 82.5448 87.9084 73.6179C87.9084 64.6906 80.7032 57.4533 71.8143 57.4533H66.0651ZM43.4272 25.1257C34.5389 25.1258 27.3335 32.3624 27.3334 41.2895C27.3336 50.2163 34.539 57.4531 43.4272 57.4533H48.2305L57.1719 41.2895H78.9673L87.9084 25.1257H43.4272Z"
              fill="var(--color-primary)"
            />
          </svg>
          <span className="sidebar__brand-name">Sealed</span>
        </div>
      </div>

      <div className="sidebar__search-row">
        <input
          className="sidebar__search-input"
          placeholder="Search"
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
        <SidebarSection title="Contacts" expanded={contactsExpanded} onToggle={() => setContactsExpanded((v) => !v)}>
          {chatsConversations.length === 0 && !showStartChat ? (
            <p className="sidebar__empty">No chats yet — search a wallet address or username to start one.</p>
          ) : (
            chatsConversations.map((c) => (
              <ConversationRow
                key={c.contactWallet}
                conversation={c}
                cachedUsername={usernameByWallet.get(c.contactWallet)}
                isSelected={selectedWallet === c.contactWallet}
                onSelect={() => selectContact(c.contactWallet)}
                onOpenInfo={() => openContactProfile(c.contactWallet)}
              />
            ))
          )}
        </SidebarSection>

        <SidebarSection title="Alias Chat" expanded={aliasExpanded} onToggle={() => setAliasExpanded((v) => !v)}>
          {aliasContacts.length === 0 && incomingInvites.length === 0 ? (
            <p className="sidebar__empty">No alias chats yet — start one from a contact's info screen.</p>
          ) : (
            <>
              {incomingInvites.length > 0 && (
                <div>
                  <div className="sidebar__group-label">Invitations</div>
                  {incomingInvites.map((inv) => (
                    <div key={inv.inviteRef} className={`sidebar__row ${selectedIncomingInviteRef === inv.inviteRef ? "sidebar__row--selected" : ""}`}>
                      <button className="sidebar__row-main" onClick={() => selectIncomingInvite(inv.inviteRef)}>
                        <span className="sidebar__avatar sidebar__avatar--dm">A</span>
                        <span className="sidebar__row-text">
                          <span className="sidebar__row-name">{inv.peerUsername ?? formatWalletAddress(inv.peerWallet)}</span>
                        </span>
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {aliasContacts.length > 0 && (
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
            </>
          )}
        </SidebarSection>

        <SidebarSection title="Spam" expanded={spamExpanded} onToggle={() => setSpamExpanded((v) => !v)}>
          {spamConversations.length === 0 ? (
            <p className="sidebar__empty">No spam.</p>
          ) : (
            spamConversations.map((c) => (
              <ConversationRow
                key={c.contactWallet}
                conversation={c}
                cachedUsername={usernameByWallet.get(c.contactWallet)}
                isSelected={selectedWallet === c.contactWallet}
                onSelect={() => selectContact(c.contactWallet)}
                onOpenInfo={() => openContactProfile(c.contactWallet)}
              />
            ))
          )}
        </SidebarSection>
      </div>
    </aside>
  );
}

/** One independently collapsible section header + body, matching the mockup's "Hide"/"Show" toggle. */
function SidebarSection({
  title,
  expanded,
  onToggle,
  children,
}: {
  title: string;
  expanded: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="sidebar__section">
      <button className="sidebar__section-header" onClick={onToggle}>
        <span className="sidebar__section-title">{title}</span>
        <span className="sidebar__section-toggle">
          {expanded ? "Hide" : "Show"}
          <svg width="10" height="10" viewBox="0 0 20 20" fill="none" className={`sidebar__section-chevron ${expanded ? "sidebar__section-chevron--up" : ""}`}>
            <path d="M5 7.5 10 12.5 15 7.5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </span>
      </button>
      {expanded && <div className="sidebar__section-body">{children}</div>}
    </div>
  );
}

/**
 * A username claim is on-chain, global state — it should show up here
 * regardless of whether this wallet is in the local contacts cache, so a
 * chat with someone who has a username never falls back to their raw
 * wallet address just because they were never manually added. `cachedUsername`
 * (contacts cache) and `conversation.contactUsername` (per-message snapshot)
 * are both fast, already-loaded local sources tried first; `useResolvedUsername`
 * only fires a live chain/indexer lookup when neither has an answer.
 */
function ConversationRow({
  conversation,
  cachedUsername,
  isSelected,
  onSelect,
  onOpenInfo,
}: {
  conversation: ConversationPreview;
  cachedUsername: string | undefined;
  isSelected: boolean;
  onSelect: () => void;
  onOpenInfo: () => void;
}) {
  const localDisplayName = cachedUsername ?? conversation.contactUsername ?? null;
  const { data: resolvedUsername } = useResolvedUsername(conversation.contactWallet, localDisplayName === null);
  const displayName = localDisplayName ?? resolvedUsername ?? null;

  return (
    <div className={`sidebar__row ${isSelected ? "sidebar__row--selected" : ""}`}>
      <button className="sidebar__row-main" onClick={onSelect}>
        {displayName ? (
          <span className="sidebar__avatar" style={{ background: avatarColor(conversation.contactWallet) }}>
            {initials(displayName)}
          </span>
        ) : (
          <span className="sidebar__avatar sidebar__avatar--dm">DM</span>
        )}
        <span className="sidebar__row-text">
          <span className="sidebar__row-line">
            <span className="sidebar__row-name">{displayName ?? formatWalletAddress(conversation.contactWallet)}</span>
            <span className="sidebar__row-time">{formatMessageTimestamp(conversation.lastMessageTimestamp)}</span>
          </span>
          <span className="sidebar__row-line">
            <span className="sidebar__row-preview">{conversation.lastMessagePreview}</span>
            {conversation.unreadCount > 0 && <span className="sidebar__unread-badge">{conversation.unreadCount}</span>}
          </span>
        </span>
      </button>
      <button className="sidebar__row-info-btn" onClick={onOpenInfo} aria-label="Contact info">
        ⓘ
      </button>
    </div>
  );
}
