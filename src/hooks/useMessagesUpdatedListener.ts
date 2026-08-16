import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { queryKeys } from "../queries/keys";
import { isTauri } from "../lib/tauriEnv";

/**
 * Listens for the `messages-updated` event the Rust background sync task
 * (`sync/mod.rs`) fires after a tick that cached at least one new message
 * — wallet DM or alias-chat alike, `commands::messaging::sync_messages`
 * sums both counts — and invalidates the queries that would show it, so
 * the UI picks up messages that arrived while the app was idle, without
 * the frontend having to poll the database itself.
 *
 * **Bug found and fixed 2026-08-10**: this listener never invalidated
 * `aliasContacts`/`aliasPendingInvites`/`aliasIncomingInvites` — so when a
 * background sync silently auto-completes an alias handshake (Phase 7h's
 * `alias::invite_delivery::handle_incoming_accept`, called from inside
 * `sync_incoming_messages`) or records a freshly-arrived incoming invite,
 * the backend's data changes correctly but the sidebar's Alias tab and
 * `ContactProfile`'s duplicate-invite guard kept showing stale (often
 * empty) state until something else happened to trigger a refetch — in
 * practice, only a full app restart. This is exactly what made
 * `create_invite_for_contact`'s "an alias chat with this contact already
 * exists" error look wrong: the backend was correctly seeing an
 * already-established contact the stale frontend cache didn't know about
 * yet.
 */
export function useMessagesUpdatedListener() {
  const queryClient = useQueryClient();

  useEffect(() => {
    // Outside a real Tauri webview (the mock-backend preview path — see
    // `lib/tauriEnv.ts`) there's no IPC bridge for `listen` to attach to.
    if (!isTauri) return;

    const unlistenPromise = listen("messages-updated", () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.conversations() });
      queryClient.invalidateQueries({ predicate: (q) => q.queryKey[0] === "conversation" || q.queryKey[0] === "unreadCount" });
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasConversations() });
      queryClient.invalidateQueries({ predicate: (q) => q.queryKey[0] === "aliasConversation" });
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasContacts() });
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasPendingInvites() });
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasIncomingInvites() });
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [queryClient]);
}
