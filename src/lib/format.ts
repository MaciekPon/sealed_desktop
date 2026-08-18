import type { ContactProfile } from "../models";

/** Algorand addresses are exactly 58 uppercase Base32 characters ([A-Z2-7]). */
const ALGORAND_ADDRESS_RE = /^[A-Z2-7]{58}$/;

export function isValidAlgorandAddress(address: string): boolean {
  return ALGORAND_ADDRESS_RE.test(address.trim());
}

/** First 5 / last 5 characters, e.g. "ABCDE•••••VWXYZ" — compact single-line display for wallet-only contacts, sidebar rows, and headers. */
export function formatWalletAddress(address: string): string {
  if (address.length <= 10) return address;
  return `${address.slice(0, 5)}•••••${address.slice(-5)}`;
}

export function truncateWalletAddress(address: string): string {
  if (address.length <= 12) return address;
  return `${address.slice(0, 6)}...${address.slice(-4)}`;
}

export function initials(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return "?";
  const parts = trimmed.split(/\s+/);
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

/** Deterministic pastel-on-dark avatar color, keyed by wallet address so it's stable across renders/sessions. */
export function avatarColor(seed: string): string {
  let hash = 0;
  for (let i = 0; i < seed.length; i++) {
    hash = (hash * 31 + seed.charCodeAt(i)) >>> 0;
  }
  const hue = hash % 360;
  return `hsl(${hue}, 45%, 42%)`;
}

export interface ContactGroup {
  label: string;
  contacts: ContactProfile[];
}

/**
 * Groups contacts by the first letter of their username, A-Z, with a
 * trailing "Unnamed" bucket for wallet-only contacts — mirrors the mobile
 * Contacts screen's section layout exactly.
 */
export function groupContacts(contacts: ContactProfile[]): ContactGroup[] {
  const byLetter = new Map<string, ContactProfile[]>();
  const unnamed: ContactProfile[] = [];

  for (const contact of contacts) {
    if (!contact.username) {
      unnamed.push(contact);
      continue;
    }
    const letter = contact.username[0]?.toUpperCase() ?? "#";
    const bucket = byLetter.get(letter) ?? [];
    bucket.push(contact);
    byLetter.set(letter, bucket);
  }

  const groups: ContactGroup[] = [...byLetter.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([label, list]) => ({
      label,
      contacts: list.sort((a, b) => (a.username ?? "").localeCompare(b.username ?? "")),
    }));

  if (unnamed.length > 0) {
    groups.push({ label: "Unnamed", contacts: unnamed.sort((a, b) => a.walletAddress.localeCompare(b.walletAddress)) });
  }

  return groups;
}

/** microAlgos -> "12.345678 ALGO", trailing zeros trimmed. */
export function formatAlgoBalance(microAlgos: number): string {
  const algo = microAlgos / 1_000_000;
  const text = algo.toFixed(6).replace(/0+$/, "").replace(/\.$/, "");
  return `${text} ALGO`;
}

export function formatMessageTimestamp(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  const now = new Date();
  if (date.toDateString() === now.toDateString()) {
    return date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (date.toDateString() === yesterday.toDateString()) {
    return "Yesterday";
  }
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
