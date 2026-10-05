import { useEffect, useState } from "react";
import { auth } from "../../lib/tauri";
import { useSessionStore } from "../../stores/sessionStore";
import { HowItWorks } from "./HowItWorks";
import { PinPad } from "./PinPad";
import { Welcome } from "./Welcome";
import "./auth.css";

type Step =
  | "welcome"
  | "howItWorks"
  | "choice"
  | "restoreMnemonic"
  | "setPin"
  | "confirmPin"
  | "backupMnemonic"
  | "working";

const CLIPBOARD_CLEAR_AFTER_MS = 60_000;

/** Same path as `assets/icons/chevron-right.svg` (Settings' list rows) — inlined here since it's the only auth-screen use and doesn't warrant a whole icon-set file for one shape. */
function ChevronIcon() {
  return (
    <svg className="choice-option__chevron" width="14" height="14" viewBox="0 0 20 20" fill="none" aria-hidden="true">
      <path d="M7.5 4.5 12.5 10l-5 5.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

/** First-run flow: welcome + how-it-works intro, then create a new wallet or restore from a recovery phrase, then set a PIN. Mirrors `welcome.dart` + `how_it_works.dart` + `wallet_setup.dart` + `pin_setup_screen.dart`, collapsed into one mandatory-PIN-at-creation flow (desktop has no pre-PIN phase — see `dek/mod.rs`'s doc comment). */
export function AuthFlow() {
  const [step, setStep] = useState<Step>("welcome");
  const [mode, setMode] = useState<"create" | "restore">("create");
  const [mnemonic, setMnemonic] = useState("");
  const [firstPin, setFirstPin] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [resetToken, setResetToken] = useState(0);
  const [obscureMnemonic, setObscureMnemonic] = useState(true);
  const [checkingCredits, setCheckingCredits] = useState(false);
  const [showNoCreditsConfirm, setShowNoCreditsConfirm] = useState(false);
  const [savedPhraseConfirmed, setSavedPhraseConfirmed] = useState(false);
  const [copyStatus, setCopyStatus] = useState<string | null>(null);

  const createAccount = useSessionStore((s) => s.createAccount);
  const restoreAccount = useSessionStore((s) => s.restoreAccount);
  const pendingMnemonic = useSessionStore((s) => s.pendingMnemonic);
  const clearPendingMnemonic = useSessionStore((s) => s.clearPendingMnemonic);

  // Auto-dismisses the copy-status hint after a few seconds — the
  // clipboard itself is separately wiped 60s after a copy (see `copySeed`
  // below), mirrors `local_account.dart`'s `_clipboardClearTimer` (a
  // copied seed phrase should not linger where another app, or a
  // clipboard-history tool, can read it later).
  useEffect(() => {
    if (!copyStatus) return;
    const t = setTimeout(() => setCopyStatus(null), 4000);
    return () => clearTimeout(t);
  }, [copyStatus]);

  async function copySeed(words: string[]) {
    const text = words.join(" ");
    await navigator.clipboard.writeText(text);
    setCopyStatus(`Copied — clears in ${CLIPBOARD_CLEAR_AFTER_MS / 1000}s. Never share it.`);
    setTimeout(async () => {
      try {
        const current = await navigator.clipboard.readText();
        if (current === text) await navigator.clipboard.writeText("");
      } catch {
        // Clipboard read can be denied by the OS/browser permission model —
        // nothing to do but leave it; not worth surfacing to the user.
      }
    }, CLIPBOARD_CLEAR_AFTER_MS);
  }

  async function handlePinConfirmed(confirmPin: string) {
    if (confirmPin !== firstPin) {
      setError("PINs don't match. Try again.");
      setFirstPin(null);
      setStep("setPin");
      setResetToken((t) => t + 1);
      return;
    }
    setStep("working");
    setError(null);
    try {
      if (mode === "create") {
        await createAccount(confirmPin);
        setStep("backupMnemonic");
      } else {
        await restoreAccount(confirmPin, mnemonic.trim());
        // Unlocked — App.tsx will switch to the main layout.
      }
    } catch (e) {
      setError(String(e));
      setFirstPin(null);
      setStep("setPin");
      setResetToken((t) => t + 1);
    }
  }

  /** Read-only credit check before ever committing the restore — see `preview_restore_credits`'s Rust doc comment for why desktop can safely check first instead of after, unlike mobile. A failed/unreachable check never blocks restore, same as mobile's fallback. */
  async function handleRestoreContinue() {
    setCheckingCredits(true);
    try {
      const credits = await auth.previewRestoreCredits(mnemonic.trim());
      setCheckingCredits(false);
      if (credits > 0) {
        setStep("setPin");
      } else {
        setShowNoCreditsConfirm(true);
      }
    } catch {
      setCheckingCredits(false);
      setStep("setPin");
    }
  }

  if (step === "welcome") {
    return <Welcome onContinue={() => setStep("howItWorks")} />;
  }

  if (step === "howItWorks") {
    return <HowItWorks onDone={() => setStep("choice")} />;
  }

  if (step === "choice") {
    return (
      <div className="auth-screen welcome-screen-bg">
        <div className="welcome-card">
          <h1 className="choice-card__title">Create an account or recover an existing one</h1>
          <p className="choice-card__subhead">Select how you want to proceed</p>
          <div className="choice-options">
            <button
              className="choice-option"
              onClick={() => {
                setMode("create");
                setStep("setPin");
              }}
            >
              <div className="choice-option__top">
                <span className="choice-option__icon">
                  <img src="/onboarding/svg/phone.svg" alt="" />
                </span>
                <ChevronIcon />
              </div>
              <div className="choice-option__text">
                <p className="choice-option__title">Create Local Wallet</p>
                <p className="choice-option__subtitle">Data stored on phone</p>
              </div>
            </button>
            <button
              className="choice-option"
              onClick={() => {
                setMode("restore");
                setStep("restoreMnemonic");
              }}
            >
              <div className="choice-option__top">
                <span className="choice-option__icon">
                  <img src="/onboarding/svg/key.svg" alt="" />
                </span>
                <ChevronIcon />
              </div>
              <div className="choice-option__text">
                <p className="choice-option__title">Restore Wallet</p>
                <p className="choice-option__subtitle">Restore with mnemonic phrase</p>
              </div>
            </button>
          </div>
        </div>
      </div>
    );
  }

  if (step === "restoreMnemonic") {
    const wordCount = mnemonic.trim().split(/\s+/).filter(Boolean).length;
    return (
      <div className="auth-screen welcome-screen-bg">
        <div className="welcome-card">
          <h1 className="choice-card__title">Mnemonic phrase account</h1>
          <p className="choice-card__subhead">Paste your private key (from Phantom, Trust wallet, etc.)</p>
          <div className="mnemonic-input-wrap">
            <input
              className="mnemonic-input"
              type={obscureMnemonic ? "password" : "text"}
              value={mnemonic}
              onChange={(e) => setMnemonic(e.target.value)}
              placeholder="word1 word2 word3 ..."
              autoFocus
              autoCorrect="off"
              spellCheck={false}
            />
            <button
              className="mnemonic-input__obscure-toggle"
              onClick={() => setObscureMnemonic((v) => !v)}
              type="button"
              aria-label={obscureMnemonic ? "Show phrase" : "Hide phrase"}
            >
              <img src={obscureMnemonic ? "/onboarding/svg/eye_closed.svg" : "/onboarding/svg/eye_open.svg"} alt="" />
            </button>
          </div>
          {error && <p className="pin-pad__error">{error}</p>}
          <button
            className="btn btn--primary welcome-card__cta"
            disabled={![12, 24].includes(wordCount) || checkingCredits}
            onClick={handleRestoreContinue}
          >
            {checkingCredits ? "Checking…" : "Create account"}
          </button>
        </div>

        {showNoCreditsConfirm && (
          <div className="auth-modal-backdrop" onClick={() => setShowNoCreditsConfirm(false)}>
            <div className="confirm-dialog" onClick={(e) => e.stopPropagation()}>
              <p className="confirm-dialog__message">
                This wallet does not have any credits. Are you sure it's your account and want to proceed?
              </p>
              <div className="confirm-dialog__actions">
                <button className="btn btn--text" onClick={() => setShowNoCreditsConfirm(false)}>
                  Cancel
                </button>
                <button
                  className="btn btn--primary"
                  onClick={() => {
                    setShowNoCreditsConfirm(false);
                    setStep("setPin");
                  }}
                >
                  Continue
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    );
  }

  if (step === "setPin") {
    return (
      <PinPad
        headline="Choose a PIN"
        subhead="This unlocks the app and encrypts everything stored on this device."
        errorText={error}
        resetToken={resetToken}
        onBack={() => setStep(mode === "create" ? "howItWorks" : "restoreMnemonic")}
        onComplete={(pin) => {
          setFirstPin(pin);
          setError(null);
          setStep("confirmPin");
        }}
      />
    );
  }

  if (step === "confirmPin") {
    return (
      <PinPad
        headline="Confirm your PIN"
        subhead="Enter it once more."
        errorText={error}
        resetToken={resetToken}
        onBack={() => setStep("setPin")}
        onComplete={handlePinConfirmed}
      />
    );
  }

  if (step === "working") {
    return (
      <div className="auth-screen">
        <p style={{ color: "var(--color-text-secondary)" }}>Setting up your wallet…</p>
      </div>
    );
  }

  // backupMnemonic — reachable only for `mode === "create"`, already fully
  // unlocked and PIN-protected by this point (see the doc comment above:
  // desktop has no pre-PIN phase, so unlike mobile's equivalent screen
  // there is nothing left to safely "discard" by going back — no back
  // button here on purpose).
  const words = (pendingMnemonic ?? "").trim().split(/\s+/);
  return (
    <div className="auth-screen">
      <h1 className="pin-pad__headline">Back up your recovery phrase</h1>
      <p className="pin-pad__subhead">
        Write these 24 words down and store them somewhere safe. This is the only way to recover your account.
      </p>
      <div className="mnemonic-grid">
        {words.map((word, i) => (
          <div className="mnemonic-grid__word" key={i}>
            <span className="mnemonic-grid__index">{i + 1}.</span>
            <span>{word}</span>
          </div>
        ))}
      </div>
      <div className="mnemonic-copy-row">
        <button className="mnemonic-copy-row__btn" onClick={() => copySeed(words)} type="button">
          Copy
        </button>
      </div>
      {copyStatus ? (
        <p className="auth-screen__hint">{copyStatus}</p>
      ) : (
        <p className="auth-screen__hint">Writing it down is safest. Copying may sync your phrase to other devices via cloud clipboard.</p>
      )}
      <button className="auth-checkbox-row" onClick={() => setSavedPhraseConfirmed((v) => !v)} type="button">
        <span className={`auth-checkbox-row__box ${savedPhraseConfirmed ? "auth-checkbox-row__box--checked" : ""}`} />
        <span className="auth-checkbox-row__label">I have saved my recovery phrase safely.</span>
      </button>
      <div className="auth-screen__actions">
        <button className="btn btn--primary" disabled={!savedPhraseConfirmed} onClick={clearPendingMnemonic}>
          I've saved it
        </button>
      </div>
    </div>
  );
}
