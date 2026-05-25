/* global React */
// Icons used across the JuhRadial design canvas.
// Stroke-based 24px feather-style icons (lucide-flavored).

const Icon = ({ name, size = 16, strokeWidth = 1.75, color = "currentColor", style }) => {
  const paths = ICONS[name];
  if (!paths) return null;
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke={color}
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      style={style}
      aria-hidden="true"
    >
      {paths}
    </svg>
  );
};

const ICONS = {
  mouse: (
    <>
      <rect x="6" y="3" width="12" height="18" rx="6" />
      <path d="M12 3v8" />
    </>
  ),
  bluetooth: (
    <path d="M6.5 6l11 12-5.5 4V2l5.5 4-11 12" />
  ),
  usb: (
    <>
      <circle cx="10" cy="20" r="1.5" />
      <path d="M10 18.5V8" />
      <path d="M10 8l-3-3h6" />
      <path d="M10 14l4-3v8a3 3 0 0 1-3 3" />
      <circle cx="14" cy="11" r="1.5" />
      <path d="M14 6V3l2 2-2 2-2-2 2-2" />
    </>
  ),
  wifi: (
    <>
      <path d="M2 8a14 14 0 0 1 20 0" />
      <path d="M5 11.5a10 10 0 0 1 14 0" />
      <path d="M8.5 15a6 6 0 0 1 7 0" />
      <circle cx="12" cy="19" r="1" fill="currentColor" />
    </>
  ),
  lightning: (
    <path d="M13 2L5 14h6l-1 8 8-12h-6l1-8z" fill="currentColor" stroke="none" />
  ),
  chevronDown: (
    <path d="M6 9l6 6 6-6" />
  ),
  chevronRight: (
    <path d="M9 6l6 6-6 6" />
  ),
  chevronLeft: (
    <path d="M15 6l-6 6 6 6" />
  ),
  plus: (
    <>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </>
  ),
  gear: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3h.1a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8v.1a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </>
  ),
  controller: (
    <>
      <path d="M6 12h4M8 10v4" />
      <circle cx="15" cy="13" r="1" fill="currentColor" />
      <circle cx="18" cy="11" r="1" fill="currentColor" />
      <rect x="2" y="7" width="20" height="11" rx="5" />
    </>
  ),
  monitor: (
    <>
      <rect x="2" y="4" width="20" height="13" rx="2" />
      <path d="M8 21h8" />
      <path d="M12 17v4" />
    </>
  ),
  switch: (
    <>
      <path d="M7 7h12" />
      <path d="M15 3l4 4-4 4" />
      <path d="M17 17H5" />
      <path d="M9 13l-4 4 4 4" />
    </>
  ),
  haptic: (
    <>
      <path d="M3 12c2-3 4-3 6 0s4 3 6 0 4-3 6 0" />
      <path d="M3 17c2-3 4-3 6 0s4 3 6 0 4-3 6 0" opacity="0.5" />
      <path d="M3 7c2-3 4-3 6 0s4 3 6 0 4-3 6 0" opacity="0.5" />
    </>
  ),
  macro: (
    <polygon points="6,4 18,12 6,20" fill="currentColor" stroke="none" />
  ),
  point: (
    <>
      <path d="M5 3l14 9-6 1 3 6-2 1-3-6-4 4z" />
    </>
  ),
  palette: (
    <>
      <path d="M12 22A10 10 0 1 1 22 12c0 3-2 4-4 4h-2a2 2 0 0 0 0 4 2 2 0 0 1-2 2z" />
      <circle cx="7.5" cy="11" r="1" fill="currentColor" stroke="none" />
      <circle cx="9.5" cy="6.5" r="1" fill="currentColor" stroke="none" />
      <circle cx="14.5" cy="6.5" r="1" fill="currentColor" stroke="none" />
      <circle cx="17.5" cy="11" r="1" fill="currentColor" stroke="none" />
    </>
  ),
  grid: (
    <>
      <rect x="3" y="3" width="7" height="7" rx="1.2" />
      <rect x="14" y="3" width="7" height="7" rx="1.2" />
      <rect x="3" y="14" width="7" height="7" rx="1.2" />
      <rect x="14" y="14" width="7" height="7" rx="1.2" />
    </>
  ),
  wrench: (
    <path d="M14.7 6.3a4 4 0 0 0-5.4 5.4l-7 7 2 2 7-7a4 4 0 0 0 5.4-5.4l-2.5 2.5-1.4-1.4z" />
  ),
  pulse: (
    <path d="M3 12h4l2-6 4 12 2-6h6" />
  ),
  signal: (
    <>
      <rect x="3"  y="14" width="3" height="6" rx="1" fill="currentColor" stroke="none" />
      <rect x="8"  y="10" width="3" height="10" rx="1" fill="currentColor" stroke="none" />
      <rect x="13" y="6"  width="3" height="14" rx="1" fill="currentColor" stroke="none" opacity="0.45"/>
      <rect x="18" y="3"  width="3" height="17" rx="1" fill="currentColor" stroke="none" opacity="0.2"/>
    </>
  ),
  signalFull: (
    <>
      <rect x="3"  y="14" width="3" height="6" rx="1" fill="currentColor" stroke="none" />
      <rect x="8"  y="10" width="3" height="10" rx="1" fill="currentColor" stroke="none" />
      <rect x="13" y="6"  width="3" height="14" rx="1" fill="currentColor" stroke="none" />
      <rect x="18" y="3"  width="3" height="17" rx="1" fill="currentColor" stroke="none" />
    </>
  ),
  check: <path d="M4 12l5 5L20 6" />,
  unplug: (
    <>
      <path d="M3 21l4-4" />
      <path d="M21 3l-4 4" />
      <path d="M9 11l-3 3a3 3 0 0 0 4 4l3-3" />
      <path d="M11 9l3-3a3 3 0 0 1 4 4l-3 3" />
    </>
  ),
  trash: (
    <>
      <path d="M4 7h16" />
      <path d="M10 11v6M14 11v6" />
      <path d="M6 7l1 13a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-13" />
      <path d="M9 7V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v3" />
    </>
  ),
  kebab: (
    <>
      <circle cx="12" cy="5"  r="1.4" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="12" cy="19" r="1.4" fill="currentColor" stroke="none" />
    </>
  ),
  speaker: (
    <>
      <path d="M11 5L6 9H3v6h3l5 4z" />
      <path d="M16 9a4 4 0 0 1 0 6" />
    </>
  ),
  power: (
    <>
      <path d="M12 3v9" />
      <path d="M5.6 7.2a8 8 0 1 0 12.8 0" />
    </>
  ),
};

window.Icon = Icon;
