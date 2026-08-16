import { useEffect, useState } from "react";
import "./auth.css";

interface PinPadProps {
  headline: string;
  subhead?: string;
  length?: number;
  errorText?: string | null;
  disabled?: boolean;
  loading?: boolean;
  onComplete: (pin: string) => void;
  onBack?: () => void;
  /** Bump this to force-clear the entry buffer (e.g. after a rejected PIN). */
  resetToken?: number;
}

/** Numeric keypad + dot indicators, shared by every PIN-entry screen (setup, confirm, unlock, change-PIN, termination code). */
export function PinPad({ headline, subhead, length = 6, errorText, disabled, loading, onComplete, onBack, resetToken }: PinPadProps) {
  const [entry, setEntry] = useState("");
  const locked = disabled || loading;

  useEffect(() => {
    setEntry("");
  }, [resetToken]);

  // Functional updates throughout so this effect doesn't need `entry` as a
  // dependency — it only needs to resubscribe when the things that gate or
  // change behavior change, not on every keystroke.
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (locked) return;
      if (e.key >= "0" && e.key <= "9") {
        setEntry((prev) => {
          if (prev.length >= length) return prev;
          const next = prev + e.key;
          if (next.length === length) {
            onComplete(next);
            return "";
          }
          return next;
        });
      } else if (e.key === "Backspace") {
        setEntry((prev) => prev.slice(0, -1));
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [locked, length, onComplete]);

  function pressDigit(digit: string) {
    if (locked || entry.length >= length) return;
    const next = entry + digit;
    setEntry(next);
    if (next.length === length) {
      onComplete(next);
      setEntry("");
    }
  }

  function backspace() {
    if (locked) return;
    setEntry((e) => e.slice(0, -1));
  }

  return (
    <div className="pin-pad">
      {onBack && (
        <button className="pin-pad__back" onClick={onBack} disabled={locked} aria-label="Back">
          ←
        </button>
      )}
      <h1 className="pin-pad__headline">{headline}</h1>
      {subhead && <p className="pin-pad__subhead">{subhead}</p>}

      <div className="pin-pad__dots" role={loading ? "status" : undefined} aria-label={loading ? "Working…" : undefined}>
        {Array.from({ length }).map((_, i) => (
          <span
            key={i}
            className={`pin-pad__dot ${!loading && i < entry.length ? "pin-pad__dot--filled" : ""} ${loading ? "pin-pad__dot--loading" : ""}`}
            style={loading ? { animationDelay: `${i * 0.12}s` } : undefined}
          />
        ))}
      </div>

      {errorText && <p className="pin-pad__error">{errorText}</p>}

      <div className="pin-pad__keys">
        {["1", "2", "3", "4", "5", "6", "7", "8", "9"].map((d) => (
          <button key={d} className="pin-pad__key" onClick={() => pressDigit(d)} disabled={locked}>
            {d}
          </button>
        ))}
        <span />
        <button className="pin-pad__key" onClick={() => pressDigit("0")} disabled={locked}>
          0
        </button>
        <button className="pin-pad__key pin-pad__key--backspace" onClick={backspace} disabled={locked || entry.length === 0}>
          ⌫
        </button>
      </div>
    </div>
  );
}
