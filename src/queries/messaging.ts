/**
 * TanStack Query hooks for the messaging domain — mirrors what
 * `messagesNotifierProvider` gives the UI on mobile (conversation list,
 * single conversation, send/sync/mark-read/force-resync actions), just
 * expressed as query/mutation hooks instead of a Riverpod notifier.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { messaging } from "../lib/tauri";
import { queryKeys } from "./keys";

export function useConversations() {
  return useQuery({
    queryKey: queryKeys.conversations(),
    queryFn: () => messaging.getAllConversations(),
  });
}

export function useConversation(contactWallet: string | null) {
  return useQuery({
    queryKey: queryKeys.conversation(contactWallet ?? ""),
    queryFn: () => messaging.getConversation(contactWallet as string),
    enabled: contactWallet !== null,
  });
}

export function useUnreadCount(contactWallet: string | null) {
  return useQuery({
    queryKey: queryKeys.unreadCount(contactWallet ?? ""),
    queryFn: () => messaging.getUnreadCount(contactWallet as string),
    enabled: contactWallet !== null,
  });
}

/** Invalidate everything a successful send/sync/mark-read/resync could have changed. */
function invalidateConversations(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: queryKeys.conversations() });
}

/**
 * **Bug fixed 2026-08-11**: `useSyncMessages`/`useForceResync` only ever
 * invalidated the regular wallet-DM conversation list — never
 * `aliasContacts`/`aliasPendingInvites`/`aliasIncomingInvites`. A sync pass
 * can silently complete an alias handshake server-side (Phase 7h's
 * `handle_incoming_accept`, running inside the same sync), but the Alias
 * tab and `ContactProfile`'s duplicate-invite guard never found out unless
 * the *background* tick's `messages-updated` event listener happened to
 * fire first (that path was already fixed separately) — clicking "Sync
 * now"/"Force resync" by hand never refreshed them at all. A user hit this
 * live: an alias contact was confirmed (via server-side log inspection) to
 * have been created successfully, but never appeared in the UI no matter
 * how many times they clicked Sync now.
 */
function invalidateAliasState(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: queryKeys.aliasContacts() });
  queryClient.invalidateQueries({ queryKey: queryKeys.aliasPendingInvites() });
  queryClient.invalidateQueries({ queryKey: queryKeys.aliasIncomingInvites() });
  queryClient.invalidateQueries({ queryKey: queryKeys.aliasConversations() });
  queryClient.invalidateQueries({ predicate: (q) => q.queryKey[0] === "aliasConversation" });
}

export function useSendMessage() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { recipientWallet: string; plaintext: string; recipientUsername?: string }) =>
      messaging.send(args.recipientWallet, args.plaintext, args.recipientUsername),
    onSuccess: (_txId, variables) => {
      invalidateConversations(queryClient);
      queryClient.invalidateQueries({ queryKey: queryKeys.conversation(variables.recipientWallet) });
    },
  });
}

export function useSyncMessages() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (fullSync: boolean = false) => messaging.sync(fullSync),
    // **Bug fixed 2026-08-11**: gating alias invalidation on `newCount > 0`
    // (as originally written just above, for `invalidateConversations`)
    // doesn't work for alias state — `sync_incoming_messages`'s Rust side
    // `continue`s immediately after processing a classified alias
    // invite/accept envelope, *before* the `new_count += 1` that regular
    // messages hit. So a sync pass that silently completes an alias
    // handshake always reports `newCount === 0` ("nothing new"), and the
    // old `if newCount > 0` guard skipped alias invalidation exactly when
    // it mattered most. Alias invalidation now always runs — cheap (just
    // marks queries stale for currently-mounted subscribers to refetch),
    // unlike `invalidateConversations`, which stays gated since regular
    // messages *do* count correctly.
    onSuccess: (newCount) => {
      if (newCount > 0) invalidateConversations(queryClient);
      invalidateAliasState(queryClient);
    },
  });
}

export function useForceResync() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => messaging.forceResync(),
    onSuccess: () => {
      invalidateConversations(queryClient);
      invalidateAliasState(queryClient);
    },
  });
}

export function useMarkConversationAsRead() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (contactWallet: string) => messaging.markConversationAsRead(contactWallet),
    onSuccess: (_void, contactWallet) => {
      invalidateConversations(queryClient);
      queryClient.invalidateQueries({ queryKey: queryKeys.unreadCount(contactWallet) });
    },
  });
}

export function useDeleteConversation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (contactWallet: string) => messaging.deleteConversation(contactWallet),
    onSuccess: (_count, contactWallet) => {
      invalidateConversations(queryClient);
      queryClient.removeQueries({ queryKey: queryKeys.conversation(contactWallet) });
    },
  });
}
