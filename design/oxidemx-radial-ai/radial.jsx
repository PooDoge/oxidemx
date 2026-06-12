/* global React, Icon */

// ─────────────────────────────────────────────────────────────
// Theme palettes — lifted from oxidemx-shared/themes/*.json
// ─────────────────────────────────────────────────────────────

const RADIAL_THEMES = {
  oxidemx: {
    label: "OxideMX MX",
    crust: "#0a0c10", mantle: "#0f1117", base: "#121418",
    surface0: "#1a1d24", surface1: "#242832", surface2: "#2e3440",
    overlay0: "#404654", text: "#f0f4f8", subtext1: "#c8d0dc", subtext0: "#9aa5b5",
    accent: "#00d4ff", accent2: "#0abdc6", accentDim: "#0891a8",
    green: "#00e676", yellow: "#ffd54f", red: "#ff5252", blue: "#4a9eff",
    mauve: "#b388ff", pink: "#ff80ab", peach: "#ffab40", teal: "#0abdc6",
  },
  dracula: {
    label: "Dracula",
    crust: "#191a21", mantle: "#21222c", base: "#282a36",
    surface0: "#343746", surface1: "#44475a", surface2: "#565a75",
    overlay0: "#6272a4", text: "#f8f8f2", subtext1: "#d6d6d2", subtext0: "#a8a8b2",
    accent: "#bd93f9", accent2: "#ff79c6", accentDim: "#8861c4",
    green: "#50fa7b", yellow: "#f1fa8c", red: "#ff5555", blue: "#8be9fd",
    mauve: "#bd93f9", pink: "#ff79c6", peach: "#ffb86c", teal: "#8be9fd",
  },
  nord: {
    label: "Nord",
    crust: "#242933", mantle: "#2e3440", base: "#2e3440",
    surface0: "#3b4252", surface1: "#434c5e", surface2: "#4c566a",
    overlay0: "#616e88", text: "#eceff4", subtext1: "#d8dee9", subtext0: "#aeb8c9",
    accent: "#88c0d0", accent2: "#81a1c1", accentDim: "#5e81ac",
    green: "#a3be8c", yellow: "#ebcb8b", red: "#bf616a", blue: "#81a1c1",
    mauve: "#b48ead", pink: "#b48ead", peach: "#d08770", teal: "#8fbcbb",
  },
  mocha: {
    label: "Catppuccin Mocha",
    crust: "#11111b", mantle: "#181825", base: "#1e1e2e",
    surface0: "#313244", surface1: "#45475a", surface2: "#585b70",
    overlay0: "#6c7086", text: "#cdd6f4", subtext1: "#bac2de", subtext0: "#a6adc8",
    accent: "#89b4fa", accent2: "#cba6f7", accentDim: "#5878b8",
    green: "#a6e3a1", yellow: "#f9e2af", red: "#f38ba8", blue: "#89b4fa",
    mauve: "#cba6f7", pink: "#f5c2e7", peach: "#fab387", teal: "#94e2d5",
  },
};

// ─────────────────────────────────────────────────────────────
// Page data — the proposed radial pages
// ─────────────────────────────────────────────────────────────

const RADIAL_PAGES = {
  apps: {
    title: "Apps",
    slices: [
      { icon: "macro",      label: "Play",       c: "green" },
      { icon: "plus",       label: "New note",   c: "yellow" },
      { icon: "terminal",   label: "Terminal",   c: "teal" },
      { icon: "gear",       label: "Settings",   c: "mauve" },
      { icon: "camera",     label: "Screenshot", c: "peach" },
      { icon: "smile",      label: "Emoji",      c: "pink" },
      { icon: "folder",     label: "Files",      c: "blue" },
      { icon: "flask",      label: "Lab",        c: "accent" },
    ],
  },
  device: {
    title: "Device",
    slices: [
      { icon: "brightness", label: "Brightness", c: "yellow", value: "70%", kind: "dial" },
      { icon: "speaker",    label: "Volume",     c: "teal",   value: "45%", kind: "dial" },
      { icon: "power",      label: "Power",      c: "red",    kind: "submenu",
        submenu: [
          { icon: "lock",     label: "Lock" },
          { icon: "logout",   label: "Log off" },
          { icon: "moon",     label: "Suspend" },
          { icon: "restart",  label: "Restart" },
          { icon: "power",    label: "Shut down" },
        ] },
      { icon: "mouse",      label: "Mouse",      c: "accent", kind: "submenu",
        submenu: [
          { icon: "point",   label: "DPI 1600" },
          { icon: "switch",  label: "SmartShift" },
          { icon: "haptic",  label: "Haptics" },
          { icon: "controller", label: "Gaming" },
        ] },
      { icon: "wifi",       label: "Network",    c: "blue",   value: "↓ 84 Mb/s" },
      { icon: "bluetooth",  label: "Bluetooth",  c: "mauve" },
      { icon: "monitor",    label: "Displays",   c: "peach" },
      { icon: "moon",       label: "Night light", c: "pink", toggled: true },
    ],
  },
  widgets: {
    title: "Widgets",
    slices: [
      { kind: "widget", label: "Weather",  c: "yellow", big: "14°", small: "Clear · Oslo", icon: "sun" },
      { kind: "widget", label: "CPU",      c: "teal",   big: "23%", small: "8 cores · 52°C", spark: [3,5,4,7,6,9,5,6,4,8] },
      { kind: "widget", label: "Memory",   c: "mauve",  big: "11.2", small: "of 32 GB", spark: [4,4,5,5,6,6,6,7,7,7] },
      { kind: "widget", label: "Network",  c: "blue",   big: "84↓",  small: "12↑ Mb/s", spark: [2,8,5,9,3,7,8,4,9,6] },
      { kind: "widget", label: "Disk",     c: "peach",  big: "412",  small: "GB free" },
      { kind: "widget", label: "Tasks",    c: "green",  big: "3",    small: "due today", icon: "check" },
      { kind: "submenu", label: "Task Mgr", c: "red",   icon: "pulse",
        submenu: [
          { icon: "pulse",  label: "Processes" },
          { icon: "grid",   label: "Services" },
          { icon: "trash",  label: "Kill app" },
        ] },
      { kind: "widget", label: "Battery",  c: "accent", big: "78%", small: "MX Master 4", icon: "mouse" },
    ],
  },
  ai: {
    title: "AI Assistant",
    slices: [],
  },
};

// ─────────────────────────────────────────────────────────────
// Geometry helpers
// ─────────────────────────────────────────────────────────────

const polar = (cx, cy, r, deg) => {
  const a = ((deg - 90) * Math.PI) / 180;
  return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
};

const wedgePath = (cx, cy, r0, r1, a0, a1) => {
  const [x0, y0] = polar(cx, cy, r1, a0);
  const [x1, y1] = polar(cx, cy, r1, a1);
  const [x2, y2] = polar(cx, cy, r0, a1);
  const [x3, y3] = polar(cx, cy, r0, a0);
  const large = a1 - a0 > 180 ? 1 : 0;
  return `M ${x0} ${y0} A ${r1} ${r1} 0 ${large} 1 ${x1} ${y1} L ${x2} ${y2} A ${r0} ${r0} 0 ${large} 0 ${x3} ${y3} Z`;
};

// ─────────────────────────────────────────────────────────────
// RadialMenu — theme-aware disc
// ─────────────────────────────────────────────────────────────

const RadialMenu = ({
  themeId = "oxidemx",
  pageId = "apps",
  size = 420,
  hover = null,          // hovered slice index
  submenuFor = null,     // slice index with open submenu
  pageIndex = 0,
  pageCount = 4,
  centerActive = false,  // page-cycle ring lit (AI handoff state)
  dimDisc = false,       // fade disc (used in morph storyboard)
  caption,
}) => {
  const T = RADIAL_THEMES[themeId] || RADIAL_THEMES.oxidemx;
  const page = RADIAL_PAGES[pageId] || RADIAL_PAGES.apps;
  const n = Math.max(page.slices.length, 1);
  const cx = size / 2, cy = size / 2;
  const rOut = size * 0.46;
  const rIn  = size * 0.155;
  const iconR = size * 0.31;
  const discAlpha = dimDisc ? 0.25 : 1;

  const subSlice = submenuFor != null ? page.slices[submenuFor] : null;
  const subItems = subSlice?.submenu || [];

  return (
    <div style={{ position: "relative", width: size, height: size, flex: `0 0 ${size}px` }}>
      <svg width={size} height={size} style={{ position: "absolute", inset: 0, overflow: "visible" }}>
        <defs>
          <radialGradient id={`glow-${themeId}-${pageId}`} cx="50%" cy="50%" r="50%">
            <stop offset="55%" stopColor={T.accent} stopOpacity="0" />
            <stop offset="85%" stopColor={T.accent} stopOpacity="0.16" />
            <stop offset="100%" stopColor={T.accent} stopOpacity="0" />
          </radialGradient>
          <radialGradient id={`dome-${themeId}`} cx="38%" cy="32%" r="80%">
            <stop offset="0%" stopColor={T.surface2} />
            <stop offset="55%" stopColor={T.mantle} />
            <stop offset="100%" stopColor={T.crust} />
          </radialGradient>
          <linearGradient id={`bevel-${themeId}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="rgba(255,255,255,0.10)" />
            <stop offset="100%" stopColor="rgba(0,0,0,0.32)" />
          </linearGradient>
        </defs>

        {/* ambient shader glow */}
        <circle cx={cx} cy={cy} r={rOut * 1.18} fill={`url(#glow-${themeId}-${pageId})`} opacity={discAlpha} />

        {/* disc body */}
        <g opacity={discAlpha}>
          <circle cx={cx} cy={cy} r={rOut + 8} fill={T.crust} opacity="0.9" />
          <circle cx={cx} cy={cy} r={rOut + 8} fill="none" stroke="rgba(255,255,255,0.05)" strokeWidth="1.5" />
          <circle cx={cx} cy={cy} r={rOut + 2} fill={T.mantle} />

          {/* wedges */}
          {page.slices.map((s, i) => {
            const a0 = (360 / n) * i + 1.2;
            const a1 = (360 / n) * (i + 1) - 1.2;
            const isHover = hover === i || submenuFor === i;
            return (
              <g key={i}>
                <path
                  d={wedgePath(cx, cy, rIn + 6, rOut - 4, a0, a1)}
                  fill={isHover ? T.surface1 : T.base}
                  stroke={isHover ? T[s.c] || T.accent : "rgba(255,255,255,0.04)"}
                  strokeWidth={isHover ? 1.5 : 1}
                />
                <path
                  d={wedgePath(cx, cy, rIn + 6, rOut - 4, a0, a1)}
                  fill={`url(#bevel-${themeId})`}
                  opacity={isHover ? 0.5 : 0.3}
                  style={{ pointerEvents: "none" }}
                />
                {isHover && (
                  <path
                    d={wedgePath(cx, cy, rOut - 10, rOut - 4, a0, a1)}
                    fill={T[s.c] || T.accent}
                    opacity="0.85"
                  />
                )}
              </g>
            );
          })}

          {/* center dome */}
          <circle cx={cx} cy={cy} r={rIn} fill={`url(#dome-${themeId})`} />
          <circle
            cx={cx} cy={cy} r={rIn}
            fill="none"
            stroke={centerActive ? T.accent : "rgba(255,255,255,0.14)"}
            strokeWidth={centerActive ? 2.5 : 1.5}
            style={centerActive ? { filter: `drop-shadow(0 0 6px ${T.accent})` } : undefined}
          />

          {/* page dots */}
          {Array.from({ length: pageCount }).map((_, i) => {
            const spread = 12;
            const x = cx - ((pageCount - 1) * spread) / 2 + i * spread;
            return (
              <circle
                key={i}
                cx={x}
                cy={cy + rIn * 0.55}
                r={i === pageIndex ? 4 : 2.5}
                fill={i === pageIndex ? T.accent : T.overlay0}
                style={i === pageIndex && centerActive ? { filter: `drop-shadow(0 0 4px ${T.accent})` } : undefined}
              />
            );
          })}
        </g>
      </svg>

      {/* slice content (icons / widgets) */}
      <div style={{ position: "absolute", inset: 0, opacity: discAlpha, pointerEvents: "none" }}>
        {page.slices.map((s, i) => {
          const mid = (360 / n) * (i + 0.5);
          const [x, y] = polar(cx, cy, iconR, mid);
          const color = T[s.c] || T.accent;
          const isHover = hover === i || submenuFor === i;

          if (s.kind === "widget") {
            return (
              <div key={i} style={{
                position: "absolute", left: x, top: y, transform: "translate(-50%, -50%)",
                textAlign: "center", width: 86,
              }}>
                <div style={{ fontSize: 21, fontWeight: 700, color: isHover ? color : T.text, letterSpacing: "-0.02em", fontVariantNumeric: "tabular-nums", lineHeight: 1 }}>
                  {s.big}
                </div>
                {s.spark ? (
                  <svg width="46" height="12" viewBox="0 0 46 12" style={{ margin: "3px 0 1px" }}>
                    <polyline
                      points={s.spark.map((v, j) => `${j * 5},${12 - v * 1.1}`).join(" ")}
                      fill="none" stroke={color} strokeWidth="1.5" strokeLinejoin="round" opacity="0.85"
                    />
                  </svg>
                ) : (
                  <div style={{ height: 3 }} />
                )}
                <div style={{ fontSize: 9.5, color: T.subtext0, lineHeight: 1.2 }}>{s.small}</div>
                <div style={{ fontSize: 9, color: isHover ? color : T.subtext1, fontWeight: 600, marginTop: 2, textTransform: "uppercase", letterSpacing: "0.06em" }}>{s.label}</div>
              </div>
            );
          }

          return (
            <div key={i} style={{ position: "absolute", left: x, top: y, transform: "translate(-50%, -50%)", textAlign: "center" }}>
              <div style={{
                width: 52, height: 52, borderRadius: 26, margin: "0 auto",
                background: isHover ? T.surface2 : T.surface0,
                border: `1.5px solid ${isHover ? color : "rgba(255,255,255,0.06)"}`,
                display: "grid", placeItems: "center",
                color,
                boxShadow: isHover ? `0 0 14px ${color}55` : "0 2px 6px rgba(0,0,0,0.4)",
              }}>
                <Icon name={s.icon} size={22} />
                {s.kind === "submenu" && (
                  <div style={{
                    position: "absolute", right: -2, bottom: -2, width: 16, height: 16, borderRadius: 8,
                    background: T.surface1, border: `1px solid rgba(255,255,255,0.12)`,
                    display: "grid", placeItems: "center", color: T.subtext1,
                  }}>
                    <Icon name="chevronRight" size={9} />
                  </div>
                )}
                {s.toggled && (
                  <div style={{
                    position: "absolute", right: -2, top: -2, width: 10, height: 10, borderRadius: 5,
                    background: T.green, boxShadow: `0 0 6px ${T.green}`,
                  }} />
                )}
              </div>
              {s.value ? (
                <div style={{ fontSize: 10, color: T.subtext1, marginTop: 4, fontVariantNumeric: "tabular-nums", fontWeight: 600 }}>{s.value}</div>
              ) : (
                <div style={{ fontSize: 10, color: isHover ? T.text : T.subtext0, marginTop: 4 }}>{s.label}</div>
              )}
            </div>
          );
        })}

        {/* submenu pop-outs */}
        {subItems.map((it, j) => {
          const parentMid = (360 / n) * (submenuFor + 0.5);
          const spread = 18;
          const a = parentMid + (j - (subItems.length - 1) / 2) * spread;
          const [x, y] = polar(cx, cy, rOut + 42, a);
          const highlight = j === 1;
          return (
            <div key={j} style={{ position: "absolute", left: x, top: y, transform: "translate(-50%, -50%)", textAlign: "center" }}>
              <div style={{
                width: 44, height: 44, borderRadius: 22, margin: "0 auto",
                background: highlight ? T.surface2 : T.surface0,
                border: `1.5px solid ${highlight ? (T[subSlice.c] || T.accent) : "rgba(255,255,255,0.10)"}`,
                display: "grid", placeItems: "center",
                color: highlight ? (T[subSlice.c] || T.accent) : T.subtext1,
                boxShadow: highlight ? `0 0 12px ${(T[subSlice.c] || T.accent)}66` : "0 3px 8px rgba(0,0,0,0.5)",
              }}>
                <Icon name={it.icon} size={18} />
              </div>
              <div style={{
                fontSize: 9.5, marginTop: 3, fontWeight: highlight ? 600 : 400,
                color: highlight ? T.text : T.subtext0, whiteSpace: "nowrap",
              }}>{it.label}</div>
            </div>
          );
        })}
      </div>

      {caption && (
        <div style={{
          position: "absolute", left: "50%", bottom: -34, transform: "translateX(-50%)",
          fontSize: 12, color: T.subtext0, whiteSpace: "nowrap", textAlign: "center",
        }}>{caption}</div>
      )}
    </div>
  );
};

window.RadialMenu = RadialMenu;
window.RADIAL_THEMES = RADIAL_THEMES;
window.RADIAL_PAGES = RADIAL_PAGES;
