import { useState } from "react";

/**
 * Shared paste-box: the desktop side of every handshake step that would be
 * a camera scan on mobile is a clipboard paste here instead (see the Faza
 * 7f plan's QR decision — desktop generates QR for a phone to scan, but
 * never scans one back). Parametrized by `onSubmit` so it's reused for
 * accept-invite and complete-invite alike.
 */
export function AliasPasteDialog({
  label,
  placeholder,
  busy,
  onSubmit,
}: {
  label: string;
  placeholder: string;
  busy: boolean;
  onSubmit: (envelopeBase64: string) => void;
}) {
  const [value, setValue] = useState("");

  return (
    <div className="settings-row--form">
      <label className="settings-section__title" style={{ marginTop: 0 }}>
        {label}
      </label>
      <textarea
        className="alias-paste__textarea"
        placeholder={placeholder}
        value={value}
        onChange={(e) => setValue(e.target.value)}
      />
      <button
        className="btn btn--secondary settings-btn-full"
        disabled={busy || !value.trim()}
        onClick={() => onSubmit(value.trim())}
      >
        {busy ? "Working…" : "Submit"}
      </button>
    </div>
  );
}
