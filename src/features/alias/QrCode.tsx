import { useEffect, useRef } from "react";
import QRCode from "qrcode";
import "./alias.css";

/**
 * Thin canvas wrapper around `qrcode`'s generator — display only, no
 * camera/scanning. Used by `SettingsScreen`'s wallet-address reveal.
 */
export function QrCode({ value, size = 220 }: { value: string; size?: number }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!canvasRef.current) return;
    QRCode.toCanvas(canvasRef.current, value, { width: size, margin: 1 }).catch((e) => {
      console.error("[QrCode] failed to render", e);
    });
  }, [value, size]);

  return <canvas ref={canvasRef} className="alias-qr__canvas" width={size} height={size} />;
}
