import type { ContactProfile } from "../models";

/** Algorand addresses are exactly 58 uppercase Base32 characters ([A-Z2-7]). */
const ALGORAND_ADDRESS_RE = /^[A-Z2-7]{58}$/;

export function isValidAlgorandAddress(address: string): boolean {
  return ALGORAND_ADDRESS_RE.test(address.trim());
}

/**
 * Groups-of-4 display with the tail masked, matching the mobile Contacts
 * screen's "3728 1927 2939 xxxx xxxx xxxx" style for wallet-only contacts.
 */
export function formatWalletAddress(address: string): string {
  const groups: string[] = [];
  for (let i = 0; i < address.length; i += 4) {
    groups.push(address.slice(i, i + 4));
  }
  return groups.map((g, i) => (i < 3 ? g : "x".repeat(g.length))).join(" ");
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
  const isToday = date.toDateString() === now.toDateString();
  if (isToday) {
    return date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  }
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
