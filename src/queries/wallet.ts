/** TanStack Query hook for the wallet balance. */

import { useQuery } from "@tanstack/react-query";
import { wallet } from "../lib/tauri";
import { queryKeys } from "./keys";

export function useWalletBalance() {
  return useQuery({
    queryKey: queryKeys.walletBalance(),
    queryFn: () => wallet.getBalance(),
  });
}
