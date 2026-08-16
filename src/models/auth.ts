// Mirrors sealed-desktop/src-tauri/src/commands/auth.rs's serde types.

export interface NewAccountInfo {
  walletAddress: string;
  /** Shown once at creation/restore time so the user can back it up. */
  mnemonic: string;
}

export interface AccountInfo {
  walletAddress: string;
  /** base64 */
  encryptionPubkey: string;
  /** base64 */
  scanPubkey: string;
}

/**
 * Mirrors `commands::auth::UnlockOutcome` — a PIN attempt is either a
 * success, a wrong PIN with attempts remaining before the device wipes, or
 * the device having just been wiped (duress termination code match, or the
 * 5th consecutive wrong PIN — both report the same `wiped` variant on
 * purpose, see the Rust doc comment on `UnlockOutcome`).
 */
export type UnlockOutcome =
  | ({ type: "success" } & AccountInfo)
  | { type: "wrongPin"; attemptsRemaining: number }
  | { type: "wiped" };
