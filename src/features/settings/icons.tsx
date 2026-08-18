/**
 * Minimal inline SVG icon set for the Settings list rows — kept tiny and
 * dependency-free (this app has never pulled in an icon library; the rest
 * of the UI gets by on unicode glyphs like "⟩"/"ⓘ"/"✕") rather than adding
 * one just for this screen. Every icon shares the same 20x20 viewBox,
 * stroke-only style so they read as one consistent set.
 */

type IconProps = { className?: string };

const base = {
  width: 18,
  height: 18,
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

export function IconAt({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="10" cy="10" r="3.2" />
      <path d="M13.2 10v1.2a2 2 0 0 0 4 0V10a7.2 7.2 0 1 0-3 5.85" />
    </svg>
  );
}

export function IconPencil({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M12.5 3.5 16.5 7.5 6.5 17.5H2.5V13.5Z" />
    </svg>
  );
}

export function IconWallet({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <rect x="2.5" y="5.5" width="15" height="10" rx="2" />
      <path d="M2.5 8.5h15" />
      <circle cx="13.5" cy="12" r="1" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function IconInfo({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="10" cy="10" r="7.2" />
      <path d="M10 9v5" />
      <circle cx="10" cy="6.6" r="0.9" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function IconStar({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M10 2.8 12.2 8l5.6.5-4.3 3.6 1.3 5.5L10 14.6l-4.8 2.9 1.3-5.4L2.2 8.5 7.8 8Z" />
    </svg>
  );
}

export function IconPlus({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M10 3.5v13M3.5 10h13" />
    </svg>
  );
}

export function IconBell({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M5 8.5a5 5 0 0 1 10 0c0 4 1.5 5 1.5 5h-13s1.5-1 1.5-5Z" />
      <path d="M8.3 16a1.8 1.8 0 0 0 3.4 0" />
    </svg>
  );
}

export function IconLock({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <rect x="4.5" y="9" width="11" height="8" rx="2" />
      <path d="M6.5 9V6.5a3.5 3.5 0 0 1 7 0V9" />
    </svg>
  );
}

export function IconShieldOff({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M10 2.7 16.5 5v5c0 4.4-3 7-6.5 8-3.5-1-6.5-3.6-6.5-8V5Z" />
      <path d="M7.5 10.2 12.5 12M12.5 10.2 7.5 12" />
    </svg>
  );
}

export function IconTrash({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M4 6.5h12M8 6.5V4.8a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1V6.5" />
      <path d="M5.5 6.5 6.2 16a1.5 1.5 0 0 0 1.5 1.4h4.6a1.5 1.5 0 0 0 1.5-1.4l.7-9.5" />
    </svg>
  );
}

export function IconRefresh({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M16 6.5a6.5 6.5 0 1 0 1.3 5" />
      <path d="M17.3 3v4.5h-4.5" />
    </svg>
  );
}

export function IconAtom({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="10" cy="10" r="1.4" fill="currentColor" stroke="none" />
      <ellipse cx="10" cy="10" rx="7.5" ry="3" />
      <ellipse cx="10" cy="10" rx="7.5" ry="3" transform="rotate(60 10 10)" />
      <ellipse cx="10" cy="10" rx="7.5" ry="3" transform="rotate(120 10 10)" />
    </svg>
  );
}

export function IconFilter({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M3 4h14l-5.5 6.5V16l-3 1.5v-7Z" />
    </svg>
  );
}

export function IconKey({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="6.5" cy="10" r="3.5" />
      <path d="M9.8 10h7.7M14.5 10v3M16.7 10v2" />
    </svg>
  );
}

export function IconPower({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M10 3v6" />
      <path d="M5.5 6a7 7 0 1 0 9 0" />
    </svg>
  );
}

export function IconChevronRight({ className }: IconProps) {
  return (
    <svg {...base} width={14} height={14} className={className}>
      <path d="M7.5 4.5 12.5 10l-5 5.5" />
    </svg>
  );
}
