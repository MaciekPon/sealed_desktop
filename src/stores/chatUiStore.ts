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
  /** Which list `MainLayout` renders in the left panel — independent of
   * whether a contact profile is showing on the right (`viewingContactWallet`),
   * since a profile can be opened from either panel (a chat header's name,
   * or a row in the A-Z address book) and the *other* panel should keep
   * showing whatever it was showing underneath. */
  leftPanel: "chats" | "contacts";
  /** Non-null → `ContactProfile` renders in the right panel instead of
   * `ChatWindow`, regardless of `leftPanel`. */
  viewingContactWallet: string | null;
  /** Non-null → `ContactProfile` renders as a modal overlay on top of
   * whatever's already showing, instead of replacing the right panel.
   * Deliberately separate from `viewingContactWallet` — reached only from
   * the chat header's avatar/name (2026-08-24), never from the Contacts
   * address book, which keeps the existing full-panel behavior. */
  viewingContactWalletModal: string | null;
  /** The hamburger-menu navigation drawer — an overlay, not part of the
   * left/right panel split, since it renders on top of whichever panels
   * are underneath. Only Settings renders *inside* this drawer (per the
   * design mockup — the drawer's own panel swaps to Settings content
   * instead of the whole app navigating away). */
  navDrawerOpen: boolean;
  navDrawerMode: "nav" | "settings";
  selectContact: (walletAddress: string) => void;
  selectAliasContact: (contactId: string) => void;
  selectIncomingInvite: (inviteRef: string) => void;
  clearSelection: () => void;
  openContactProfile: (walletAddress: string) => void;
  closeContactProfile: () => void;
  openContactProfileModal: (walletAddress: string) => void;
  closeContactProfileModal: () => void;
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
  leftPanel: "chats",
  viewingContactWallet: null,
  viewingContactWalletModal: null,
  navDrawerOpen: false,
  navDrawerMode: "nav",
  selectContact: (walletAddress) =>
    set({
      selectedWallet: walletAddress,
      selectedAliasContactId: null,
      selectedIncomingInviteRef: null,
      leftPanel: "chats",
      viewingContactWallet: null,
      viewingContactWalletModal: null,
    }),
  selectAliasContact: (contactId) =>
    set({
      selectedAliasContactId: contactId,
      selectedWallet: null,
      selectedIncomingInviteRef: null,
      leftPanel: "chats",
      viewingContactWallet: null,
      viewingContactWalletModal: null,
    }),
  selectIncomingInvite: (inviteRef) =>
    set({
      selectedIncomingInviteRef: inviteRef,
      selectedWallet: null,
      selectedAliasContactId: null,
      leftPanel: "chats",
      viewingContactWallet: null,
      viewingContactWalletModal: null,
    }),
  clearSelection: () => set({ selectedWallet: null, selectedAliasContactId: null, selectedIncomingInviteRef: null }),
  // Opening a profile never touches `leftPanel` — it can be reached from a
  // chat header's name (leftPanel stays "chats") or an address-book row
  // (leftPanel stays "contacts"); closing it should return to whichever
  // panel was already showing underneath.
  openContactProfile: (walletAddress) => set({ viewingContactWallet: walletAddress }),
  closeContactProfile: () => set({ viewingContactWallet: null }),
  openContactProfileModal: (walletAddress) => set({ viewingContactWalletModal: walletAddress }),
  closeContactProfileModal: () => set({ viewingContactWalletModal: null }),
  openContactsList: () =>
    set({
      leftPanel: "contacts",
      selectedWallet: null,
      selectedAliasContactId: null,
      selectedIncomingInviteRef: null,
      viewingContactWallet: null,
      viewingContactWalletModal: null,
    }),
  goToChats: () =>
    set({
      leftPanel: "chats",
      viewingContactWallet: null,
      viewingContactWalletModal: null,
      selectedWallet: null,
      selectedAliasContactId: null,
      selectedIncomingInviteRef: null,
    }),
  openNavDrawer: () => set({ navDrawerOpen: true, navDrawerMode: "nav" }),
  closeNavDrawer: () => set({ navDrawerOpen: false }),
  setNavDrawerMode: (mode) => set({ navDrawerMode: mode }),
}));
