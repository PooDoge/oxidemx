/* global React, Icon, BatteryGlyph, Popup, DEVICES */

// ─────────────────────────────────────────────────────────────
// GNOME top bar with the JuhRadial indicator in place
// ─────────────────────────────────────────────────────────────

const TopBar = ({ device, popupOpen, dropdownOpen, gaming, haptics, radial, contextLabel = "JuhRadial" }) => {
  return (
    <div className="jr-topbar">
      <div className="jr-topbar-left">
        <div className="jr-topbar-activities">Activities</div>
        <div style={{ fontSize: 12, color: "var(--jr-fg-muted)" }}>{contextLabel}</div>
      </div>
      <div className="jr-topbar-center">Wed 24 May · 14:32</div>
      <div className="jr-topbar-right">
        <div className="jr-tray-btn">
          <Icon name="speaker" size={14} />
        </div>
        <div className="jr-tray-btn">
          <Icon name="wifi" size={14} />
        </div>
        <div className="jr-tray-btn">
          <Icon name="bluetooth" size={14} />
        </div>
        <div
          className={`jr-tray-btn jr-juhradial-indicator ${popupOpen ? "is-active" : ""}`}
          style={{ position: "relative", color: device.battery <= 30 && !device.charging ? "var(--jr-yellow)" : undefined }}
        >
          <Icon name="mouse" size={14} />
          <BatteryGlyph pct={device.battery} charging={device.charging} showLabel />
          {popupOpen && (
            <div style={{ position: "absolute", top: "calc(100% + 8px)", right: -8, zIndex: 20 }}>
              {/* Arrow pointer */}
              <div style={{
                position: "absolute",
                top: -7,
                right: 28,
                width: 14,
                height: 14,
                background: "var(--jr-surface-1)",
                borderTop: "1px solid var(--jr-border-strong)",
                borderLeft: "1px solid var(--jr-border-strong)",
                transform: "rotate(45deg)",
                zIndex: 21,
              }} />
              <Popup deviceId={device.id} dropdownOpen={dropdownOpen} gaming={gaming} haptics={haptics} radial={radial} />
            </div>
          )}
        </div>
        <div className="jr-tray-btn">
          <Icon name="power" size={14} />
        </div>
      </div>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────
// Detached "just the indicator" strip used to showcase states
// ─────────────────────────────────────────────────────────────

const IndicatorStrip = ({ items }) => (
  <div style={{ display: "flex", flexDirection: "column", gap: 24, padding: "32px 28px", width: "100%" }}>
    {items.map((it, i) => (
      <div key={i}>
        <div style={{ fontSize: 11, letterSpacing: "0.1em", textTransform: "uppercase", color: "var(--jr-fg-dim)", marginBottom: 8 }}>
          {it.label}
        </div>
        <div style={{ background: "#000", borderRadius: 8, height: 32, display: "flex", alignItems: "center", justifyContent: "flex-end", paddingRight: 12, gap: 4, border: "1px solid rgba(255,255,255,0.04)" }}>
          <div className="jr-tray-btn"><Icon name="speaker" size={14} /></div>
          <div className="jr-tray-btn"><Icon name="wifi" size={14} /></div>
          <div className="jr-tray-btn"><Icon name="bluetooth" size={14} /></div>
          <div
            className="jr-tray-btn"
            style={{ color: it.color || undefined }}
          >
            <Icon name="mouse" size={14} />
            <BatteryGlyph pct={it.pct} charging={it.charging} showLabel />
          </div>
          <div className="jr-tray-btn"><Icon name="power" size={14} /></div>
        </div>
        <div style={{ fontSize: 12, color: "var(--jr-fg-muted)", marginTop: 6 }}>{it.note}</div>
      </div>
    ))}
  </div>
);

// ─────────────────────────────────────────────────────────────
// "Disconnected" lozenge — small variant for the indicator when no mouse is paired
// ─────────────────────────────────────────────────────────────

const DisconnectedIndicator = () => (
  <div className="jr-tray-btn" style={{ color: "var(--jr-fg-muted)" }}>
    <Icon name="mouse" size={14} />
    <span style={{ fontSize: 12 }}>— no device</span>
  </div>
);

window.TopBar = TopBar;
window.IndicatorStrip = IndicatorStrip;
window.DisconnectedIndicator = DisconnectedIndicator;
