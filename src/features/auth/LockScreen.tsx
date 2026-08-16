import { useState } from "react";
import { useSessionStore } from "../../stores/sessionStore";
import { PinPad } from "./PinPad";
import "./auth.css";

/**
 * Mirrors `lock_screen.dart`: a wrong PIN and a termination-code match are
 * both handled by the same backend call (`unlock_account`) and must look
 * identical to the user — the duress-safety point of a termination code
 * is defeated if the UI can distinguish "wiped because 5 wrong PINs" from
 * "wiped because you typed the duress code". Both surface the same
 * generic "Incorrect PIN" text here.
 */
export function LockScreen() {
  const unlock = useSessionStore((s) => s.unlock);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [resetToken, setResetToken] = useState(0);

  async function handleComplete(pin: string) {
    setBusy(true);
    setError(null);
    try {
      const outcome = await unlock(pin);
      if (outcome === "wrongPin") {
        setError("Incorrect PIN");
        setResetToken((t) => t + 1);
      } else if (outcome === "wiped") {
        // Same visible message as a wrong PIN — see the doc comment above.
        setError("Incorrect PIN");
        setResetToken((t) => t + 1);
      }
      // "success" — sessionStore flips to "unlocked", App.tsx re-renders.
    } catch (e) {
      setError(String(e));
      setResetToken((t) => t + 1);
    } finally {
      setBusy(false);
    }
  }

  return (
    <PinPad
      headline="Enter your PIN"
      subhead="Sealed"
      errorText={error}
      disabled={busy}
      loading={busy}
      resetToken={resetToken}
      onComplete={handleComplete}
    />
  );
}
