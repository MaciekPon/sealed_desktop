/**
 * Renders an SVG file's raw markup inline (via Vite's `?raw` import, not
 * `<img src>`), so `stroke="currentColor"`/`fill="currentColor"` inside the
 * SVG file keeps inheriting this app's dynamic theme colors (active/hover
 * states etc.) exactly like the hand-written inline-JSX icons this
 * replaced. To swap an icon, overwrite its file in `src/assets/icons/`
 * with the same filename — no code changes needed. Use `currentColor` for
 * any stroke/fill you want to follow the surrounding text color; hardcode
 * a hex value instead for anything that should stay fixed regardless of
 * theme/state.
 */
export function Icon({ svg, className }: { svg: string; className?: string }) {
  return <span className={`icon ${className ?? ""}`} dangerouslySetInnerHTML={{ __html: svg }} />;
}
