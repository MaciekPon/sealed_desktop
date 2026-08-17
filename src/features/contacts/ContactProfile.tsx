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
import { avatarColor, formatWalletAddress, initials, truncateWalletAddress } from "../../lib/format";
import "./contactProfile.css";

/** Block/unblock and add/remove-contact, reached from a contact row's info button in `ContactsSidebar`. */
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
  const { data: resolvedUsername } = useResolvedUsername(walletAddress ?? "", walletAddress !== null && !contact?.username);
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

  if (!walletAddress) return null;

  const busy =
    addToContacts.isPending || removeFromContacts.isPending || blockContact.isPending || unblockContact.isPending || deleteContact.isPending;

  const existingAliasContact = aliasContacts.find((c) => c.peerWallet === walletAddress);
  const myPendingAliasInvite = pendingInvites.find((p) => p.peerWallet === walletAddress && !p.dismissed);
  const theirIncomingAliasInvite = incomingInvites.find((i) => i.peerWallet === walletAddress);

  async function handleDelete() {
    if (!walletAddress) return;
    await deleteContact.mutateAsync(walletAddress);
    clearSelection();
    closeContactProfile();
  }

  async function handleStartAliasChat() {
    if (!walletAddress) return;
    setAliasError(null);
    try {
      await createInviteForContact.mutateAsync({ recipientWallet: walletAddress });
    } catch (e) {
      setAliasError(String(e));
    }
  }

  async function handleAcceptAliasInvite() {
    if (!theirIncomingAliasInvite) return;
    setAliasError(null);
    try {
      const newContact = await acceptIncomingInvite.mutateAsync({ inviteRef: theirIncomingAliasInvite.inviteRef });
      selectAliasContact(newContact.contactId);
      closeContactProfile();
    } catch (e) {
      setAliasError(String(e));
    }
  }

  return (
    <div className="contact-profile">
      <div className="contact-profile__header">
        <button className="sidebar__icon-btn" onClick={closeContactProfile} aria-label="Back">
          ←
        </button>
        <h2 className="settings-screen__title">Contact</h2>
      </div>

      <div className="contact-profile__body">
        {contact?.username ?? resolvedUsername ? (
          <span className="contact-profile__avatar" style={{ background: avatarColor(walletAddress) }}>
            {initials((contact?.username ?? resolvedUsername) as string)}
          </span>
        ) : (
          <span className="contact-profile__avatar contact-profile__avatar--dm">DM</span>
        )}
        <h3 className="contact-profile__name">{contact?.username ?? resolvedUsername ?? formatWalletAddress(walletAddress)}</h3>
        <p className="contact-profile__address">{truncateWalletAddress(walletAddress)}</p>

        {contact?.isBlocked && <p className="contact-profile__status contact-profile__status--blocked">Blocked</p>}
        {contact?.isContact && !contact?.isBlocked && <p className="contact-profile__status">In your contacts</p>}

        <div className="contact-profile__actions">
          {contact?.isContact ? (
            <button
              className="btn btn--secondary settings-btn-full"
              disabled={busy}
              onClick={() => removeFromContacts.mutate(walletAddress)}
            >
              Remove from contacts
            </button>
          ) : (
            <button
              className="btn btn--secondary settings-btn-full"
              disabled={busy || contact?.isBlocked}
              onClick={() => addToContacts.mutate(walletAddress)}
            >
              Add to contacts
            </button>
          )}

          {contact?.isBlocked ? (
            <button className="btn btn--secondary settings-btn-full" disabled={busy} onClick={() => unblockContact.mutate(walletAddress)}>
              Unblock
            </button>
          ) : (
            <button className="btn btn--danger settings-btn-full" disabled={busy} onClick={() => blockContact.mutate(walletAddress)}>
              Block
            </button>
          )}

          <button
            className="btn btn--primary settings-btn-full"
            onClick={() => {
              selectContact(walletAddress);
              closeContactProfile();
            }}
          >
            Open chat
          </button>

          {aliasError && <p className="pin-pad__error">{aliasError}</p>}
          {existingAliasContact ? (
            <button
              className="btn btn--secondary settings-btn-full"
              onClick={() => {
                selectAliasContact(existingAliasContact.contactId);
                closeContactProfile();
              }}
            >
              Open alias chat
            </button>
          ) : myPendingAliasInvite ? (
            <button className="btn btn--secondary settings-btn-full" disabled>
              Alias invite pending…
            </button>
          ) : theirIncomingAliasInvite ? (
            <button className="btn btn--secondary settings-btn-full" disabled={acceptIncomingInvite.isPending} onClick={handleAcceptAliasInvite}>
              {acceptIncomingInvite.isPending ? "Accepting…" : "Accept alias invite"}
            </button>
          ) : (
            <button className="btn btn--secondary settings-btn-full" disabled={createInviteForContact.isPending} onClick={handleStartAliasChat}>
              {createInviteForContact.isPending ? "Sending…" : "Start alias chat"}
            </button>
          )}

          <button className="btn btn--text settings-btn-full" disabled={busy} onClick={handleDelete}>
            Delete contact (clears cached keys/name too)
          </button>
        </div>
      </div>
    </div>
  );
}
