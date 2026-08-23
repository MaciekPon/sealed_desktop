/** TanStack Query hooks for username claim/release. */

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { username } from "../lib/tauri";
import { useSessionStore } from "../stores/sessionStore";
import type { ContactKeys } from "../models";
import { queryKeys } from "./keys";

/**
 * `useResolvedUsername` (queries/contacts.ts) caches the on-chain username
 * for any wallet forever (this project's global `staleTime: Infinity`),
 * including your own — if it ever resolved "no username" for your own
 * wallet before you claimed one (e.g. on first unlock), that stale `null`
 * sticks around permanently unless something invalidates it. Claim/release
 * are exactly the two actions that change what's true on-chain for your
 * own wallet, so both invalidate the whole `resolvedUsername` cache here
 * (a broad, prefix-matching invalidateQueries — cheap, and correctness
 * matters more than avoiding one extra refetch for a claim/release action
 * that already costs a real on-chain transaction).
 */
function invalidateResolvedUsernames(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: ["resolvedUsername"] });
}

/**
 * Writes `username` directly into the caller's own cached `ContactKeys`
 * entry (same query key `useResolvedUsername`/`useResolvedBio` both read)
 * so the new value appears the instant the mutation resolves, instead of
 * showing the old one for the extra IPC round trip an invalidate-then-
 * refetch would otherwise cost — same fix as `useSetBio` in `queries/bio.ts`,
 * see its comment for the full reasoning.
 */
function setOwnCachedUsername(queryClient: ReturnType<typeof useQueryClient>, walletAddress: string | undefined, value: string | null) {
  if (!walletAddress) return;
  queryClient.setQueryData<ContactKeys>(queryKeys.resolvedUsername(walletAddress), (old) => (old ? { ...old, username: value } : old));
}

export function useClaimUsername() {
  const queryClient = useQueryClient();
  const walletAddress = useSessionStore((s) => s.account?.walletAddress);
  return useMutation({
    mutationFn: ({ name, oldName }: { name: string; oldName?: string }) => username.claim(name, oldName),
    onSuccess: (_txId, variables) => {
      setOwnCachedUsername(queryClient, walletAddress, variables.name.trim().toLowerCase());
      invalidateResolvedUsernames(queryClient);
    },
  });
}

export function useReleaseUsername() {
  const queryClient = useQueryClient();
  const walletAddress = useSessionStore((s) => s.account?.walletAddress);
  return useMutation({
    mutationFn: (oldName?: string) => username.release(oldName),
    onSuccess: () => {
      setOwnCachedUsername(queryClient, walletAddress, null);
      invalidateResolvedUsernames(queryClient);
    },
  });
}
