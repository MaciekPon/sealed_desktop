import { useState } from "react";
import { useChatUiStore } from "../../stores/chatUiStore";
import {
  useAcceptInvite,
  useCompleteInvite,
  useCreateInvite,
  useDeletePendingInvite,
  useDismissPendingInvite,
  usePendingInvites,
} from "../../queries/alias";
import { QrCode } from "./QrCode";
import { AliasPasteDialog } from "./AliasPasteDialog";
import "../settings/settings.css";
import "./alias.css";

type View = "main" | "create" | "accept";

/**
 * Local QR/paste handshake for alias chats (Faza 7f). One screen with
 * local sub-views, matching `SettingsScreen`'s established pattern rather
 * than a router — reached from the sidebar's "New alias chat" button.
 *
 * Desktop never scans a camera: this side always generates a QR (for a
 * phone to scan) and accepts the other party's envelope via paste instead.
 * That means desktop-to-desktop pairing works cleanly both ways (copy the
 * text out of one window, paste into the other), but a phone-shown QR
 * can't be scanned back into desktop — only desktop-shown QRs are
 * scannable, one direction.
 */
export function AliasScreen() {
  const closeAlias = useChatUiStore((s) => s.closeAlias);
  const selectAliasContact = useChatUiStore((s) => s.selectAliasContact);

  const { data: pendingInvites = [] } = usePendingInvites();
  const createInvite = useCreateInvite();
  const acceptInvite = useAcceptInvite();
  const completeInvite = useCompleteInvite();
  const dismissPendingInvite = useDismissPendingInvite();
  const deletePendingInvite = useDeletePendingInvite();

  const [view, setView] = useState<View>("main");
  const [error, setError] = useState<string | null>(null);
  const [createdInvite, setCreatedInvite] = useState<{ envelopeBase64: string } | null>(null);
  const [acceptedReply, setAcceptedReply] = useState<{ envelopeBase64: string; contactId: string } | null>(null);
  const [completingRef, setCompletingRef] = useState<string | null>(null);

  function backToMain() {
    setView("main");
    setError(null);
    setCreatedInvite(null);
    setAcceptedReply(null);
    setCompletingRef(null);
  }

  async function handleCreateInvite() {
    setError(null);
    try {
      const invite = await createInvite.mutateAsync(undefined);
      setCreatedInvite({ envelopeBase64: invite.envelopeBase64 });
      setView("create");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleAcceptSubmit(envelopeBase64: string) {
    setError(null);
    try {
      const result = await acceptInvite.mutateAsync({ envelopeBase64 });
      setAcceptedReply({ envelopeBase64: result.replyEnvelopeBase64, contactId: result.contact.contactId });
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleCompleteSubmit(envelopeBase64: string) {
    setError(null);
    try {
      const contact = await completeInvite.mutateAsync(envelopeBase64);
      setCompletingRef(null);
      selectAliasContact(contact.contactId);
    } catch (e) {
      setError(String(e));
    }
  }

  if (view === "create" && createdInvite) {
    return (
      <div className="settings-screen">
        <div className="settings-screen__header">
          <button className="sidebar__icon-btn" onClick={backToMain} aria-label="Back">
            ←
          </button>
          <h2 className="settings-screen__title">Your invite</h2>
        </div>
        <div className="settings-screen__body">
          <p className="settings-screen__hint">Show this to a phone to scan, or copy the text below to another desktop.</p>
          <div className="alias-qr">
            <QrCode value={createdInvite.envelopeBase64} />
          </div>
          <textarea className="alias-paste__textarea" readOnly value={createdInvite.envelopeBase64} />
          <button className="btn btn--secondary settings-btn-full" onClick={() => navigator.clipboard.writeText(createdInvite.envelopeBase64)}>
            Copy text
          </button>
          <p className="settings-screen__hint">Once they reply, come back here — your pending invite will show up below with a "Complete" option.</p>
          <button className="btn btn--primary settings-btn-full" onClick={backToMain}>
            Done
          </button>
        </div>
      </div>
    );
  }

  if (view === "accept") {
    return (
      <div className="settings-screen">
        <div className="settings-screen__header">
          <button className="sidebar__icon-btn" onClick={backToMain} aria-label="Back">
            ←
          </button>
          <h2 className="settings-screen__title">Accept an invite</h2>
        </div>
        <div className="settings-screen__body">
          {error && <p className="pin-pad__error">{error}</p>}
          {!acceptedReply ? (
            <>
              <p className="settings-screen__hint">Paste the invite text the other party sent (or scanned to you) below.</p>
              <AliasPasteDialog label="Invite text" placeholder="Paste invite envelope…" busy={acceptInvite.isPending} onSubmit={handleAcceptSubmit} />
            </>
          ) : (
            <>
              <p className="settings-screen__notice">Accepted. Send this reply back to complete the handshake.</p>
              <div className="alias-qr">
                <QrCode value={acceptedReply.envelopeBase64} />
              </div>
              <textarea className="alias-paste__textarea" readOnly value={acceptedReply.envelopeBase64} />
              <button className="btn btn--secondary settings-btn-full" onClick={() => navigator.clipboard.writeText(acceptedReply.envelopeBase64)}>
                Copy text
              </button>
              <button
                className="btn btn--primary settings-btn-full"
                onClick={() => {
                  const contactId = acceptedReply.contactId;
                  backToMain();
                  selectAliasContact(contactId);
                }}
              >
                Open chat
              </button>
            </>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="settings-screen">
      <div className="settings-screen__header">
        <h2 className="settings-screen__title">Alias chats</h2>
        <button className="sidebar__icon-btn" onClick={closeAlias} aria-label="Close">
          ✕
        </button>
      </div>
      <div className="settings-screen__body">
        {error && <p className="pin-pad__error">{error}</p>}

        <section className="settings-section">
          <h3 className="settings-section__title">Start</h3>
          <button className="btn btn--secondary settings-btn-full" disabled={createInvite.isPending} onClick={handleCreateInvite}>
            {createInvite.isPending ? "Creating…" : "Create invite"}
          </button>
          <button className="btn btn--secondary settings-btn-full" onClick={() => setView("accept")}>
            Accept an invite
          </button>
        </section>

        {pendingInvites.filter((p) => !p.dismissed).length > 0 && (
          <section className="settings-section">
            <h3 className="settings-section__title">Pending invites</h3>
            {pendingInvites
              .filter((p) => !p.dismissed)
              .map((p) => (
                <div key={p.inviteRef} className="alias-pending-row">
                  <div className="alias-pending-row__info">
                    <span className="settings-row__value">{p.label ?? "Untitled invite"}</span>
                    <span className="alias-pending-row__ref">{p.inviteRef.slice(0, 16)}…</span>
                  </div>
                  {completingRef === p.inviteRef ? (
                    <AliasPasteDialog
                      label="Reply text"
                      placeholder="Paste their reply envelope…"
                      busy={completeInvite.isPending}
                      onSubmit={handleCompleteSubmit}
                    />
                  ) : (
                    <div className="alias-pending-row__actions">
                      <button className="btn btn--text" onClick={() => setCompletingRef(p.inviteRef)}>
                        Complete
                      </button>
                      <button className="btn btn--text" onClick={() => dismissPendingInvite.mutate(p.inviteRef)}>
                        Dismiss
                      </button>
                      <button className="btn btn--text" onClick={() => deletePendingInvite.mutate(p.inviteRef)}>
                        Delete
                      </button>
                    </div>
                  )}
                </div>
              ))}
          </section>
        )}
      </div>
    </div>
  );
}
