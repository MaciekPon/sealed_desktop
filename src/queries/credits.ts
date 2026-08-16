/** TanStack Query hooks for credits — mirrors `CreditsService`'s read/redeem surface. */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { credits } from "../lib/tauri";
import { queryKeys } from "./keys";

export function useCredits() {
  return useQuery({
    queryKey: queryKeys.credits(),
    queryFn: () => credits.get(),
  });
}

export function useRedeemCode() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ code, username }: { code: string; username?: string }) => credits.redeem(code, username),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.credits() }),
  });
}
