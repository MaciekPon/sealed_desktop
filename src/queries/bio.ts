/** TanStack Query hook for setting/clearing the caller's on-chain bio. */

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { bio } from "../lib/tauri";
import { useSessionStore } from "../stores/sessionStore";
import type { ContactKeys } from "../models";
import { queryKeys } from "./keys";

export function useSetBio() {
  const queryClient = useQueryClient();
  const walletAddress = useSessionStore((s) => s.account?.walletAddress);
  return useMutation({
    mutationFn: (text: string) => bio.set(text),
    // `useResolvedBio`/`useResolvedUsername` cache this wallet's profile
    // forever (`staleTime: Infinity`) under the shared `resolvedUsername`
    // query key. Merely invalidating leaves a visible gap — the on-chain
    // write already blocked on confirmation (`set_bio`'s Rust side), but
    // the *frontend* still needs a fresh IPC round trip to re-resolve
    // after invalidation, so the old bio would flash for a moment before
    // the refetch lands. Writing the new value into the cache directly
    // (same wallet, same query key both hooks read) makes it appear the
    // instant the mutation resolves; the invalidate below still runs as a
    // background reconciliation in case anything else about the profile
    // changed too.
    onSuccess: (_txId, text) => {
      if (walletAddress) {
        queryClient.setQueryData<ContactKeys>(queryKeys.resolvedUsername(walletAddress), (old) =>
          old ? { ...old, bio: text.trim() || null } : old,
        );
      }
      queryClient.invalidateQueries({ queryKey: ["resolvedUsername"] });
    },
  });
}
