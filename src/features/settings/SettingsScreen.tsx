import { useEffect, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useChatUiStore } from "../../stores/chatUiStore";
import { useSessionStore } from "../../stores/sessionStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { useCredits, useRedeemCode } from "../../queries/credits";
import { useWalletBalance } from "../../queries/wallet";
import { useClaimUsername, useReleaseUsername } from "../../queries/username";
import { useForceResync, useSyncMessages } from "../../queries/messaging";
import { useDisableTerminationCode, useIsTerminationConfigured, useSetTerminationCode } from "../../queries/settings";
import { settings as settingsApi, keys as keysApi } from "../../lib/tauri";
import { PinPad } from "../auth/PinPad";
import { formatAlgoBalance } from "../../lib/format";
import "./settings.css";

type View =
  | "main"
  | "changePin_old"
  | "changePin_new"
  | "changePin_confirm"
  | "termination_verify"
  | "termination_setCode"
  | "termination_confirmCode"
  | "seed_verify"
  | "seed_view";

/**
 * The one screen that reaches everything Phase 1/2 of the desktop parity
 * work left with real, tested backend but no UI path at all: credits,
 * username, PIN change, termination code, seed backup, and log out. Mirrors
 * `ui/settings/screens/settings.dart` + `change_pin_flow.dart` +
 * `change_termination_flow.dart`'s flow *shape* (not their widget code).
 */
export function SettingsScreen() {
  const closeSettings = useChatUiStore((s) => s.closeSettings);
  const account = useSessionStore((s) => s.account);
  const changePin = useSessionStore((s) => s.changePin);
  const logOut = useSessionStore((s) => s.logOut);

  const autoSyncEnabled = useSettingsStore((s) => s.autoSyncEnabled);
  const settingsLoaded = useSettingsStore((s) => s.loaded);
  const loadSettings = useSettingsStore((s) => s.load);
  const setAutoSyncEnabled = useSettingsStore((s) => s.setAutoSyncEnabled);

  useEffect(() => {
    if (!settingsLoaded) loadSettings();
  }, [settingsLoaded, loadSettings]);

  const { data: credits } = useCredits();
  const { data: balanceMicroAlgos } = useWalletBalance();
  const redeemCode = useRedeemCode();
  const claimUsername = useClaimUsername();
  const releaseUsername = useReleaseUsername();
  const { data: terminationConfigured } = useIsTerminationConfigured();
  const setTerminationCode = useSetTerminationCode();
  const disableTerminationCode = useDisableTerminationCode();
  const forceResync = useForceResync();
  const syncNow = useSyncMessages();
  const republishKeys = useMutation({ mutationFn: () => keysApi.ensurePublished() });

  const [view, setView] = useState<View>("main");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [resetToken, setResetToken] = useState(0);
  const [busy, setBusy] = useState(false);

  const [oldPin, setOldPin] = useState<string | null>(null);
  const [newPin, setNewPin] = useState<string | null>(null);

  const [terminationTargetAction, setTerminationTargetAction] = useState<"set" | "disable" | null>(null);
  const [pendingTerminationCode, setPendingTerminationCode] = useState<string | null>(null);

  const [seedWords, setSeedWords] = useState<string[] | null>(null);

  const [redeemCodeInput, setRedeemCodeInput] = useState("");
  const [redeemUsernameInput, setRedeemUsernameInput] = useState("");
  const [claimInput, setClaimInput] = useState("");
  const [confirmingLogout, setConfirmingLogout] = useState(false);
  const [addressCopied, setAddressCopied] = useState(false);

  async function handleCopyAddress() {
    if (!account) return;
    await navigator.clipboard.writeText(account.walletAddress);
    setAddressCopied(true);
    setTimeout(() => setAddressCopied(false), 1500);
  }

  function backToMain() {
    setView("main");
    setError(null);
    setOldPin(null);
    setNewPin(null);
    setTerminationTargetAction(null);
    setPendingTerminationCode(null);
  }

  // --- change PIN ---

  function handleOldPinEntered(pin: string) {
    setOldPin(pin);
    setError(null);
    setView("changePin_new");
  }

  function handleNewPinEntered(pin: string) {
    setNewPin(pin);
    setError(null);
    setView("changePin_confirm");
  }

  async function handleConfirmPinEntered(pin: string) {
    if (!oldPin) return; // unreachable — old PIN is always set before this step
    if (pin !== newPin) {
      setError("PINs don't match. Try again.");
      setNewPin(null);
      setView("changePin_new");
      setResetToken((t) => t + 1);
      return;
    }
    setBusy(true);
    try {
      await changePin(oldPin, pin);
      setNotice("PIN changed.");
      backToMain();
    } catch (e) {
      setError(String(e));
      setOldPin(null);
      setNewPin(null);
      setView("changePin_old");
      setResetToken((t) => t + 1);
    } finally {
      setBusy(false);
    }
  }

  // --- termination code ---

  function startSetTermination() {
    setError(null);
    setTerminationTargetAction("set");
    setView("termination_verify");
  }

  function startDisableTermination() {
    setError(null);
    setTerminationTargetAction("disable");
    setView("termination_verify");
  }

  async function handleTerminationVerifyPin(pin: string) {
    setBusy(true);
    setError(null);
    try {
      const ok = await settingsApi.verifyPin(pin);
      if (!ok) {
        setError("Wrong PIN.");
        setResetToken((t) => t + 1);
        return;
      }
      if (terminationTargetAction === "disable") {
        await disableTerminationCode.mutateAsync();
        setNotice("Termination code disabled.");
        backToMain();
      } else {
        setView("termination_setCode");
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function handleTerminationCodeEntered(code: string) {
    setPendingTerminationCode(code);
    setError(null);
    setView("termination_confirmCode");
  }

  async function handleTerminationCodeConfirmed(code: string) {
    if (code !== pendingTerminationCode) {
      setError("Codes don't match. Try again.");
      setPendingTerminationCode(null);
      setView("termination_setCode");
      setResetToken((t) => t + 1);
      return;
    }
    setBusy(true);
    try {
      await setTerminationCode.mutateAsync(code);
      setNotice("Termination code set.");
      backToMain();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  // --- seed phrase backup ---

  async function handleSeedVerifyPin(pin: string) {
    setBusy(true);
    setError(null);
    try {
      const ok = await settingsApi.verifyPin(pin);
      if (!ok) {
        setError("Wrong PIN.");
        setResetToken((t) => t + 1);
        return;
      }
      const phrase = await settingsApi.getSeedPhraseForBackup();
      setSeedWords(phrase.trim().split(/\s+/));
      setView("seed_view");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  // --- credits / username (no PIN gate — same as mobile) ---

  async function handleRedeem() {
    setError(null);
    setNotice(null);
    try {
      await redeemCode.mutateAsync({ code: redeemCodeInput.trim(), username: redeemUsernameInput.trim() || undefined });
      setNotice("Code redeemed.");
      setRedeemCodeInput("");
      setRedeemUsernameInput("");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleClaim() {
    setError(null);
    setNotice(null);
    const name = claimInput.trim();
    try {
      await claimUsername.mutateAsync({ name });
      setNotice(`Username "${name}" claimed.`);
      setClaimInput("");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleRelease() {
    setError(null);
    setNotice(null);
    try {
      await releaseUsername.mutateAsync(undefined);
      setNotice("Username released.");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleForceResync() {
    setError(null);
    setNotice(null);
    try {
      const newCount = await forceResync.mutateAsync();
      setNotice(newCount > 0 ? `Resync complete — ${newCount} message(s) found.` : "Resync complete — nothing new.");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleSyncNow() {
    setError(null);
    setNotice(null);
    try {
      const newCount = await syncNow.mutateAsync(false);
      setNotice(newCount > 0 ? `Synced — ${newCount} new message(s).` : "Synced — nothing new.");
    } catch (e) {
      setError(String(e));
    }
  }

  /** Manual trigger + visible feedback for what was previously a silent,
   * fire-and-forget call on unlock (`sessionStore.ts`'s `fireEnsureKeysPublished`)
   * — a user hit a case where an incoming message from someone who'd never
   * cached our keys still failed to decrypt, and there was no way to check
   * (or force) whether our corrected key material had actually made it
   * on-chain, versus the publish having silently failed/no-op'd earlier. */
  async function handleRepublishKeys() {
    setError(null);
    setNotice(null);
    try {
      const published = await republishKeys.mutateAsync();
      setNotice(published ? "Keys republished on-chain." : "Keys already up to date on-chain — nothing to publish.");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleLogOut() {
    setBusy(true);
    try {
      await logOut();
    } finally {
      setBusy(false);
    }
  }

  if (view === "changePin_old") {
    return <PinPad headline="Enter current PIN" errorText={error} resetToken={resetToken} onBack={backToMain} onComplete={handleOldPinEntered} />;
  }
  if (view === "changePin_new") {
    return <PinPad headline="Choose a new PIN" errorText={error} resetToken={resetToken} onBack={backToMain} onComplete={handleNewPinEntered} />;
  }
  if (view === "changePin_confirm") {
    return (
      <PinPad
        headline="Confirm new PIN"
        errorText={error}
        loading={busy}
        resetToken={resetToken}
        onBack={() => setView("changePin_new")}
        onComplete={handleConfirmPinEntered}
      />
    );
  }
  if (view === "termination_verify") {
    return (
      <PinPad
        headline="Enter your PIN to continue"
        errorText={error}
        loading={busy}
        resetToken={resetToken}
        onBack={backToMain}
        onComplete={handleTerminationVerifyPin}
      />
    );
  }
  if (view === "termination_setCode") {
    return (
      <PinPad
        headline="Choose a termination code"
        subhead="Entering this instead of your PIN on the lock screen wipes this device. Make it different from your real PIN."
        errorText={error}
        resetToken={resetToken}
        onBack={backToMain}
        onComplete={handleTerminationCodeEntered}
      />
    );
  }
  if (view === "termination_confirmCode") {
    return (
      <PinPad
        headline="Confirm termination code"
        errorText={error}
        loading={busy}
        resetToken={resetToken}
        onBack={() => setView("termination_setCode")}
        onComplete={handleTerminationCodeConfirmed}
      />
    );
  }
  if (view === "seed_verify") {
    return (
      <PinPad
        headline="Enter your PIN to view your recovery phrase"
        errorText={error}
        loading={busy}
        resetToken={resetToken}
        onBack={backToMain}
        onComplete={handleSeedVerifyPin}
      />
    );
  }
  if (view === "seed_view" && seedWords) {
    return (
      <div className="settings-screen">
        <div className="settings-screen__header">
          <button className="sidebar__icon-btn" onClick={backToMain} aria-label="Back">
            ←
          </button>
          <h2 className="settings-screen__title">Recovery phrase</h2>
        </div>
        <div className="settings-screen__body">
          <p className="settings-screen__hint">Anyone with these words can access your wallet. Keep them offline and private.</p>
          <div className="mnemonic-grid">
            {seedWords.map((word, i) => (
              <div className="mnemonic-grid__word" key={i}>
                <span className="mnemonic-grid__index">{i + 1}.</span>
                <span>{word}</span>
              </div>
            ))}
          </div>
          <button className="btn btn--primary" onClick={backToMain}>
            Done
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="settings-screen">
      <div className="settings-screen__header">
        <h2 className="settings-screen__title">Settings</h2>
        <button className="sidebar__icon-btn" onClick={closeSettings} aria-label="Close settings">
          ✕
        </button>
      </div>

      <div className="settings-screen__body">
        {notice && <p className="settings-screen__notice">{notice}</p>}
        {error && <p className="pin-pad__error">{error}</p>}

        <section className="settings-section">
          <h3 className="settings-section__title">Account</h3>
          <div className="settings-row settings-row--address">
            <span className="settings-row__value settings-row__value--address">{account?.walletAddress ?? "—"}</span>
            <button className="btn btn--secondary" disabled={!account} onClick={handleCopyAddress}>
              {addressCopied ? "Copied!" : "Copy"}
            </button>
          </div>
          <div className="settings-row">
            <span className="settings-row__label">Balance</span>
            <span className="settings-row__value">{balanceMicroAlgos !== undefined ? formatAlgoBalance(balanceMicroAlgos) : "—"}</span>
          </div>
          <p className="settings-screen__hint">
            Your encryption keys publish on-chain automatically on unlock, silently. If someone new can't decrypt
            messages you receive from them (they see your messages fine, but you don't see theirs), republish here
            to confirm it actually went through.
          </p>
          <button className="btn btn--secondary settings-btn-full" disabled={republishKeys.isPending} onClick={handleRepublishKeys}>
            {republishKeys.isPending ? "Publishing…" : "Republish keys"}
          </button>
        </section>

        <section className="settings-section">
          <h3 className="settings-section__title">Username</h3>
          <div className="settings-row settings-row--form">
            <input
              className="settings-input"
              placeholder="choose-a-username"
              value={claimInput}
              onChange={(e) => setClaimInput(e.target.value)}
            />
            <button className="btn btn--secondary" disabled={!claimInput.trim() || claimUsername.isPending} onClick={handleClaim}>
              {claimUsername.isPending ? "Claiming…" : "Claim"}
            </button>
          </div>
          <button className="btn btn--text settings-btn-full" disabled={releaseUsername.isPending} onClick={handleRelease}>
            Release my current username
          </button>
        </section>

        <section className="settings-section">
          <h3 className="settings-section__title">Credits</h3>
          <div className="settings-row">
            <span className="settings-row__label">Balance</span>
            <span className="settings-row__value">{credits ?? "—"}</span>
          </div>
          <div className="settings-row settings-row--form">
            <input
              className="settings-input"
              placeholder="XXXX-XXXX-XXXX-XXXX"
              value={redeemCodeInput}
              onChange={(e) => setRedeemCodeInput(e.target.value)}
            />
            <button className="btn btn--secondary" disabled={!redeemCodeInput.trim() || redeemCode.isPending} onClick={handleRedeem}>
              {redeemCode.isPending ? "Redeeming…" : "Redeem"}
            </button>
          </div>
          <input
            className="settings-input settings-input--full"
            placeholder="Claim this username with the code (optional)"
            value={redeemUsernameInput}
            onChange={(e) => setRedeemUsernameInput(e.target.value)}
          />
        </section>

        <section className="settings-section">
          <h3 className="settings-section__title">Sync</h3>
          <label className="settings-row settings-row--toggle">
            <span className="settings-row__label">Auto-sync in background</span>
            <input type="checkbox" checked={autoSyncEnabled} onChange={(e) => setAutoSyncEnabled(e.target.checked)} />
          </label>
          <p className="settings-screen__hint">
            Background sync checks for new messages every 8 seconds while unlocked. Use "Sync now" for an immediate
            check (cheap, only fetches what changed since the last sync). If a message you can see on another
            device still isn't showing up after that, force a full resync from the chain instead.
          </p>
          <button className="btn btn--secondary settings-btn-full" disabled={syncNow.isPending} onClick={handleSyncNow}>
            {syncNow.isPending ? "Syncing…" : "Sync now"}
          </button>
          <button className="btn btn--secondary settings-btn-full" disabled={forceResync.isPending} onClick={handleForceResync}>
            {forceResync.isPending ? "Syncing…" : "Force resync"}
          </button>
        </section>

        <section className="settings-section">
          <h3 className="settings-section__title">Security</h3>
          <button className="btn btn--secondary settings-btn-full" onClick={() => setView("changePin_old")}>
            Change PIN
          </button>
          <button className="btn btn--secondary settings-btn-full" onClick={() => setView("seed_verify")}>
            View recovery phrase
          </button>
          {terminationConfigured ? (
            <button className="btn btn--secondary settings-btn-full" onClick={startDisableTermination}>
              Disable termination code
            </button>
          ) : (
            <button className="btn btn--secondary settings-btn-full" onClick={startSetTermination}>
              Set termination code
            </button>
          )}
        </section>

        <section className="settings-section settings-section--danger">
          <h3 className="settings-section__title">Danger zone</h3>
          {!confirmingLogout ? (
            <button className="btn btn--danger settings-btn-full" onClick={() => setConfirmingLogout(true)}>
              Log out
            </button>
          ) : (
            <div className="settings-row--form">
              <p className="settings-screen__hint">This wipes this device's local data. Make sure you've backed up your recovery phrase.</p>
              <button className="btn btn--danger settings-btn-full" disabled={busy} onClick={handleLogOut}>
                {busy ? "Logging out…" : "Confirm log out"}
              </button>
              <button className="btn btn--text settings-btn-full" onClick={() => setConfirmingLogout(false)}>
                Cancel
              </button>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
