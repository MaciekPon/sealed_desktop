import { create } from "zustand";

interface ChatUiState {
  selectedWallet: string | null;
  /** Alias-chat contact selection — kept separate from `selectedWallet`
   * rather than folded into a discriminated union, so the existing,
   * stable wallet-chat call sites (`ContactsSidebar`/`ChatWindow`) don't
   * need renaming. Selecting one clears the other. */
  selectedAliasContactId: string | null;
  /** A not-yet-accepted incoming alias invite, opened in the chat window so
   * Accept/Decline can live where the composer normally does. Mutually
   * exclusive with `selectedWallet`/`selectedAliasContactId`. */
  selectedIncomingInviteRef: string | null;
  sidebarCollapsed: boolean;
  /** Which top-level screen `MainLayout` renders — chat is the default. */
  screen: "chat" | "settings" | "contactProfile" | "alias";
  /** Set only while `screen === "contactProfile"`. */
  viewingContactWallet: string | null;
  selectContact: (walletAddress: string) => void;
  selectAliasContact: (contactId: string) => void;
  selectIncomingInvite: (inviteRef: string) => void;
  clearSelection: () => void;
  toggleSidebar: () => void;
  openSettings: () => void;
  closeSettings: () => void;
  openContactProfile: (walletAddress: string) => void;
  closeContactProfile: () => void;
  openAlias: () => void;
  closeAlias: () => void;
}

export const useChatUiStore = create<ChatUiState>((set) => ({
  selectedWallet: null,
  selectedAliasContactId: null,
  selectedIncomingInviteRef: null,
  sidebarCollapsed: false,
  screen: "chat",
  viewingContactWallet: null,
  selectContact: (walletAddress) => set({ selectedWallet: walletAddress, selectedAliasContactId: null, selectedIncomingInviteRef: null, screen: "chat" }),
  selectAliasContact: (contactId) => set({ selectedAliasContactId: contactId, selectedWallet: null, selectedIncomingInviteRef: null, screen: "chat" }),
  selectIncomingInvite: (inviteRef) => set({ selectedIncomingInviteRef: inviteRef, selectedWallet: null, selectedAliasContactId: null, screen: "chat" }),
  clearSelection: () => set({ selectedWallet: null, selectedAliasContactId: null, selectedIncomingInviteRef: null }),
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  openSettings: () => set({ screen: "settings" }),
  closeSettings: () => set({ screen: "chat" }),
  openContactProfile: (walletAddress) => set({ screen: "contactProfile", viewingContactWallet: walletAddress }),
  closeContactProfile: () => set({ screen: "chat", viewingContactWallet: null }),
  openAlias: () => set({ screen: "alias" }),
  closeAlias: () => set({ screen: "chat" }),
}));
