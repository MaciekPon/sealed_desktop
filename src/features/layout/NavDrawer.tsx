import { useEffect, useState } from "react";
import { useChatUiStore } from "../../stores/chatUiStore";
import { useSessionStore } from "../../stores/sessionStore";
import { useResolvedUsername } from "../../queries/contacts";
import { useCredits } from "../../queries/credits";
import { avatarColor, initials, truncateWalletAddress } from "../../lib/format";
import { SettingsScreen } from "../settings/SettingsScreen";
import { IconChats, IconContacts, IconFiles, IconSettings } from "./icons";
import "./layout.css";

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
 * open and its content swaps to Settings. Chats/Contacts instead swap
 * `MainLayout`'s left panel (`chatUiStore`'s `leftPanel`) — picking either
 * closes the drawer.
 */
export function NavDrawer() {
  const open = useChatUiStore((s) => s.navDrawerOpen);
  const mode = useChatUiStore((s) => s.navDrawerMode);
  const close = useChatUiStore((s) => s.closeNavDrawer);
  const setMode = useChatUiStore((s) => s.setNavDrawerMode);
  const leftPanel = useChatUiStore((s) => s.leftPanel);
  const storeGoToChats = useChatUiStore((s) => s.goToChats);
  const openContactsList = useChatUiStore((s) => s.openContactsList);

  const account = useSessionStore((s) => s.account);
  const { data: username } = useResolvedUsername(account?.walletAddress ?? "", !!account);
  const { data: credits } = useCredits();

  // `open` flipping to `false` used to unmount this immediately — no exit
  // animation was possible. Keep it mounted for one more animation cycle
  // (`closing`), playing the reverse slide-out, before actually unmounting.
  // Duration must match the longer of `.nav-drawer`'s/`.nav-drawer-backdrop`'s
  // CSS animation durations (250ms) in `layout.css`.
  const [rendered, setRendered] = useState(open);
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    if (open) {
      setRendered(true);
      setClosing(false);
      return;
    }
    if (!rendered) return;
    setClosing(true);
    const timer = setTimeout(() => {
      setRendered(false);
      setClosing(false);
    }, 250);
    return () => clearTimeout(timer);
  }, [open, rendered]);

  if (!rendered) return null;

  function goToChats() {
    storeGoToChats();
    close();
  }

  function goToContactsList() {
    openContactsList();
    close();
  }

  if (mode === "settings") {
    return (
      <div className={`nav-drawer-backdrop ${closing ? "nav-drawer-backdrop--closing" : ""}`} onClick={close}>
        <div className={`nav-drawer nav-drawer--wide ${closing ? "nav-drawer--closing" : ""}`} onClick={(e) => e.stopPropagation()}>
          <SettingsScreen onClose={() => setMode("nav")} />
        </div>
      </div>
    );
  }

  return (
    <div className={`nav-drawer-backdrop ${closing ? "nav-drawer-backdrop--closing" : ""}`} onClick={close}>
      <div className={`nav-drawer ${closing ? "nav-drawer--closing" : ""}`} onClick={(e) => e.stopPropagation()}>
        <div className="nav-drawer__brand">
          <button className="hamburger-btn" onClick={close} aria-label="Close menu">
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
          <button className={`nav-drawer__item ${leftPanel === "chats" ? "nav-drawer__item--active" : ""}`} onClick={goToChats}>
            <span className="nav-drawer__item-icon">
              <IconChats />
            </span>
            Chats
          </button>
          <button className={`nav-drawer__item ${leftPanel === "contacts" ? "nav-drawer__item--active" : ""}`} onClick={goToContactsList}>
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
