/**
 * TanStack Query hooks for the alias-chat domain — local QR/paste handshake
 * (create/accept/complete invite) plus send/query/manage over established
 * alias contacts. Mirrors `queries/messaging.ts`'s shape.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { alias } from "../lib/tauri";
import { queryKeys } from "./keys";

export function usePendingInvites() {
  return useQuery({
    queryKey: queryKeys.aliasPendingInvites(),
    queryFn: () => alias.listPendingInvites(),
  });
}

export function useAliasContacts() {
  return useQuery({
    queryKey: queryKeys.aliasContacts(),
    queryFn: () => alias.getContacts(),
  });
}

export function useIncomingInvites() {
  return useQuery({
    queryKey: queryKeys.aliasIncomingInvites(),
    queryFn: () => alias.listIncomingInvites(),
  });
}

export function useAliasConversations() {
  return useQuery({
    queryKey: queryKeys.aliasConversations(),
    queryFn: () => alias.getConversations(),
  });
}

export function useAliasConversation(contactId: string | null) {
  return useQuery({
    queryKey: queryKeys.aliasConversation(contactId ?? ""),
    queryFn: () => alias.getConversation(contactId as string),
    enabled: contactId !== null,
  });
}

/** Invalidate everything a successful handshake step or send/sync could have changed. */
function invalidateAliasLists(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: queryKeys.aliasPendingInvites() });
  queryClient.invalidateQueries({ queryKey: queryKeys.aliasContacts() });
  queryClient.invalidateQueries({ queryKey: queryKeys.aliasConversations() });
}

export function useCreateInvite() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (label?: string) => alias.createInvite(label),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.aliasPendingInvites() }),
  });
}

export function useDismissPendingInvite() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (inviteRef: string) => alias.dismissPendingInvite(inviteRef),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.aliasPendingInvites() }),
  });
}

export function useDeletePendingInvite() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (inviteRef: string) => alias.deletePendingInvite(inviteRef),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.aliasPendingInvites() }),
  });
}

export function useAcceptInvite() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { envelopeBase64: string; label?: string }) => alias.acceptInvite(args.envelopeBase64, args.label),
    onSuccess: () => invalidateAliasLists(queryClient),
  });
}

export function useCompleteInvite() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (envelopeBase64: string) => alias.completeInvite(envelopeBase64),
    onSuccess: () => invalidateAliasLists(queryClient),
  });
}

export function useSendAliasMessage() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { contactId: string; plaintext: string }) => alias.sendMessage(args.contactId, args.plaintext),
    onSuccess: (_txId, variables) => {
      invalidateAliasLists(queryClient);
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasConversation(variables.contactId) });
    },
  });
}

export function useMarkAliasConversationRead() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (contactId: string) => alias.markConversationAsRead(contactId),
    onSuccess: () => invalidateAliasLists(queryClient),
  });
}

export function useRenameAliasContact() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { contactId: string; label: string }) => alias.rename(args.contactId, args.label),
    onSuccess: (_void, variables) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasContacts() });
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasConversations() });
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasContact(variables.contactId) });
    },
  });
}

export function useCreateInviteForContact() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { recipientWallet: string; label?: string }) => alias.createInviteForContact(args.recipientWallet, args.label),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.aliasPendingInvites() }),
  });
}

export function useAcceptIncomingInvite() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { inviteRef: string; label?: string }) => alias.acceptIncomingInvite(args.inviteRef, args.label),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.aliasIncomingInvites() });
      invalidateAliasLists(queryClient);
    },
  });
}

export function useDeclineIncomingInvite() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (inviteRef: string) => alias.declineIncomingInvite(inviteRef),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queryKeys.aliasIncomingInvites() }),
  });
}

export function useDeleteAliasContact() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (contactId: string) => alias.delete(contactId),
    onSuccess: (_void, contactId) => {
      invalidateAliasLists(queryClient);
      queryClient.removeQueries({ queryKey: queryKeys.aliasConversation(contactId) });
    },
  });
}
