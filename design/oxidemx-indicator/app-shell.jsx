/* global React, Icon, BatteryGlyph, DEVICES */

// Shared chrome for the OxideMX Settings app — header, sidebar nav,
// status bar. Pages plug into the content slot.

const APP_NAV = [
  { id: "buttons",    label: "Mouse Buttons",   icon: "mouse" },
  { id: "menu",       label: "Menu",            icon: "palette" },
  { id: "point",      label: "Point & Scroll",  icon: "point" },
  { id: "indicator",  label: "Indicator Popup", icon: "pulse" },
  { id: "haptic",     label: "Haptic Feedback", icon: "haptic" },
  { id: "devices",    label: "Devices",         icon: "monitor" },
  { id: "switch",     label: "Easy-Switch",     icon: "switch" },
  { id: "flow",       label: "Flow",            icon: "grid", stub: "STUB" },
  { id: "macros",     label: "Macros",          icon: "macro" },
  { id: "gaming",     label: "Gaming",          icon: "controller" },
  { id: "settings",   label: "Settings",        icon: "wrench" },
];

const JRAppShell = ({ activeNavId, activeDevice, children, statusText = "Idle." }) => (
  <div className="jr-app">
    <div className="jr-app-header">
      <div className="jr-app-title">
        OxideMX
        <span className="jr-app-mx">MX</span>
        <span className="jr-app-sub">MOUSE CONFIGURATION</span>
      </div>

      <button className="jr-app-device-btn" style={{ display: "flex", alignItems: "center", gap: 8 }}>
        {activeDevice.name.toUpperCase()}
        <Icon name="chevronDown" size={12} style={{ color: "var(--jr-fg-muted)" }} />
      </button>
      <div className="jr-app-bat-mini">
        <BatteryGlyph pct={activeDevice.battery} charging={activeDevice.charging} showLabel size="lg" />
      </div>

      <button className="jr-app-exit">Exit</button>
    </div>

    <div className="jr-app-body">
      <div className="jr-app-sidebar">
        {APP_NAV.map(n => (
          <div key={n.id} className={`jr-nav-item ${n.id === activeNavId ? "is-active" : ""}`}>
            <Icon name={n.icon} size={16} />
            <span>{n.label}</span>
            {n.stub && <span className="jr-nav-stub">{n.stub}</span>}
          </div>
        ))}
      </div>

      <div className="jr-app-content jr-noscroll">
        {children}
      </div>
    </div>

    <div className="jr-app-statusbar">
      <span>OxideMX · Free &amp; open source software</span>
      <span style={{ color: "var(--jr-fg-dim)" }}>/home/jim/.config/oxidemx/config.json</span>
      <span className="jr-status-right">{statusText}</span>
    </div>
  </div>
);

// Reusable page header for any settings page
const JRPageHeader = ({ title, sub }) => (
  <>
    <div className="jr-app-page-title">{title}</div>
    {sub && <div className="jr-app-page-sub">{sub}</div>}
  </>
);

// Reusable section heading (small uppercase) used inside pages
const JRSectionHead = ({ title, right }) => (
  <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", marginTop: 28, marginBottom: 6 }}>
    <div style={{ fontSize: 11, letterSpacing: "0.1em", textTransform: "uppercase", color: "var(--jr-fg-dim)", fontWeight: 600 }}>
      {title}
    </div>
    {right && <div style={{ fontSize: 12, color: "var(--jr-fg-muted)" }}>{right}</div>}
  </div>
);

window.JRAppShell = JRAppShell;
window.JRPageHeader = JRPageHeader;
window.JRSectionHead = JRSectionHead;
window.APP_NAV = APP_NAV;
