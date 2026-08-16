/** TanStack Query hooks for username claim/release. */

import { useMutation } from "@tanstack/react-query";
import { username } from "../lib/tauri";

export function useClaimUsername() {
  return useMutation({
    mutationFn: ({ name, oldName }: { name: string; oldName?: string }) => username.claim(name, oldName),
  });
}

export function useReleaseUsername() {
  return useMutation({
    mutationFn: (oldName?: string) => username.release(oldName),
  });
}
