import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Local-first data (SQLite via Tauri commands, not a remote API) —
      // staleness is driven by explicit invalidation after mutations/sync,
      // not by time, so retries/refetch-on-focus would just add latency
      // for no benefit.
      retry: false,
      refetchOnWindowFocus: false,
      staleTime: Infinity,
    },
  },
});
