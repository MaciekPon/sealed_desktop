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
  /** Which top-level screen `MainLayout` renders — chat is the default.
   * `contactsList` is the A-Z address-book view reached from the nav
   * drawer's "Contacts" item (distinct from `contactProfile`, which is one
   * specific contact's detail screen). */
  screen: "chat" | "contactProfile" | "contactsList";
  /** Set only while `screen === "contactProfile"`. */
  viewingContactWallet: string | null;
  /** The hamburger-menu navigation drawer — an overlay, not a `screen`
   * value, since it renders on top of whichever screen is underneath.
   * Only Settings renders *inside* this drawer (per the design mockup —
   * the drawer's own panel swaps to Settings content instead of the whole
   * app navigating away); Contacts is a normal full-width `screen` like
   * everything else, reached by closing the drawer and switching screens. */
  navDrawerOpen: boolean;
  navDrawerMode: "nav" | "settings";
  selectContact: (walletAddress: string) => void;
  selectAliasContact: (contactId: string) => void;
  selectIncomingInvite: (inviteRef: string) => void;
  clearSelection: () => void;
  openContactProfile: (walletAddress: string) => void;
  closeContactProfile: () => void;
  openContactsList: () => void;
  goToChats: () => void;
  openNavDrawer: () => void;
  closeNavDrawer: () => void;
  setNavDrawerMode: (mode: "nav" | "settings") => void;
}

export const useChatUiStore = create<ChatUiState>((set) => ({
  selectedWallet: null,
  selectedAliasContactId: null,
  selectedIncomingInviteRef: null,
  screen: "chat",
  viewingContactWallet: null,
  navDrawerOpen: false,
  navDrawerMode: "nav",
  selectContact: (walletAddress) => set({ selectedWallet: walletAddress, selectedAliasContactId: null, selectedIncomingInviteRef: null, screen: "chat" }),
  selectAliasContact: (contactId) => set({ selectedAliasContactId: contactId, selectedWallet: null, selectedIncomingInviteRef: null, screen: "chat" }),
  selectIncomingInvite: (inviteRef) => set({ selectedIncomingInviteRef: inviteRef, selectedWallet: null, selectedAliasContactId: null, screen: "chat" }),
  clearSelection: () => set({ selectedWallet: null, selectedAliasContactId: null, selectedIncomingInviteRef: null }),
  openContactProfile: (walletAddress) => set({ screen: "contactProfile", viewingContactWallet: walletAddress }),
  closeContactProfile: () => set({ screen: "chat", viewingContactWallet: null }),
  openContactsList: () => set({ screen: "contactsList" }),
  goToChats: () => set({ screen: "chat" }),
  openNavDrawer: () => set({ navDrawerOpen: true, navDrawerMode: "nav" }),
  closeNavDrawer: () => set({ navDrawerOpen: false }),
  setNavDrawerMode: (mode) => set({ navDrawerMode: mode }),
}));
