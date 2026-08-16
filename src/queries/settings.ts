/** TanStack Query hooks for the settings surface not already owned by `stores/settingsStore.ts` (auto-sync). */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { settings } from "../lib/tauri";
import { queryKeys } from "./keys";

export function useIsTerminationConfigured() {
  return useQuery({
    queryKey: queryKeys.terminationConfigured(),
    queryFn: () => settings.isTerminationConfigured(),
  });
}

export function useSetTerminationCode() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (code: string) => settings.setTerminationCode(code),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.terminationConfigured() }),
  });
}

export function useDisableTerminationCode() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => settings.disableTerminationCode(),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.terminationConfigured() }),
  });
}
