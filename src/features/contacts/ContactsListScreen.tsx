import { useChatUiStore } from "../../stores/chatUiStore";
import { useContacts } from "../../queries/contacts";
import { avatarColor, groupContacts, initials, truncateWalletAddress } from "../../lib/format";
import "./contactsList.css";

/**
 * A-Z address-book view, reached from the nav drawer's "Contacts" item —
 * distinct from `ContactsSidebar`'s message-driven Chats list (this one
 * lists everyone in `contacts_cache`, whether or not you've exchanged a
 * message with them yet). Revives `lib/format.ts`'s `groupContacts` helper,
 * which had been dead code since the sidebar's 2026-08-07 rewrite to a
 * conversation-driven list.
 *
 * A normal full-width `screen` (unlike `SettingsScreen`, which renders
 * inside the nav drawer) — matches the profile mockup, which is clearly a
 * full-content-area view, not a narrow drawer panel.
 */
export function ContactsListScreen() {
  const goToChats = useChatUiStore((s) => s.goToChats);
  const openContactProfile = useChatUiStore((s) => s.openContactProfile);
  const { data: contacts = [] } = useContacts();
  const groups = groupContacts(contacts);

  return (
    <div className="contacts-list-screen">
      <div className="settings-screen__header">
        <button className="sidebar__icon-btn" onClick={goToChats} aria-label="Back">
          ←
        </button>
        <h2 className="settings-screen__title">Contacts</h2>
      </div>
      <div className="contacts-list-screen__body">
        {groups.length === 0 && <p className="sidebar__empty">No contacts yet.</p>}
        {groups.map((group) => (
          <div key={group.label}>
            <div className="sidebar__group-label">{group.label}</div>
            {group.contacts.map((c) => (
              <button key={c.walletAddress} className="contacts-list-screen__row" onClick={() => openContactProfile(c.walletAddress)}>
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
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
