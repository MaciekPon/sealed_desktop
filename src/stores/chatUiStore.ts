import { create } from "zustand";

interface ChatUiState {
  selectedWallet: string | null;
  /** Alias-chat contact selection — kept separate from `selectedWallet`
   * rather than folded into a discriminated union, so the existing,
   * stable wallet-chat call sites (`ContactsSidebar`/`ChatWindow`) don't
   * need renaming. Selecting one clears the other. */
  selectedAliasContactId: string | null;
  sidebarCollapsed: boolean;
  /** Which top-level screen `MainLayout` renders — chat is the default. */
  screen: "chat" | "settings" | "contactProfile" | "alias";
  /** Set only while `screen === "contactProfile"`. */
  viewingContactWallet: string | null;
  selectContact: (walletAddress: string) => void;
  selectAliasContact: (contactId: string) => void;
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
  sidebarCollapsed: false,
  screen: "chat",
  viewingContactWallet: null,
  selectContact: (walletAddress) => set({ selectedWallet: walletAddress, selectedAliasContactId: null, screen: "chat" }),
  selectAliasContact: (contactId) => set({ selectedAliasContactId: contactId, selectedWallet: null, screen: "chat" }),
  clearSelection: () => set({ selectedWallet: null, selectedAliasContactId: null }),
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  openSettings: () => set({ screen: "settings" }),
  closeSettings: () => set({ screen: "chat" }),
  openContactProfile: (walletAddress) => set({ screen: "contactProfile", viewingContactWallet: walletAddress }),
  closeContactProfile: () => set({ screen: "chat", viewingContactWallet: null }),
  openAlias: () => set({ screen: "alias" }),
  closeAlias: () => set({ screen: "chat" }),
}));
