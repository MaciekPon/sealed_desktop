/**
 * Session/auth state — mirrors mobile's `pinSessionProvider` +
 * `localWalletProvider` combined (there's no separate "wallet loaded but
 * locked" phase on desktop: the vault holds the wallet, and the vault only
 * opens with the PIN — see `dek/mod.rs`'s module doc comment on why
 * desktop skips mobile's pre-PIN device-wrapped phase entirely).
 */

import { create } from "zustand";
import { auth, keys, settings } from "../lib/tauri";
import type { AccountInfo } from "../models";
import { queryClient } from "../queries/queryClient";

/** Fire-and-forget: never blocks or fails the caller's flow. */
function fireEnsureKeysPublished() {
  keys.ensurePublished().catch((err) => console.warn("[sessionStore] ensureKeysPublished failed", err));
}

export type SessionStatus =
  /** Haven't checked yet — app just started. */
  | "unknown"
  /** No account exists on this device at all — show create/restore flow. */
  | "noAccount"
  /** Account exists but the vault is locked — show the PIN entry screen. */
  | "locked"
  /** Vault unlocked, account info loaded. */
  | "unlocked";

interface SessionState {
  status: SessionStatus;
  account: AccountInfo | null;
  /** Set only right after create/restore, so the UI can prompt a backup — never persisted. */
  pendingMnemonic: string | null;
  /** Non-null right after a wrong-PIN attempt, so the lock screen can show it. */
  attemptsRemaining: number | null;

  /** Call once at app startup to decide which screen to show first. */
  bootstrap: () => Promise<void>;
  createAccount: (pin: string) => Promise<void>;
  restoreAccount: (pin: string, mnemonic: string) => Promise<void>;
  /** Returns `true` on success. On `wiped`, the caller should navigate back to the create/restore flow. */
  unlock: (pin: string) => Promise<"success" | "wrongPin" | "wiped">;
  lock: () => Promise<void>;
  changePin: (oldPin: string, newPin: string) => Promise<void>;
  clearPendingMnemonic: () => void;
  /** Irreversibly wipes the vault/db/settings (see `commands::settings::log_out`), then returns to the create/restore flow. */
  logOut: () => Promise<void>;
}

export const useSessionStore = create<SessionState>((set) => ({
  status: "unknown",
  account: null,
  pendingMnemonic: null,
  attemptsRemaining: null,

  bootstrap: async () => {
    const hasAccount = await auth.hasExistingAccount();
    if (!hasAccount) {
      set({ status: "noAccount" });
      return;
    }
    const unlocked = await auth.isUnlocked();
    set({ status: unlocked ? "unlocked" : "locked" });
  },

  createAccount: async (pin) => {
    const info = await auth.createAccount(pin);
    // **Bug fixed 2026-08-11**: `queryClient` (see `queries/queryClient.ts`)
    // is configured with `staleTime: Infinity` and no background refetch —
    // by design, for a local-first app, cached data is only ever refreshed
    // by explicit `invalidateQueries` calls after mutations/sync. Nothing
    // was clearing it on an account switch (create/restore/unlock a
    // *different* wallet than whatever was last cached), so every screen
    // kept showing the previous account's contacts/balance/settings/etc.
    // indefinitely — a user hit this live, confirmed only a full app
    // restart (which creates a fresh `QueryClient` instance) fixed it.
    queryClient.clear();
    set({
      status: "unlocked",
      account: { walletAddress: info.walletAddress, encryptionPubkey: "", scanPubkey: "" },
      pendingMnemonic: info.mnemonic,
    });
    fireEnsureKeysPublished();
  },

  restoreAccount: async (pin, mnemonic) => {
    const info = await auth.restoreAccount(pin, mnemonic);
    queryClient.clear();
    set({
      status: "unlocked",
      account: { walletAddress: info.walletAddress, encryptionPubkey: "", scanPubkey: "" },
      pendingMnemonic: null,
    });
    fireEnsureKeysPublished();
  },

  unlock: async (pin) => {
    const outcome = await auth.unlockAccount(pin);
    switch (outcome.type) {
      case "success":
        queryClient.clear();
        set({ status: "unlocked", account: outcome, attemptsRemaining: null });
        fireEnsureKeysPublished();
        return "success";
      case "wrongPin":
        set({ attemptsRemaining: outcome.attemptsRemaining });
        return "wrongPin";
      case "wiped":
        queryClient.clear();
        set({ status: "noAccount", account: null, attemptsRemaining: null });
        return "wiped";
    }
  },

  lock: async () => {
    await auth.lockAccount();
    queryClient.clear();
    set({ status: "locked", account: null });
  },

  changePin: async (oldPin, newPin) => {
    await auth.changePin(oldPin, newPin);
  },

  clearPendingMnemonic: () => set({ pendingMnemonic: null }),

  logOut: async () => {
    await settings.logOut();
    queryClient.clear();
    set({ status: "noAccount", account: null, pendingMnemonic: null, attemptsRemaining: null });
  },
}));
