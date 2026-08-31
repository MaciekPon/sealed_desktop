import { useState } from "react";
import { useChatUiStore } from "../../stores/chatUiStore";
import {
  useContact,
  useAddToContacts,
  useBlockContact,
  useDeleteContact,
  useRemoveFromContacts,
  useResolvedBio,
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
import { avatarColor, formatWalletAddress, formatWalletAddressGrouped, initials } from "../../lib/format";
import { QrCode } from "../alias/QrCode";
import { IconLock } from "../settings/icons";
import {
  IconCheck,
  IconCopy,
  IconCreateAliasChat,
  IconLockOpen,
  IconPhone,
  IconUserCheck,
  IconUserMinus,
  IconUserPlus,
} from "./icons";
import "./contactProfile.css";

interface ContactProfileProps {
  walletAddress?: string;
  onClose?: () => void;
}

/**
 * Reached from a contact row's info button in `ContactsSidebar`, restyled
 * 2026-08-18 to match a supplied design mockup — avatar/QR/bio/action-row
 * layout. Body restyled again 2026-08-23 to match a second (dark-theme)
 * mockup: the address row now shows the full address grouped into 4-char
 * chunks (`formatWalletAddressGrouped`) with an inline copy icon that swaps
 * to a checkmark once clicked, instead of a boxed copy button; each action
 * row (Add/Remove/Block/Unblock/Create Alias Chat) is now its own tinted
 * full-width card instead of a plain row with a divider line below it. The
 * "Wallet address"/"Bio"/"Contact" section labels and bio's card
 * background are unchanged from the first pass — an earlier attempt
 * removed them based on a mis-exported white-background screenshot where
 * those dark, semi-transparent elements had rendered invisible. The header
 * (back button, name, action icons) is unchanged — the mockup only covered
 * the body. "Bio" reads this contact's public on-chain bio via the same
 * lazy chain-resolve path as their username — `contact.bio` (already
 * cached in `contacts_cache`) first, falling back to a live resolve via
 * `useResolvedBio` for a contact never fully resolved before. Calling (the
 * phone icon) is still disabled — there is no voice-call feature in this
 * app at all.
 *
 * Renders two ways (2026-08-24): as the full right panel (reached from the
 * Contacts address book — no props, reads `viewingContactWallet`/
 * `closeContactProfile` from the store; no close button at all — the left
 * panel's contact list is always visible in this mode, so there's nothing
 * to "go back" to) or, when given `walletAddress`/`onClose` explicitly,
 * inside `ContactProfileModal`'s overlay (reached from the chat header's
 * avatar/name — "✕" close button, since a modal needs an explicit
 * dismiss). All internal navigation (open chat, delete, alias actions)
 * closes via whichever `close` applies, so both entry points behave
 * correctly without needing to know which one they are.
 */
export function ContactProfile({ walletAddress: walletAddressProp, onClose }: ContactProfileProps = {}) {
  const viewingContactWallet = useChatUiStore((s) => s.viewingContactWallet);
  const closeContactProfileStore = useChatUiStore((s) => s.closeContactProfile);
  const walletAddress = walletAddressProp ?? viewingContactWallet;
  const isModal = onClose !== undefined;
  const close = onClose ?? closeContactProfileStore;
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
  // Same reasoning as `resolvedUsername` above, plus this contact's public
  // on-chain bio — `useResolvedBio` shares `useResolvedUsername`'s query
  // key/network call, so this doesn't cost a second round trip.
  const { data: resolvedBio } = useResolvedBio(
    walletAddress ?? "",
    walletAddress !== null && !contact?.bio,
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
    close();
  }

  async function handleDelete() {
    await deleteContact.mutateAsync(walletAddress as string);
    clearSelection();
    close();
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
        close();
      } else if (theirIncomingAliasInvite) {
        const newContact = await acceptIncomingInvite.mutateAsync({
          inviteRef: theirIncomingAliasInvite.inviteRef,
        });
        selectAliasContact(newContact.contactId);
        close();
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
        {isModal && (
          <button className="sidebar__icon-btn" onClick={close} aria-label="Close">
            ✕
          </button>
        )}
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
              <IconCreateAliasChat />
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
              <IconCreateAliasChat /> Alias Chat
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
          <div className="contact-profile__wallet-card">
            <div className="contact-profile__qr-frame">
              <QrCode value={walletAddress} size={160} />
            </div>
            <div className="contact-profile__address-input">
              <span className="contact-profile__address-text">
                {formatWalletAddressGrouped(walletAddress)}
              </span>
              <button
                className="contact-profile__address-copy-btn"
                onClick={handleCopyAddress}
                aria-label="Copy address"
                title={addressCopied ? "Copied!" : "Copy"}
              >
                {addressCopied ? <IconCheck /> : <IconCopy />}
              </button>
            </div>
          </div>
        </div>

        <div className="contact-profile__section">
          <div className="contact-profile__section-label">Bio</div>
          {(() => {
            const bio = contact?.bio ?? resolvedBio ?? null;
            return bio ? (
              <p className="contact-profile__bio">{bio}</p>
            ) : (
              <p className="contact-profile__bio contact-profile__bio--disabled">
                No bio yet.
              </p>
            );
          })()}
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
            icon={<IconCreateAliasChat />}
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
