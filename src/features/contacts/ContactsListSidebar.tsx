import { useMemo, useState } from "react";
import { useChatUiStore } from "../../stores/chatUiStore";
import { useContacts } from "../../queries/contacts";
import { avatarColor, groupContacts, initials, truncateWalletAddress } from "../../lib/format";
import "../chat/chat.css";

/**
 * Left-panel replacement for `ContactsSidebar` while browsing the A-Z
 * address book (`leftPanel === "contacts"` in `chatUiStore`) — same header
 * chrome (hamburger/brand/search) so switching between Chats and Contacts
 * doesn't jump the layout, but lists everyone in `contacts_cache` (whether
 * or not you've exchanged a message with them) instead of the
 * message-driven conversation list, and clicking a row opens that
 * contact's profile in the right panel instead of a chat.
 *
 * Replaces the previous `ContactsListScreen`, which rendered as its own
 * full-width panel *next to* the still-visible chat sidebar (2026-08-23
 * mockup: the right panel should swap to the profile, not add a third
 * column) — `MainLayout` now renders this instead of `ContactsSidebar`
 * for the left panel, and `ContactProfile` instead of `ChatWindow` for the
 * right, whenever a contact is being viewed.
 */
export function ContactsListSidebar() {
  const viewingContactWallet = useChatUiStore((s) => s.viewingContactWallet);
  const openContactProfile = useChatUiStore((s) => s.openContactProfile);
  const openNavDrawer = useChatUiStore((s) => s.openNavDrawer);

  const { data: contacts = [] } = useContacts();
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return contacts;
    return contacts.filter((c) => c.username?.toLowerCase().includes(q) || c.walletAddress.toLowerCase().includes(q));
  }, [contacts, query]);
  const groups = groupContacts(filtered);

  return (
    <aside className="sidebar">
      <div className="sidebar__header">
        <div className="sidebar__brand">
          <button className="hamburger-btn" onClick={openNavDrawer} aria-label="Menu">
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
        />
      </div>

      <div className="sidebar__list">
        <div className="sidebar__section">
          <div className="sidebar__section-header">
            <span className="sidebar__section-title">Contacts</span>
          </div>
          <div className="sidebar__section-body">
            {groups.length === 0 ? (
              <p className="sidebar__empty">No contacts yet.</p>
            ) : (
              groups.map((group) => (
                <div key={group.label}>
                  <div className="sidebar__group-label">{group.label}</div>
                  {group.contacts.map((c) => (
                    <div
                      key={c.walletAddress}
                      className={`sidebar__row ${viewingContactWallet === c.walletAddress ? "sidebar__row--selected" : ""}`}
                    >
                      <button className="sidebar__row-main" onClick={() => openContactProfile(c.walletAddress)}>
                        {c.username ? (
                          <span className="sidebar__avatar" style={{ background: avatarColor(c.walletAddress) }}>
                            {initials(c.username)}
                          </span>
                        ) : (
                          <span className="sidebar__avatar sidebar__avatar--dm">DM</span>
                        )}
                        <span className="sidebar__row-text">
                          <span className="sidebar__row-name">{c.username ?? truncateWalletAddress(c.walletAddress)}</span>
                        </span>
                      </button>
                    </div>
                  ))}
                </div>
              ))
            )}
          </div>
        </div>
      </div>
    </aside>
  );
}
