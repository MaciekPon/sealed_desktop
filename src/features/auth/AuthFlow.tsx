import { useState } from "react";
import { useSessionStore } from "../../stores/sessionStore";
import { PinPad } from "./PinPad";
import "./auth.css";

type Step = "choice" | "restoreMnemonic" | "setPin" | "confirmPin" | "backupMnemonic" | "working";

/** First-run flow: create a new wallet or restore from a recovery phrase, then set a PIN. Mirrors `wallet_setup.dart` + `pin_setup_screen.dart`, collapsed into one mandatory-PIN-at-creation flow (desktop has no pre-PIN phase — see `dek/mod.rs`'s doc comment). */
export function AuthFlow() {
  const [step, setStep] = useState<Step>("choice");
  const [mode, setMode] = useState<"create" | "restore">("create");
  const [mnemonic, setMnemonic] = useState("");
  const [firstPin, setFirstPin] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [resetToken, setResetToken] = useState(0);

  const createAccount = useSessionStore((s) => s.createAccount);
  const restoreAccount = useSessionStore((s) => s.restoreAccount);
  const pendingMnemonic = useSessionStore((s) => s.pendingMnemonic);
  const clearPendingMnemonic = useSessionStore((s) => s.clearPendingMnemonic);

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

  if (step === "choice") {
    return (
      <div className="auth-screen">
        <div className="auth-screen__logo">Sealed</div>
        <p style={{ color: "var(--color-text-secondary)", maxWidth: 320 }}>
          End-to-end encrypted, quantum-resistant messaging on Algorand.
        </p>
        <div className="auth-screen__actions">
          <button
            className="btn btn--primary"
            onClick={() => {
              setMode("create");
              setStep("setPin");
            }}
          >
            Create New Wallet
          </button>
          <button
            className="btn btn--secondary"
            onClick={() => {
              setMode("restore");
              setStep("restoreMnemonic");
            }}
          >
            Restore From Recovery Phrase
          </button>
        </div>
      </div>
    );
  }

  if (step === "restoreMnemonic") {
    return (
      <div className="auth-screen">
        <h1 className="pin-pad__headline">Enter your recovery phrase</h1>
        <p className="pin-pad__subhead">12 or 24 words, separated by spaces, in the original order.</p>
        <textarea
          className="auth-screen__textarea"
          value={mnemonic}
          onChange={(e) => setMnemonic(e.target.value)}
          placeholder="word1 word2 word3 ..."
          autoFocus
        />
        {error && <p className="pin-pad__error">{error}</p>}
        <div className="auth-screen__actions">
          <button
            className="btn btn--primary"
            disabled={![12, 24].includes(mnemonic.trim().split(/\s+/).filter(Boolean).length)}
            onClick={() => {
              setError(null);
              setStep("setPin");
            }}
          >
            Continue
          </button>
          <button className="btn btn--text" onClick={() => setStep("choice")}>
            Back
          </button>
        </div>
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
        onBack={() => setStep(mode === "create" ? "choice" : "restoreMnemonic")}
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

  // backupMnemonic
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
      <div className="auth-screen__actions">
        <button className="btn btn--primary" onClick={clearPendingMnemonic}>
          I've saved it
        </button>
      </div>
    </div>
  );
}
