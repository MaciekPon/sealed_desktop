import { useEffect, useState } from "react";
import { useContacts, useResolvedUsername } from "../../queries/contacts";
import { useConversation, useMarkConversationAsRead, useSendMessage, useSyncMessages } from "../../queries/messaging";
import {
  useAcceptIncomingInvite,
  useAliasContacts,
  useAliasConversation,
  useDeclineIncomingInvite,
  useDeleteAliasContact,
  useIncomingInvites,
  useMarkAliasConversationRead,
  useRenameAliasContact,
  useSendAliasMessage,
} from "../../queries/alias";
import { useChatUiStore } from "../../stores/chatUiStore";
import { formatWalletAddress, truncateWalletAddress } from "../../lib/format";
import { MessageBubble } from "./MessageBubble";
import "./chat.css";

export function ChatWindow() {
  const selectedWallet = useChatUiStore((s) => s.selectedWallet);
  const selectedAliasContactId = useChatUiStore((s) => s.selectedAliasContactId);
  const selectedIncomingInviteRef = useChatUiStore((s) => s.selectedIncomingInviteRef);
  const sidebarCollapsed = useChatUiStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useChatUiStore((s) => s.toggleSidebar);
  const clearSelection = useChatUiStore((s) => s.clearSelection);
  const selectAliasContact = useChatUiStore((s) => s.selectAliasContact);

  const { data: contacts = [] } = useContacts();
  const contact = contacts.find((c) => c.walletAddress === selectedWallet) ?? null;
  // A username claim is on-chain, global state — resolve it even if this
  // wallet was never manually added as a contact, same reasoning as
  // `ContactsSidebar`'s `ConversationRow`.
  const { data: resolvedUsername } = useResolvedUsername(selectedWallet ?? "", selectedWallet !== null && !contact?.username);

  const { data: aliasContacts = [] } = useAliasContacts();
  const aliasContact = aliasContacts.find((c) => c.contactId === selectedAliasContactId) ?? null;

  const { data: incomingInvites = [] } = useIncomingInvites();
  const incomingInvite = incomingInvites.find((i) => i.inviteRef === selectedIncomingInviteRef) ?? null;

  const { data: messages = [] } = useConversation(selectedWallet);
  const { data: aliasMessages = [] } = useAliasConversation(selectedAliasContactId);
  const sendMessage = useSendMessage();
  const sendAliasMessage = useSendAliasMessage();
  const markAsRead = useMarkConversationAsRead();
  const markAliasAsRead = useMarkAliasConversationRead();
  const renameAliasContact = useRenameAliasContact();
  const deleteAliasContact = useDeleteAliasContact();
  const syncMessages = useSyncMessages();
  const acceptIncomingInvite = useAcceptIncomingInvite();
  const declineIncomingInvite = useDeclineIncomingInvite();

  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [renameInput, setRenameInput] = useState("");

  useEffect(() => {
    if (selectedWallet) {
      markAsRead.mutate(selectedWallet);
      // Opening a conversation is exactly when staleness is most visible —
      // don't make the user wait out the rest of the 8s background poll
      // interval (or reach for the heavier "Force resync" in Settings) just
      // because they happened to open the chat between ticks.
      syncMessages.mutate(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedWallet]);

  useEffect(() => {
    if (selectedAliasContactId) markAliasAsRead.mutate(selectedAliasContactId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedAliasContactId]);

  async function handleSend() {
    if (!draft.trim()) return;
    const text = draft;
    setDraft("");
    setError(null);
    try {
      if (selectedAliasContactId) {
        await sendAliasMessage.mutateAsync({ contactId: selectedAliasContactId, plaintext: text });
      } else if (selectedWallet) {
        await sendMessage.mutateAsync({ recipientWallet: selectedWallet, plaintext: text, recipientUsername: contact?.username ?? undefined });
      } else {
        return;
      }
    } catch (e) {
      setError(String(e));
      setDraft(text);
    }
  }

  function handleDeleteAliasContact() {
    if (!selectedAliasContactId) return;
    deleteAliasContact.mutate(selectedAliasContactId);
    clearSelection();
  }

  function submitRename() {
    if (!selectedAliasContactId || !renameInput.trim()) return;
    renameAliasContact.mutate({ contactId: selectedAliasContactId, label: renameInput.trim() });
    setRenaming(false);
  }

  async function handleAcceptInvite() {
    if (!selectedIncomingInviteRef) return;
    setError(null);
    try {
      const contact = await acceptIncomingInvite.mutateAsync({ inviteRef: selectedIncomingInviteRef });
      // Land directly on the now-established contact — this is the moment
      // the composer should appear, replacing the Accept/Decline buttons.
      selectAliasContact(contact.contactId);
    } catch (e) {
      setError(String(e));
    }
  }

  function handleDeclineInvite() {
    if (!selectedIncomingInviteRef) return;
    declineIncomingInvite.mutate(selectedIncomingInviteRef);
    clearSelection();
  }

  if (!selectedWallet && !selectedAliasContactId && !selectedIncomingInviteRef) {
    return (
      <div className="chat-window">
        {sidebarCollapsed && (
          <button className="chat-window__sidebar-toggle" style={{ margin: 16 }} onClick={toggleSidebar}>
            ⟩ Contacts
          </button>
        )}
        <div className="chat-window__empty">
          <p>Select a contact to start chatting.</p>
        </div>
      </div>
    );
  }

  const isAlias = selectedAliasContactId !== null;
  const isIncoming = selectedIncomingInviteRef !== null;
  const headerName = isIncoming
    ? (incomingInvite?.peerUsername ?? (incomingInvite ? formatWalletAddress(incomingInvite.peerWallet) : "Alias invitation"))
    : isAlias
      ? (aliasContact?.label ?? "Alias chat")
      : (contact?.username ?? resolvedUsername ?? formatWalletAddress(selectedWallet as string));
  const activeMessages = isAlias ? aliasMessages : messages;
  const sending = isAlias ? sendAliasMessage.isPending : sendMessage.isPending;

  return (
    <div className="chat-window">
      <div className="chat-window__header">
        {sidebarCollapsed && (
          <button className="chat-window__sidebar-toggle" onClick={toggleSidebar}>
            ⟩
          </button>
        )}
        <div>
          <p className="chat-window__header-name">{headerName}</p>
          {!isAlias && !isIncoming && (contact?.username ?? resolvedUsername) && <p className="chat-window__header-address">{truncateWalletAddress(selectedWallet as string)}</p>}
          {isAlias && <p className="chat-window__header-address">Alias contact — no wallet identity shared</p>}
        </div>
        {isAlias && !renaming && (
          <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
            <button
              className="btn btn--text"
              onClick={() => {
                setRenameInput(aliasContact?.label ?? "");
                setRenaming(true);
              }}
            >
              Rename
            </button>
            <button className="btn btn--text" onClick={handleDeleteAliasContact}>
              Delete
            </button>
          </div>
        )}
        {isAlias && renaming && (
          <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
            <input className="settings-input" value={renameInput} onChange={(e) => setRenameInput(e.target.value)} />
            <button className="btn btn--text" onClick={submitRename}>
              Save
            </button>
            <button className="btn btn--text" onClick={() => setRenaming(false)}>
              Cancel
            </button>
          </div>
        )}
      </div>

      <div className="chat-window__messages">
        {isIncoming ? (
          <p className="chat-window__empty" style={{ margin: "auto" }}>
            Accept to start exchanging messages, or decline to dismiss this invitation.
          </p>
        ) : (
          activeMessages.map((m) => <MessageBubble key={m.id} message={m} />)
        )}
      </div>

      {error && (
        <p className="pin-pad__error" style={{ padding: "0 20px" }}>
          {error}
        </p>
      )}

      {isIncoming ? (
        <div className="chat-window__composer">
          <button
            className="chat-window__send-btn"
            onClick={handleAcceptInvite}
            disabled={acceptIncomingInvite.isPending}
            style={{ flex: 1 }}
          >
            Accept
          </button>
          <button
            className="btn btn--text"
            onClick={handleDeclineInvite}
            disabled={declineIncomingInvite.isPending}
            style={{ flex: 1 }}
          >
            Decline
          </button>
        </div>
      ) : (
        <div className="chat-window__composer">
          <textarea
            className="chat-window__composer-input"
            placeholder="Type a message…"
            rows={1}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                handleSend();
              }
            }}
          />
          <button className="chat-window__send-btn" onClick={handleSend} disabled={sending || !draft.trim()}>
            Send
          </button>
        </div>
      )}
    </div>
  );
}
