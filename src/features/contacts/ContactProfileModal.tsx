import { useChatUiStore } from "../../stores/chatUiStore";
import { ContactProfile } from "./ContactProfile";
import "./contactProfile.css";

/**
 * Modal wrapper around `ContactProfile`, reached only from the chat
 * header's avatar/name (2026-08-24) — floats on top of whatever's already
 * showing instead of replacing the right panel, unlike the Contacts
 * address book's entry point (`ContactsListSidebar` → `viewingContactWallet`,
 * full-panel, unchanged). Renders nothing when `viewingContactWalletModal`
 * is null. Clicking the backdrop closes it, same as `NavDrawer`'s overlay.
 */
export function ContactProfileModal() {
  const walletAddress = useChatUiStore((s) => s.viewingContactWalletModal);
  const close = useChatUiStore((s) => s.closeContactProfileModal);

  if (!walletAddress) return null;

  return (
    <div className="contact-profile-modal-backdrop" onClick={close}>
      <div className="contact-profile-modal" onClick={(e) => e.stopPropagation()}>
        <ContactProfile walletAddress={walletAddress} onClose={close} />
      </div>
    </div>
  );
}
