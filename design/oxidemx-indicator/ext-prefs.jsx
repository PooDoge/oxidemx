/* global React, Icon, BatteryGlyph */

// ─────────────────────────────────────────────────────────────
// GNOME extension preferences dialog — what opens when the user
// clicks the cog next to "OxideMX" in the GNOME Extensions app.
//
// Per spec: ONLY icon-display options. The popup itself is owned
// by the OxideMX daemon and configured in the main app's
// "Indicator Popup" tab, not here.
// ─────────────────────────────────────────────────────────────

const DEFAULT_THRESHOLDS = {
  critical: 15,
  low: 30,
  colors: { critical: "#FF3E5A", low: "#F2C94C", healthy: "#5BE095" },
};

const ExtPrefsDialog = ({
  displayMode = "both",       // "percent" | "icon" | "both"
  showMouseGlyph = true,
  tintGlyph = true,
  thresholds = DEFAULT_THRESHOLDS,
  panelTarget = "auto",       // "auto" | "topbar" | "dtp" | "both"
  position = "right",         // "left" | "center" | "right"
  positionIndex = 0,
  clickBehavior = "popup",    // "popup" | "settings" | "none"
  refreshInterval = 30,
  previewBattery = 42,
}) => {
  const previewColor = thresholds.colors[
    previewBattery <= thresholds.critical ? "critical"
    : previewBattery <= thresholds.low   ? "low"
    : "healthy"
  ];

  return (
    <div className="jr-adw">
      {/* Title bar */}
      <div className="jr-adw-titlebar">
        <div className="jr-adw-title">OxideMX Indicator</div>
        <div className="jr-adw-titlebar-right">
          <button className="jr-adw-wbtn" title="Close">
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
              <path d="M2 2l6 6M8 2l-6 6" />
            </svg>
          </button>
        </div>
      </div>

      {/* Body */}
      <div className="jr-adw-body">

        {/* preview card */}
        <div className="jr-adw-card">
          <div className="jr-adw-row" style={{ background: "rgba(0,0,0,0.18)" }}>
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">Preview</div>
              <div className="jr-adw-row-sub">Reflects the current settings live; battery shown is a mock value of {previewBattery}%.</div>
            </div>
            <div className="adw-preview-pill">
              {showMouseGlyph && (
                <Icon name="mouse" size={14} style={{ color: tintGlyph ? previewColor : "#fff" }} />
              )}
              {(displayMode === "icon" || displayMode === "both") && (
                <BatteryGlyph pct={previewBattery} />
              )}
              {(displayMode === "percent" || displayMode === "both") && (
                <span style={{ fontSize: 12, color: previewColor, fontVariantNumeric: "tabular-nums", fontWeight: 500 }}>
                  {previewBattery}%
                </span>
              )}
            </div>
          </div>
        </div>

        {/* DISPLAY */}
        <div className="jr-adw-section-title">Display</div>
        <div className="jr-adw-card">
          <div className="jr-adw-row">
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">Show as</div>
              <div className="jr-adw-row-sub">Pick what OxideMX draws in the panel — the percentage, the battery glyph, or both.</div>
            </div>
            <div className="adw-radio-group">
              <button className={`adw-radio-btn ${displayMode === "percent" ? "is-active" : ""}`}>Percent</button>
              <button className={`adw-radio-btn ${displayMode === "icon" ? "is-active" : ""}`}>Icon</button>
              <button className={`adw-radio-btn ${displayMode === "both" ? "is-active" : ""}`}>Both</button>
            </div>
          </div>
          <div className="jr-adw-row">
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">Show symbolic mouse glyph</div>
              <div className="jr-adw-row-sub">Draws a small mouse icon next to the battery indicator.</div>
            </div>
            <div className={`adw-switch ${showMouseGlyph ? "is-on" : ""}`} />
          </div>
          <div className="jr-adw-row" style={{ opacity: showMouseGlyph ? 1 : 0.4 }}>
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">Tint glyph by battery level</div>
              <div className="jr-adw-row-sub">Color the mouse glyph using the active battery-level color instead of the foreground.</div>
            </div>
            <div className={`adw-switch ${tintGlyph ? "is-on" : ""}`} />
          </div>
        </div>

        {/* BATTERY THRESHOLDS */}
        <div className="jr-adw-section-title">Battery level colors</div>
        <div className="jr-adw-card">
          <div className="jr-adw-row is-stacked">
            <div>
              <div className="jr-adw-row-label">Thresholds &amp; colors</div>
              <div className="jr-adw-row-sub">Drag the handles to set where each band starts. Click a swatch to pick a color.</div>
            </div>

            {/* threshold bar */}
            <div style={{ paddingTop: 22, paddingBottom: 8 }}>
              <ThresholdBar thresholds={thresholds} />
            </div>

            {/* per-band rows */}
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 10, marginTop: 4 }}>
              <BandRow label="Critical" range={`0 – ${thresholds.critical}%`} color={thresholds.colors.critical} />
              <BandRow label="Low"      range={`${thresholds.critical + 1} – ${thresholds.low}%`} color={thresholds.colors.low} />
              <BandRow label="Healthy"  range={`${thresholds.low + 1} – 100%`} color={thresholds.colors.healthy} />
            </div>
          </div>

          <div className="jr-adw-row">
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">Apply colors to percentage text</div>
              <div className="jr-adw-row-sub">When off, only the icon picks up the band color.</div>
            </div>
            <div className="adw-switch is-on" />
          </div>
        </div>

        {/* PLACEMENT */}
        <div className="jr-adw-section-title">Placement</div>
        <div className="jr-adw-card">
          <div className="jr-adw-row">
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">Panel</div>
              <div className="jr-adw-row-sub">Auto-detect renders on the Dash to Panel tray when it's installed; otherwise the GNOME top bar.</div>
            </div>
            <button className="adw-combo">
              {panelTarget === "auto"   && "Auto-detect"}
              {panelTarget === "topbar" && "Top bar only"}
              {panelTarget === "dtp"    && "Dash to Panel only"}
              {panelTarget === "both"   && "Both panels"}
              <Icon name="chevronDown" size={12} />
            </button>
          </div>
          <div className="jr-adw-row">
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">Position</div>
              <div className="jr-adw-row-sub">Where in the panel to place the indicator, and the order within that section.</div>
            </div>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <div className="adw-radio-group">
                <button className={`adw-radio-btn ${position === "left" ? "is-active" : ""}`}>Left</button>
                <button className={`adw-radio-btn ${position === "center" ? "is-active" : ""}`}>Center</button>
                <button className={`adw-radio-btn ${position === "right" ? "is-active" : ""}`}>Right</button>
              </div>
              <div className="adw-spinrow" title="Index">
                <button className="adw-spinrow-btn">−</button>
                <input className="adw-spinrow-input" defaultValue={positionIndex} readOnly />
                <button className="adw-spinrow-btn">+</button>
              </div>
            </div>
          </div>
        </div>

        {/* BEHAVIOR */}
        <div className="jr-adw-section-title">Behavior</div>
        <div className="jr-adw-card">
          <div className="jr-adw-row">
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">On click</div>
              <div className="jr-adw-row-sub">What happens when the user activates the indicator.</div>
            </div>
            <button className="adw-combo">
              {clickBehavior === "popup"    && "Open OxideMX popup"}
              {clickBehavior === "settings" && "Open OxideMX Settings"}
              {clickBehavior === "none"     && "Do nothing"}
              <Icon name="chevronDown" size={12} />
            </button>
          </div>
          <div className="jr-adw-row">
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">Poll interval</div>
              <div className="jr-adw-row-sub">How often to read battery + connection state from the daemon. Lower = snappier, higher = lighter.</div>
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 12, flex: "0 0 auto", minWidth: 220 }}>
              <div className="adw-slider" style={{ "--pct": `${(refreshInterval / 120) * 100}%`, width: 160 }}>
                <div className="adw-slider-track" />
                <div className="adw-slider-fill" />
                <div className="adw-slider-thumb" />
              </div>
              <span style={{ fontSize: 13, fontVariantNumeric: "tabular-nums", minWidth: 40, textAlign: "right" }}>{refreshInterval}s</span>
            </div>
          </div>
        </div>

        {/* ABOUT */}
        <div className="jr-adw-section-title">About</div>
        <div className="jr-adw-card">
          <div className="jr-adw-row">
            <div className="jr-adw-row-body">
              <div className="jr-adw-row-label">OxideMX Indicator</div>
              <div className="jr-adw-row-sub">v0.4.2 · GNOME 45+ · GPL-3.0 · Reports battery from the OxideMX daemon via D-Bus.</div>
            </div>
            <div style={{ display: "flex", gap: 6 }}>
              <button className="adw-btn">
                <a className="adw-link" style={{ color: "inherit" }}>Open daemon</a>
              </button>
              <button className="adw-btn">Report issue</button>
            </div>
          </div>
        </div>

        {/* Reset footer */}
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 6, marginTop: 18 }}>
          <button className="adw-btn is-destructive">Reset to defaults</button>
        </div>

        <div style={{ height: 24 }} />
      </div>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────
// Threshold bar — visual representation of the 3 bands
// ─────────────────────────────────────────────────────────────

const ThresholdBar = ({ thresholds }) => {
  const { critical, low, colors } = thresholds;
  return (
    <div className="jr-thresh-bar">
      <div className="jr-thresh-band" style={{ left: 0, width: `${critical}%`, background: colors.critical }}>Critical</div>
      <div className="jr-thresh-band" style={{ left: `${critical}%`, width: `${low - critical}%`, background: colors.low }}>Low</div>
      <div className="jr-thresh-band" style={{ left: `${low}%`, width: `${100 - low}%`, background: colors.healthy }}>Healthy</div>

      <div className="jr-thresh-handle" data-value={`${critical}%`} style={{ left: `calc(${critical}% - 1.5px)` }} />
      <div className="jr-thresh-handle" data-value={`${low}%`}      style={{ left: `calc(${low}% - 1.5px)` }} />
    </div>
  );
};

const BandRow = ({ label, range, color }) => (
  <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px", background: "rgba(0,0,0,0.22)", borderRadius: 8 }}>
    <div className="jr-swatch" style={{ background: color }} />
    <div style={{ minWidth: 0 }}>
      <div style={{ fontSize: 12, fontWeight: 600 }}>{label}</div>
      <div style={{ fontSize: 11, color: "rgba(255,255,255,0.55)", fontVariantNumeric: "tabular-nums" }}>{range}</div>
    </div>
  </div>
);

window.ExtPrefsDialog = ExtPrefsDialog;
