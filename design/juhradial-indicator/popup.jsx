/* global React, Icon */

const DEVICES = [
  {
    id: "mx4",
    name: "MX Master 4",
    sub: "Wireless · Bluetooth",
    battery: 30,
    charging: false,
    connection: "bluetooth",
    rssi: -42,
    host: 1,
    hosts: [
      { name: "jim-thinkpad" },
      { name: "jim-macbook" },
      { name: null },
    ],
    dpi: 1600,
    isMx: true,
  },
  {
    id: "lift",
    name: "MX Lift",
    sub: "Wireless · Bolt receiver",
    battery: 78,
    charging: false,
    connection: "unifying",
    rssi: -55,
    host: 2,
    hosts: [
      { name: "jim-thinkpad" },
      { name: "work-imac" },
      { name: null },
    ],
    dpi: 1200,
  },
  {
    id: "anywhere",
    name: "MX Anywhere 3S",
    sub: "Wireless · Bluetooth",
    battery: 12,
    charging: true,
    connection: "bluetooth",
    rssi: -60,
    host: 1,
    hosts: [
      { name: "jim-thinkpad" },
      { name: null },
      { name: null },
    ],
    dpi: 1000,
  },
];

const bandFor = (pct, charging) => {
  if (charging) return "charging";
  if (pct <= 15) return "critical";
  if (pct <= 30) return "low";
  return "ok";
};

const ringColorClass = (band) =>
  band === "critical" ? "is-critical" : band === "low" ? "is-low" : band === "charging" ? "is-charging" : "";

const fillColorVar = (band) =>
  band === "critical" ? "var(--jr-red)"
  : band === "low" ? "var(--jr-yellow)"
  : band === "charging" ? "var(--jr-green)"
  : "var(--jr-accent)";

// ─────────────────────────────────────────────────────────────
// Battery ring — large circular progress used in the popup hero
// ─────────────────────────────────────────────────────────────

const BatteryRing = ({ pct, charging, size = 88, stroke = 7 }) => {
  const band = bandFor(pct, charging);
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const off = c * (1 - pct / 100);
  return (
    <div className="jr-ring-wrap" style={{ width: size, height: size, flex: `0 0 ${size}px` }}>
      <svg viewBox={`0 0 ${size} ${size}`}>
        <circle cx={size/2} cy={size/2} r={r} className="jr-ring-bg" strokeWidth={stroke} fill="none" />
        <circle
          cx={size/2}
          cy={size/2}
          r={r}
          className={`jr-ring-fg ${ringColorClass(band)}`}
          strokeWidth={stroke}
          fill="none"
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={off}
        />
      </svg>
      <div className="jr-ring-label">
        <div style={{ display: "flex", alignItems: "baseline", gap: 1 }}>
          {pct}
          <span style={{ fontSize: 11, color: "var(--jr-fg-muted)", fontWeight: 500 }}>%</span>
          {charging && (
            <span style={{ marginLeft: 4, color: "var(--jr-green)", display: "inline-flex" }}>
              <Icon name="lightning" size={14} />
            </span>
          )}
        </div>
      </div>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────
// Compact horizontal battery glyph (used in indicator + header)
// ─────────────────────────────────────────────────────────────

const BatteryGlyph = ({ pct, charging, showLabel = false, size = "sm" }) => {
  const band = bandFor(pct, charging);
  const klass =
    band === "critical" ? "is-critical"
    : band === "low" ? "is-low"
    : band === "charging" ? "is-charging" : "";
  const dims = size === "lg"
    ? { w: 28, h: 14, cap: 8 }
    : { w: 22, h: 12, cap: 6 };
  return (
    <span className={`jr-bat ${klass}`} style={{ gap: 5 }}>
      <span className="jr-bat-body" style={{ width: dims.w, height: dims.h }}>
        <span className="jr-bat-fill" style={{ width: `${pct}%` }} />
      </span>
      <span className="jr-bat-cap" style={{ height: dims.cap }} />
      {showLabel && (
        <span style={{ marginLeft: 4, fontSize: 12, fontVariantNumeric: "tabular-nums", color: band === "ok" ? "var(--jr-fg)" : undefined }}>
          {pct}%
          {charging && (
            <Icon name="lightning" size={11} style={{ marginLeft: 2, verticalAlign: "-1px" }} />
          )}
        </span>
      )}
    </span>
  );
};

// ─────────────────────────────────────────────────────────────
// Mouse icon used in pills + dropdown rows
// ─────────────────────────────────────────────────────────────

const MouseGlyph = ({ size = 32, accent }) => (
  <div className="jr-mouse-glyph" style={{ width: size, height: size, color: accent ? "var(--jr-accent)" : undefined }}>
    <Icon name="mouse" size={Math.round(size * 0.55)} />
  </div>
);

// ─────────────────────────────────────────────────────────────
// Device pill (the clickable header that opens the dropdown)
// ─────────────────────────────────────────────────────────────

const DevicePill = ({ device, open, onClick }) => (
  <button className="jr-device-pill" onClick={onClick} style={{ position: "relative" }}>
    <Icon name="mouse" size={14} />
    <span style={{ fontWeight: 500 }}>{device.name}</span>
    <Icon name="chevronDown" size={14} style={{ transform: open ? "rotate(180deg)" : "none", transition: "transform 150ms" }} />
  </button>
);

// ─────────────────────────────────────────────────────────────
// The big popup. Default / standard / power-user are variants.
// ─────────────────────────────────────────────────────────────

const Popup = ({ deviceId = "mx4", devices = DEVICES, dropdownOpen = false, gaming = false, haptics = true, radial = true, variant = "standard" }) => {
  const device = devices.find(d => d.id === deviceId) || devices[0];
  const band = bandFor(device.battery, device.charging);

  // Estimated remaining
  const remaining = device.charging
    ? "Charging — full in ~1h 20m"
    : band === "critical" ? "About 4 hours left"
    : band === "low" ? "About 1 day left"
    : "About 3–5 days left";

  return (
    <div className="jr-popup" style={{ position: "relative" }}>

      {/* ─── Header: device pill + small status dot ─── */}
      <div className="jr-popup-header" style={{ paddingBottom: 6 }}>
        <DevicePill device={device} open={dropdownOpen} />
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6, color: device.connection === "off" ? "var(--jr-fg-muted)" : "var(--jr-fg-muted)", fontSize: 12 }}>
          <span className={`jr-dot ${device.connection === "off" ? "is-off" : ""}`} />
          {device.connection === "off" ? "Disconnected" : "Connected"}
        </div>

        {dropdownOpen && (
          <div className="jr-menu" style={{ top: 56, left: 14, right: 14 }}>
            {devices.map(d => {
              const dband = bandFor(d.battery, d.charging);
              return (
                <div key={d.id} className={`jr-menu-item ${d.id === deviceId ? "is-selected" : ""}`}>
                  <Icon name="mouse" size={16} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ fontSize: 13, fontWeight: 500 }}>{d.name}</div>
                    <div className="jr-menu-sub">{d.sub}</div>
                  </div>
                  <BatteryGlyph pct={d.battery} charging={d.charging} showLabel />
                  {d.id === deviceId && <Icon name="check" size={14} />}
                </div>
              );
            })}
            <div style={{ height: 1, background: "var(--jr-border)", margin: "4px 8px" }} />
            <div className="jr-menu-item">
              <Icon name="plus" size={16} style={{ color: "var(--jr-fg-muted)" }} />
              <span style={{ flex: 1, color: "var(--jr-fg-muted)" }}>Pair new device…</span>
            </div>
          </div>
        )}
      </div>

      {/* ─── Hero: battery ring + name/meta ─── */}
      <div className="jr-device-hero">
        <BatteryRing pct={device.battery} charging={device.charging} />
        <div style={{ minWidth: 0, flex: 1 }}>
          <div className="jr-device-hero-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
            {device.name}
            {device.charging && (
              <span style={{ color: "var(--jr-green)", display: "inline-flex", alignItems: "center", gap: 4, fontSize: 12 }}>
                <Icon name="lightning" size={12} /> Charging
              </span>
            )}
          </div>
          <div className="jr-device-hero-sub">{remaining}</div>
          <div style={{ marginTop: 8, display: "flex", gap: 6, flexWrap: "wrap" }}>
            <span className="jr-tag">
              {device.connection === "bluetooth" ? <Icon name="bluetooth" size={10} /> : device.connection === "unifying" ? <Icon name="wifi" size={10} /> : <Icon name="usb" size={10} />}
              {device.connection === "bluetooth" ? "Bluetooth" : device.connection === "unifying" ? "Bolt" : "USB"}
            </span>
            <span className="jr-tag">
              {device.rssi} dBm
            </span>
            <span className="jr-tag">DPI {device.dpi.toLocaleString()}</span>
          </div>
        </div>
      </div>

      {/* ─── Easy-Switch ─── */}
      {variant !== "compact" && (
        <>
          <div className="jr-popup-section-title">Easy-Switch host</div>
          <div className="jr-popup-section">
            <div className="jr-seg">
              {[1,2,3].map(h => {
                const hostInfo = device.hosts?.[h-1];
                const hostName = hostInfo?.name;
                return (
                  <button key={h} className={`jr-seg-btn ${h === device.host ? "is-active" : ""}`}>
                    <div className="jr-seg-btn-host">{hostName ? `Host ${h}` : "Channel"}</div>
                    <div style={{ fontSize: hostName ? 12 : 16, fontWeight: 600, letterSpacing: "-0.01em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }} title={hostName || `Channel ${h}`}>
                      {hostName || h}
                    </div>
                  </button>
                );
              })}
            </div>
          </div>
        </>
      )}

      {/* ─── Quick toggles ─── */}
      <div className="jr-popup-section-title">Quick toggles</div>
      <div className="jr-popup-section">
        <div className="jr-list">
          <div className="jr-list-row is-interactive">
            <Icon name="controller" size={16} style={{ color: "var(--jr-fg-muted)" }} />
            <div className="jr-list-row-label">
              Gaming mode
              <div className="jr-list-row-sub">Raises DPI, hides the radial</div>
            </div>
            <div className={`jr-switch ${gaming ? "is-on" : ""}`} />
          </div>
          <div className="jr-list-row is-interactive">
            <Icon name="haptic" size={16} style={{ color: "var(--jr-fg-muted)" }} />
            <div className="jr-list-row-label">
              Haptic feedback
            </div>
            <div className={`jr-switch ${haptics ? "is-on" : ""}`} />
          </div>
          {variant !== "compact" && (
            <div className="jr-list-row is-interactive">
              <Icon name="palette" size={16} style={{ color: "var(--jr-fg-muted)" }} />
              <div className="jr-list-row-label">
                Radial overlay
              </div>
              <div className={`jr-switch ${radial ? "is-on" : ""}`} />
            </div>
          )}
        </div>
      </div>

      {/* ─── DPI quick-cycle ─── */}
      {variant === "power" && (
        <>
          <div className="jr-popup-section-title">Pointer DPI</div>
          <div className="jr-popup-section">
            <div className="jr-list">
              <div className="jr-list-row" style={{ flexDirection: "column", alignItems: "stretch", gap: 8, paddingTop: 12 }}>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span className="jr-list-row-label" style={{ fontSize: 12, color: "var(--jr-fg-muted)" }}>1000 · 1600 · 3200 · 4800 dpi</span>
                  <span style={{ fontVariantNumeric: "tabular-nums", fontWeight: 600 }}>{device.dpi}</span>
                </div>
                <div className="jr-slider" style={{ "--pct": `${(device.dpi / 6400) * 100}%` }}>
                  <div className="jr-slider-track" />
                  <div className="jr-slider-fill" />
                  <div className="jr-slider-thumb" />
                </div>
              </div>
            </div>
          </div>
        </>
      )}

      {/* ─── Footer ─── */}
      <div className="jr-popup-footer">
        <button className="jr-btn is-flat" style={{ flex: "0 0 auto" }}>
          <Icon name="gear" size={14} />
        </button>
        <button className="jr-btn">
          <Icon name="plus" size={14} />
          Pair new device
        </button>
        <button className="jr-btn is-suggested">
          Open JuhRadial
        </button>
      </div>
    </div>
  );
};

window.Popup = Popup;
window.BatteryGlyph = BatteryGlyph;
window.BatteryRing = BatteryRing;
window.MouseGlyph = MouseGlyph;
window.DEVICES = DEVICES;
window.bandFor = bandFor;
