/** Minimal inline SVG icon set for `ContactProfile`, matching the style already established in `features/settings/icons.tsx`. */

type IconProps = { className?: string };

const base = {
  width: 16,
  height: 16,
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.6,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

export function IconUserCheck({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="8" cy="6.5" r="3" />
      <path d="M3 17c0-3 2.2-5 5-5" />
      <path d="M12.5 12.5 14.5 14.5 18 10.5" />
    </svg>
  );
}

export function IconUserMinus({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="8" cy="6.5" r="3" />
      <path d="M3 17c0-3 2.2-5 5-5" />
      <path d="M13 11h6" />
    </svg>
  );
}

export function IconUserPlus({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <circle cx="8" cy="6.5" r="3" />
      <path d="M3 17c0-3 2.2-5 5-5" />
      <path d="M16 8v6M13 11h6" />
    </svg>
  );
}

export function IconLockOpen({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <rect x="4.5" y="9" width="11" height="8" rx="2" />
      <path d="M6.5 9V6.5a3.5 3.5 0 0 1 6.6-1.6" />
    </svg>
  );
}

export function IconChatBubble({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M3 4.5h14v9H8l-3.5 3v-3H3Z" />
    </svg>
  );
}

export function IconPhone({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M5 3.5h2.5l1 3.5-2 1.5a10 10 0 0 0 5 5l1.5-2 3.5 1V15c0 1-.8 1.8-1.8 1.7C8.7 16 4 11.3 3.3 5.3 3.2 4.3 4 3.5 5 3.5Z" />
    </svg>
  );
}

export function IconCheck({ className }: IconProps) {
  return (
    <svg {...base} className={className}>
      <path d="M4 10.5 8 14.5 16 5.5" />
    </svg>
  );
}
