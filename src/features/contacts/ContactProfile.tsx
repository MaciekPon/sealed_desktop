import { useState } from "react";
import { useChatUiStore } from "../../stores/chatUiStore";
import {
  useContact,
  useAddToContacts,
  useBlockContact,
  useDeleteContact,
  useRemoveFromContacts,
  useResolvedUsername,
  useUnblockContact,
} from "../../queries/contacts";
import {
  useAcceptIncomingInvite,
  useAliasContacts,
  useCreateInviteForContact,
  useIncomingInvites,
  usePendingInvites,
} from "../../queries/alias";
import { avatarColor, formatWalletAddress, initials } from "../../lib/format";
import { QrCode } from "../alias/QrCode";
import { IconLock } from "../settings/icons";
import {
  IconCheck,
  IconChatBubble,
  IconLockOpen,
  IconPhone,
  IconUserCheck,
  IconUserMinus,
  IconUserPlus,
} from "./icons";
import "./contactProfile.css";

/**
 * Reached from a contact row's info button in `ContactsSidebar`, restyled
 * 2026-08-18 to match a supplied design mockup — avatar/QR/bio/action-row
 * layout. "Bio" has no backend (no such field exists anywhere) so it's
 * rendered as a disabled placeholder, same treatment as Settings' rows for
 * features that don't exist yet. Calling (the phone icon) is likewise
 * disabled — there is no voice-call feature in this app at all.
 */
export function ContactProfile() {
  const walletAddress = useChatUiStore((s) => s.viewingContactWallet);
  const closeContactProfile = useChatUiStore((s) => s.closeContactProfile);
  const clearSelection = useChatUiStore((s) => s.clearSelection);
  const selectContact = useChatUiStore((s) => s.selectContact);
  const selectAliasContact = useChatUiStore((s) => s.selectAliasContact);

  const { data: contact } = useContact(walletAddress);
  // A username claim is on-chain, global state — resolve it even for a
  // wallet that was never manually added, same reasoning as
  // `ContactsSidebar`'s `ConversationRow`.
  const { data: resolvedUsername } = useResolvedUsername(
    walletAddress ?? "",
    walletAddress !== null && !contact?.username,
  );
  const addToContacts = useAddToContacts();
  const removeFromContacts = useRemoveFromContacts();
  const blockContact = useBlockContact();
  const unblockContact = useUnblockContact();
  const deleteContact = useDeleteContact();

  // Contact-initiated alias chat (Phase 7h) — mirrors `contact_profile.dart`'s
  // "Create Alias Chat" action: delivers the invite directly to this
  // already-known wallet over the regular messaging channel, no QR needed.
  const { data: aliasContacts = [] } = useAliasContacts();
  const { data: pendingInvites = [] } = usePendingInvites();
  const { data: incomingInvites = [] } = useIncomingInvites();
  const createInviteForContact = useCreateInviteForContact();
  const acceptIncomingInvite = useAcceptIncomingInvite();
  const [aliasError, setAliasError] = useState<string | null>(null);
  const [addressCopied, setAddressCopied] = useState(false);

  if (!walletAddress) return null;

  const displayName = contact?.username ?? resolvedUsername ?? null;
  const busy =
    addToContacts.isPending ||
    removeFromContacts.isPending ||
    blockContact.isPending ||
    unblockContact.isPending ||
    deleteContact.isPending;

  const existingAliasContact = aliasContacts.find(
    (c) => c.peerWallet === walletAddress,
  );
  const myPendingAliasInvite = pendingInvites.find(
    (p) => p.peerWallet === walletAddress && !p.dismissed,
  );
  const theirIncomingAliasInvite = incomingInvites.find(
    (i) => i.peerWallet === walletAddress,
  );
  const aliasBusy =
    acceptIncomingInvite.isPending || createInviteForContact.isPending;
  const aliasLabel = existingAliasContact
    ? "Open chat"
    : myPendingAliasInvite
      ? "Pending…"
      : theirIncomingAliasInvite
        ? acceptIncomingInvite.isPending
          ? "Accepting…"
          : "Accept"
        : createInviteForContact.isPending
          ? "Sending…"
          : "Start Chat";

  function openChat() {
    selectContact(walletAddress as string);
    closeContactProfile();
  }

  async function handleDelete() {
    await deleteContact.mutateAsync(walletAddress as string);
    clearSelection();
    closeContactProfile();
  }

  async function handleCopyAddress() {
    await navigator.clipboard.writeText(walletAddress as string);
    setAddressCopied(true);
    setTimeout(() => setAddressCopied(false), 1500);
  }

  async function handleAliasAction() {
    setAliasError(null);
    try {
      if (existingAliasContact) {
        selectAliasContact(existingAliasContact.contactId);
        closeContactProfile();
      } else if (theirIncomingAliasInvite) {
        const newContact = await acceptIncomingInvite.mutateAsync({
          inviteRef: theirIncomingAliasInvite.inviteRef,
        });
        selectAliasContact(newContact.contactId);
        closeContactProfile();
      } else if (!myPendingAliasInvite) {
        await createInviteForContact.mutateAsync({
          recipientWallet: walletAddress as string,
        });
      }
    } catch (e) {
      setAliasError(String(e));
    }
  }

  return (
    <div className="contact-profile">
      <div className="contact-profile__header">
        <button
          className="sidebar__icon-btn"
          onClick={closeContactProfile}
          aria-label="Back"
        >
          ←
        </button>
        <div className="contact-profile__header-inner">
          <h2 className="contact-profile__header-name">
            {displayName ?? formatWalletAddress(walletAddress)}
          </h2>
          <div className="contact-profile__header-actions">
            <button
              className="contact-profile__icon-btn"
              onClick={openChat}
              aria-label="Open chat"
            >
              <IconChatBubble />
            </button>
            <button
              className="contact-profile__icon-btn"
              disabled
              title="Coming soon"
              aria-label="Call"
            >
              <IconPhone />
            </button>
            <button
              className="contact-profile__alias-pill"
              disabled={!!myPendingAliasInvite || aliasBusy}
              onClick={handleAliasAction}
            >
              <IconChatBubble /> Alias Chat
            </button>
          </div>
        </div>
      </div>

      <div className="contact-profile__body">
        {contact?.isBlocked ? (
          <span className="contact-profile__badge contact-profile__badge--danger">
            Blocked
          </span>
        ) : (
          contact?.isContact && (
            <span className="contact-profile__badge">
              <IconUserCheck /> In contacts
            </span>
          )
        )}

        {displayName ? (
          <span
            className="contact-profile__avatar"
            style={{ background: avatarColor(walletAddress) }}
          >
            {initials(displayName)}
          </span>
        ) : (
          <span className="contact-profile__avatar contact-profile__avatar--dm">
            DM
          </span>
        )}

        <div className="contact-profile__section">
          <div className="contact-profile__section-label">Wallet address</div>
          <div className="contact-profile__qr-card">
            <QrCode value={walletAddress} size={160} />
          </div>
          <div className="contact-profile__address-row">
            <span className="contact-profile__address-text">
              {walletAddress}
            </span>
            <button
              className="contact-profile__copy-btn"
              onClick={handleCopyAddress}
              aria-label="Copy address"
              title={addressCopied ? "Copied!" : "Copy"}
            >
              <IconCheck />
            </button>
          </div>
        </div>

        <div className="contact-profile__section">
          <div className="contact-profile__section-label">Bio</div>
          <p className="contact-profile__bio contact-profile__bio--disabled">
            Not available yet.
          </p>
        </div>

        <div className="contact-profile__section">
          <div className="contact-profile__section-label">Contact</div>

          {contact?.isContact ? (
            <ProfileActionRow
              icon={<IconUserMinus />}
              label="Remove From Contacts"
              tone="danger"
              buttonLabel="Remove"
              disabled={busy}
              onClick={() => removeFromContacts.mutate(walletAddress as string)}
            />
          ) : (
            <ProfileActionRow
              icon={<IconUserPlus />}
              label="Add to Contacts"
              tone="neutral"
              buttonLabel="Add"
              disabled={busy || !!contact?.isBlocked}
              onClick={() => addToContacts.mutate(walletAddress as string)}
            />
          )}

          {contact?.isBlocked ? (
            <ProfileActionRow
              icon={<IconLockOpen />}
              label="Unblock Wallet"
              tone="neutral"
              buttonLabel="Remove from Spam"
              disabled={busy}
              onClick={() => unblockContact.mutate(walletAddress as string)}
            />
          ) : (
            <ProfileActionRow
              icon={<IconLock />}
              label="Block Wallet"
              tone="danger"
              buttonLabel="Block"
              disabled={busy}
              onClick={() => blockContact.mutate(walletAddress as string)}
            />
          )}

          <ProfileActionRow
            icon={<IconChatBubble />}
            label="Create Alias Chat"
            tone="accent"
            buttonLabel={aliasLabel}
            disabled={!!myPendingAliasInvite || aliasBusy}
            onClick={handleAliasAction}
          />
          {aliasError && <p className="pin-pad__error">{aliasError}</p>}

          <button
            className="btn btn--text contact-profile__delete-link"
            disabled={busy}
            onClick={handleDelete}
          >
            Delete contact (clears cached keys/name too)
          </button>
        </div>
      </div>
    </div>
  );
}

function ProfileActionRow({
  icon,
  label,
  tone,
  buttonLabel,
  disabled,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  tone: "danger" | "neutral" | "accent";
  buttonLabel: string;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <div className={`contact-profile__row contact-profile__row--${tone}`}>
      <span className="contact-profile__row-icon">{icon}</span>
      <span className="contact-profile__row-label">{label}</span>
      <button
        className={`contact-profile__row-btn contact-profile__row-btn--${tone}`}
        disabled={disabled}
        onClick={onClick}
      >
        {buttonLabel}
      </button>
    </div>
  );
}
