/** TanStack Query hooks for the contacts cache — mirrors `ContactRepository`'s read surface. */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { contacts } from "../lib/tauri";
import type { ContactProfile } from "../models";
import { queryKeys } from "./keys";

export function useContacts() {
  return useQuery({
    queryKey: queryKeys.contacts(),
    queryFn: () => contacts.getAll(),
  });
}

export function useContact(walletAddress: string | null) {
  return useQuery({
    queryKey: queryKeys.contact(walletAddress ?? ""),
    queryFn: () => contacts.get(walletAddress as string),
    enabled: walletAddress !== null,
  });
}

export function useContactSearch(query: string) {
  return useQuery({
    queryKey: queryKeys.contactSearch(query),
    queryFn: () => contacts.searchMany(query, 20),
    enabled: query.trim().length > 0,
  });
}

export function useSaveContact() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (profile: ContactProfile) => contacts.save(profile),
    onSuccess: (_void, profile) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.contacts() });
      queryClient.invalidateQueries({ queryKey: queryKeys.contact(profile.walletAddress) });
    },
  });
}

export function useDeleteContact() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (walletAddress: string) => contacts.delete(walletAddress),
    onSuccess: (_void, walletAddress) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.contacts() });
      queryClient.removeQueries({ queryKey: queryKeys.contact(walletAddress) });
    },
  });
}

/** Resolve (and optionally cache) a contact's keys before sending a first message to them. */
export function useResolveContactKeys() {
  return useMutation({
    mutationFn: (walletAddress: string) => contacts.resolveKeys(walletAddress),
  });
}

function useContactFlagMutation(mutationFn: (walletAddress: string) => Promise<void>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn,
    onSuccess: (_void, walletAddress) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.contacts() });
      queryClient.invalidateQueries({ queryKey: queryKeys.contact(walletAddress) });
    },
  });
}

export function useAddToContacts() {
  return useContactFlagMutation((walletAddress) => contacts.addToContacts(walletAddress));
}

export function useRemoveFromContacts() {
  return useContactFlagMutation((walletAddress) => contacts.removeFromContacts(walletAddress));
}

export function useBlockContact() {
  return useContactFlagMutation((walletAddress) => contacts.block(walletAddress));
}

export function useUnblockContact() {
  return useContactFlagMutation((walletAddress) => contacts.unblock(walletAddress));
}
