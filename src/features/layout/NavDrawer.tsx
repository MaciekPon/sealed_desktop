import { useChatUiStore } from "../../stores/chatUiStore";
import { useSessionStore } from "../../stores/sessionStore";
import { useResolvedUsername } from "../../queries/contacts";
import { useCredits } from "../../queries/credits";
import { avatarColor, initials, truncateWalletAddress } from "../../lib/format";
import { SettingsScreen } from "../settings/SettingsScreen";
import "./layout.css";

const navIconBase = {
  width: 18,
  height: 18,
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

function IconChats() {
  return (
    <svg {...navIconBase}>
      <path d="M3 4.5h14v9H8l-3.5 3v-3H3Z" />
    </svg>
  );
}

function IconContacts() {
  return (
    <svg {...navIconBase}>
      <circle cx="10" cy="7" r="3" />
      <path d="M4 17c0-3 2.7-5 6-5s6 2 6 5" />
    </svg>
  );
}

function IconFiles() {
  return (
    <svg {...navIconBase}>
      <path d="M5 3.5h6l4 4v9a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1v-12a1 1 0 0 1 1-1Z" />
      <path d="M11 3.5v4h4" />
    </svg>
  );
}

function IconSettings() {
  return (
    <svg {...navIconBase}>
      <circle cx="10" cy="10" r="2.6" />
      <path d="M10 3.5v1.7M10 14.8v1.7M16.5 10h-1.7M5.2 10H3.5M14.6 5.4l-1.2 1.2M6.6 13.4l-1.2 1.2M14.6 14.6l-1.2-1.2M6.6 6.6 5.4 5.4" />
    </svg>
  );
}

/**
 * Hamburger-menu navigation drawer, matching the supplied design mockup —
 * an overlay (dimmed backdrop + slide-out panel), not a `screen` value,
 * since which screen is showing underneath doesn't matter while it's open.
 * "Files" has no backend at all yet (no file-sharing feature exists
 * anywhere in this app) so it's rendered disabled, same treatment as the
 * Settings screen's not-yet-implemented rows.
 *
 * Only Settings renders *inside* this same panel (`navDrawerMode`) rather
 * than navigating the whole app away — per the mockup, the drawer stays
 * open and its content swaps to Settings. Contacts is a normal full-width
 * `screen` (matches the profile/address-book mockups, which are clearly
 * full-content-area views, not drawer panels) — picking it closes the
 * drawer and switches screens, same as Chats.
 */
export function NavDrawer() {
  const open = useChatUiStore((s) => s.navDrawerOpen);
  const mode = useChatUiStore((s) => s.navDrawerMode);
  const close = useChatUiStore((s) => s.closeNavDrawer);
  const setMode = useChatUiStore((s) => s.setNavDrawerMode);
  const screen = useChatUiStore((s) => s.screen);
  const clearSelection = useChatUiStore((s) => s.clearSelection);
  const openContactsList = useChatUiStore((s) => s.openContactsList);

  const account = useSessionStore((s) => s.account);
  const { data: username } = useResolvedUsername(account?.walletAddress ?? "", !!account);
  const { data: credits } = useCredits();

  if (!open) return null;

  function goToChats() {
    clearSelection();
    close();
  }

  function goToContactsList() {
    openContactsList();
    close();
  }

  if (mode === "settings") {
    return (
      <div className="nav-drawer-backdrop" onClick={close}>
        <div className="nav-drawer nav-drawer--wide" onClick={(e) => e.stopPropagation()}>
          <SettingsScreen onClose={() => setMode("nav")} />
        </div>
      </div>
    );
  }

  return (
    <div className="nav-drawer-backdrop" onClick={close}>
      <div className="nav-drawer" onClick={(e) => e.stopPropagation()}>
        <div className="nav-drawer__profile">
          {username ? (
            <span className="nav-drawer__avatar" style={{ background: avatarColor(account?.walletAddress ?? "") }}>
              {initials(username)}
            </span>
          ) : (
            <span className="nav-drawer__avatar nav-drawer__avatar--dm">DM</span>
          )}
          <p className="nav-drawer__name">{username ?? (account ? truncateWalletAddress(account.walletAddress) : "—")}</p>
          <button className="nav-drawer__edit-profile" onClick={() => setMode("settings")}>
            Edit profile
          </button>
        </div>

        <nav className="nav-drawer__nav">
          <button className={`nav-drawer__item ${screen === "chat" ? "nav-drawer__item--active" : ""}`} onClick={goToChats}>
            <span className="nav-drawer__item-icon">
              <IconChats />
            </span>
            Chats
          </button>
          <button className={`nav-drawer__item ${screen === "contactsList" ? "nav-drawer__item--active" : ""}`} onClick={goToContactsList}>
            <span className="nav-drawer__item-icon">
              <IconContacts />
            </span>
            Contacts
          </button>
          <button className="nav-drawer__item nav-drawer__item--disabled" disabled title="Coming soon">
            <span className="nav-drawer__item-icon">
              <IconFiles />
            </span>
            Files
          </button>
          <button className="nav-drawer__item" onClick={() => setMode("settings")}>
            <span className="nav-drawer__item-icon">
              <IconSettings />
            </span>
            Settings
          </button>
        </nav>

        <div className="nav-drawer__footer">
          <div className="nav-drawer__footer-row">
            <span>Sealed Credits</span>
            <span className="nav-drawer__credits-pill">{credits ?? "—"}</span>
          </div>
          <p className="nav-drawer__footer-hint">enough to send {credits ?? 0} Messages</p>
        </div>
      </div>
    </div>
  );
}
