import { useEffect, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useSessionStore } from "../../stores/sessionStore";
import { useSettingsStore } from "../../stores/settingsStore";
import { useCredits, useRedeemCode } from "../../queries/credits";
import { useWalletBalance } from "../../queries/wallet";
import { useResolvedUsername } from "../../queries/contacts";
import { useClaimUsername, useReleaseUsername } from "../../queries/username";
import { useForceResync, useSyncMessages } from "../../queries/messaging";
import { useDisableTerminationCode, useIsTerminationConfigured, useSetTerminationCode } from "../../queries/settings";
import { settings as settingsApi, keys as keysApi } from "../../lib/tauri";
import { PinPad } from "../auth/PinPad";
import { QrCode } from "../alias/QrCode";
import { formatAlgoBalance, truncateWalletAddress } from "../../lib/format";
import {
  IconAt,
  IconAtom,
  IconBell,
  IconChevronRight,
  IconFilter,
  IconInfo,
  IconKey,
  IconLock,
  IconPencil,
  IconPlus,
  IconPower,
  IconRefresh,
  IconShieldOff,
  IconStar,
  IconTrash,
  IconWallet,
} from "./icons";
import "./settings.css";

type View =
  | "main"
  | "username"
  | "redeemCode"
  | "walletReveal"
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
 *
 * Main-view layout restyled 2026-08-18 to match a supplied design mockup —
 * see the plan/memory entry for the source screenshots. Rows that need
 * backend work the app doesn't have yet (Bio, Top up Wallet, Push
 * notification, Auto-Delete Local Files, Falcon/post-quantum transactions,
 * Spam filter) are rendered per the mockup but disabled/non-interactive,
 * per an explicit product decision rather than silently guessed at. Real,
 * already-working features that don't appear in the mockup at all
 * (auto-sync toggle, "Sync now", "Republish keys") are kept in an
 * "Advanced" section below the mockup's sections rather than deleted.
 *
 * Hosted inside `NavDrawer` (not a standalone `screen`) per the mockup —
 * the drawer's own panel swaps to this content instead of the whole app
 * navigating away, so `onClose` is passed in by the caller rather than
 * reaching for `useChatUiStore` directly, keeping this component
 * embeddable regardless of where it's shown from.
 */
export function SettingsScreen({ onClose }: { onClose: () => void }) {
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
  const { data: myUsername } = useResolvedUsername(account?.walletAddress ?? "", !!account);
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
      backToMain();
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

  if (view === "username") {
    return (
      <div className="settings-screen">
        <div className="settings-screen__header">
          <button className="sidebar__icon-btn" onClick={backToMain} aria-label="Back">
            ←
          </button>
          <h2 className="settings-screen__title">Username</h2>
        </div>
        <div className="settings-screen__body">
          {notice && <p className="settings-screen__notice">{notice}</p>}
          {error && <p className="pin-pad__error">{error}</p>}
          <p className="settings-screen__hint">
            {myUsername ? `Your current username is @${myUsername}.` : "You haven't claimed a username yet."} Usernames are
            public, on-chain, and let others find you without your wallet address.
          </p>
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
          {myUsername && (
            <button className="btn btn--text settings-btn-full" disabled={releaseUsername.isPending} onClick={handleRelease}>
              Release my current username
            </button>
          )}
        </div>
      </div>
    );
  }

  if (view === "redeemCode") {
    return (
      <div className="settings-screen">
        <div className="settings-screen__header">
          <button className="sidebar__icon-btn" onClick={backToMain} aria-label="Back">
            ←
          </button>
          <h2 className="settings-screen__title">Redeem code</h2>
        </div>
        <div className="settings-screen__body">
          {notice && <p className="settings-screen__notice">{notice}</p>}
          {error && <p className="pin-pad__error">{error}</p>}
          <p className="settings-screen__hint">Enter a credit code to top up your balance — 1 credit sends 1 message.</p>
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
        </div>
      </div>
    );
  }

  if (view === "walletReveal") {
    return (
      <div className="settings-screen">
        <div className="settings-screen__header">
          <button className="sidebar__icon-btn" onClick={backToMain} aria-label="Back">
            ←
          </button>
          <h2 className="settings-screen__title">Wallet address</h2>
        </div>
        <div className="settings-screen__body settings-screen__body--centered">
          {account && <QrCode value={account.walletAddress} size={200} />}
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
        </div>
      </div>
    );
  }

  return (
    <div className="settings-screen">
      <div className="settings-screen__header">
        <h2 className="settings-screen__title">Settings</h2>
        <button className="sidebar__icon-btn" onClick={onClose} aria-label="Close settings">
          ✕
        </button>
      </div>

      <div className="settings-screen__body">
        {notice && <p className="settings-screen__notice">{notice}</p>}
        {error && <p className="pin-pad__error">{error}</p>}

        <SettingsSection title="My Account">
          <SettingsListRow
            icon={<IconAt />}
            label="Username"
            sublabel={myUsername ? `@${myUsername}` : "Not claimed"}
            right={<IconChevronRight />}
            onClick={() => setView("username")}
          />
          <SettingsListRow icon={<IconPencil />} label="Bio Description" sublabel="Not available yet" disabled />
          <SettingsListRow
            icon={<IconWallet />}
            label="Wallet"
            sublabel={account ? truncateWalletAddress(account.walletAddress) : "—"}
            right={<span className="settings-list-row__btn">Show wallet</span>}
            onClick={() => setView("walletReveal")}
          />
        </SettingsSection>

        <SettingsSection title="App Credits">
          <SettingsListRow
            icon={<IconInfo />}
            label="Sealed Credits"
            sublabel={`enough to send ${credits ?? 0} Messages`}
            right={<span className="settings-list-row__pill">{credits ?? "—"}</span>}
          />
          <SettingsListRow
            icon={<IconStar />}
            label="Redeem code"
            sublabel="Enter the code and claim your credits"
            right={<IconChevronRight />}
            onClick={() => setView("redeemCode")}
          />
          <SettingsListRow icon={<IconPlus />} label="Top up Wallet" sublabel="Add credits to your account" disabled />
        </SettingsSection>

        <SettingsSection title="Notification">
          <SettingsListRow icon={<IconBell />} label="Push notification" disabled right={<ToggleIndicator on={false} disabled />} />
        </SettingsSection>

        <SettingsSection title="Passwords">
          <SettingsListRow icon={<IconLock />} label="Change pass code" right={<IconChevronRight />} onClick={() => setView("changePin_old")} />
          <SettingsListRow
            icon={<IconShieldOff />}
            label="Change termination code"
            sublabel={terminationConfigured ? "Currently set" : "Not set"}
            right={<IconChevronRight />}
            onClick={terminationConfigured ? startDisableTermination : startSetTermination}
          />
        </SettingsSection>

        <SettingsSection title="Preferences">
          <SettingsListRow icon={<IconTrash />} label="Auto-Delete Local Files" sublabel="Enable Local Data Wipe" disabled right={<ToggleIndicator on={false} disabled />} />
          <SettingsListRow
            icon={<IconRefresh className={forceResync.isPending ? "settings-list-row__icon--spin" : undefined} />}
            label="Force re-sync"
            sublabel="re-sync all messages and files"
            right={<span className="settings-list-row__status">{forceResync.isPending ? "Syncing…" : "Synced ✓"}</span>}
            onClick={handleForceResync}
          />
          <SettingsListRow icon={<IconAtom />} label="Falcon" sublabel="Post-quantum Transaction" disabled right={<ToggleIndicator on={false} disabled />} />
          <SettingsListRow icon={<IconFilter />} label="Spam filter" disabled right={<ToggleIndicator on={false} disabled />} />
        </SettingsSection>

        <SettingsSection title="Advanced">
          <SettingsListRow
            icon={<IconRefresh />}
            label="Auto-sync in background"
            sublabel="Checks for new messages every few seconds while unlocked"
            right={<ToggleIndicator on={autoSyncEnabled} onClick={() => setAutoSyncEnabled(!autoSyncEnabled)} />}
          />
          <SettingsListRow
            icon={<IconRefresh />}
            label="Sync now"
            sublabel="Quick check for anything new since the last sync"
            right={<span className="settings-list-row__status">{syncNow.isPending ? "Syncing…" : ""}</span>}
            onClick={handleSyncNow}
          />
          <SettingsListRow
            icon={<IconKey />}
            label="Republish keys"
            sublabel="Confirm your encryption keys are up to date on-chain"
            right={<span className="settings-list-row__status">{republishKeys.isPending ? "Publishing…" : ""}</span>}
            onClick={handleRepublishKeys}
          />
          <SettingsListRow icon={<IconKey />} label="View recovery phrase" right={<IconChevronRight />} onClick={() => setView("seed_verify")} />
        </SettingsSection>

        <section className="settings-section settings-section--danger">
          {!confirmingLogout ? (
            <SettingsListRow icon={<IconPower />} label="Log out" danger onClick={() => setConfirmingLogout(true)} />
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

function SettingsSection({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="settings-section">
      <h3 className="settings-section__title settings-section__title--accent">{title}</h3>
      <div className="settings-list">{children}</div>
    </section>
  );
}

function SettingsListRow({
  icon,
  label,
  sublabel,
  right,
  onClick,
  disabled,
  danger,
}: {
  icon: React.ReactNode;
  label: string;
  sublabel?: string;
  right?: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  danger?: boolean;
}) {
  const clickable = !!onClick && !disabled;
  const Tag = clickable ? "button" : "div";
  return (
    <Tag
      className={`settings-list-row ${disabled ? "settings-list-row--disabled" : ""} ${danger ? "settings-list-row--danger" : ""}`}
      onClick={clickable ? onClick : undefined}
    >
      <span className={`settings-list-row__icon ${danger ? "settings-list-row__icon--danger" : ""}`}>{icon}</span>
      <span className="settings-list-row__text">
        <span className="settings-list-row__label">{label}</span>
        {sublabel && <span className="settings-list-row__sublabel">{sublabel}</span>}
      </span>
      {right && <span className="settings-list-row__right">{right}</span>}
    </Tag>
  );
}

/** Visual-only toggle pill. `onClick` present + no `disabled` makes it interactive; otherwise it's a static, dimmed indicator for a setting with no backend yet. */
function ToggleIndicator({ on, disabled, onClick }: { on: boolean; disabled?: boolean; onClick?: () => void }) {
  const interactive = !!onClick && !disabled;
  return (
    <span
      role={interactive ? "switch" : undefined}
      aria-checked={interactive ? on : undefined}
      className={`settings-toggle ${on ? "settings-toggle--on" : ""} ${disabled ? "settings-toggle--disabled" : ""}`}
      onClick={interactive ? onClick : undefined}
    >
      <span className="settings-toggle__dot" />
    </span>
  );
}
