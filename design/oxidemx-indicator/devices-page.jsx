/* global React, Icon, BatteryGlyph, JRAppShell, JRPageHeader, JRSectionHead, DEVICES, bandFor */

// ─────────────────────────────────────────────────────────────
// Devices page inside the OxideMX config app
// ─────────────────────────────────────────────────────────────

const DevicesPage = ({ activeId = "mx4", devices = DEVICES }) => {
  const active = devices.find(d => d.id === activeId) || devices[0];

  return (
    <JRAppShell activeNavId="devices" activeDevice={active}>
      <JRPageHeader
        title="Devices"
        sub="Pair, switch, and manage every mouse OxideMX knows about. The active device receives radial overlays, haptic events, and button remaps; everything else is held in inventory."
      />

      <JRSectionHead title="Active device" right="Receiving events" />
      <DeviceCard device={active} isActive />

      <JRSectionHead
        title={`Paired (${devices.length - 1})`}
        right={
          <button className="jr-btn">
            <Icon name="plus" size={14} />
            Pair new device
          </button>
        }
      />
      {devices.filter(d => d.id !== active.id).map(d => (
        <DeviceCard key={d.id} device={d} />
      ))}

      <JRSectionHead title="GNOME indicator" />
      <div className="jr-device-card" style={{ flexDirection: "column", alignItems: "stretch" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
          <div style={{ flex: 1 }}>
            <div className="jr-device-card-title">Top-bar indicator</div>
            <div className="jr-device-card-meta">
              Shows a battery glyph in the GNOME top bar; click to open the OxideMX popup. Configure icons in the extension preferences, popup in the <em style={{ color: "var(--jr-accent)", fontStyle: "normal" }}>Indicator Popup</em> tab.
            </div>
          </div>
          <div className="jr-switch is-on" />
        </div>
        <div style={{ display: "flex", gap: 12, marginTop: 16, alignItems: "center" }}>
          <span className="jr-tag">Preview</span>
          <div style={{ background: "#000", borderRadius: 6, padding: "4px 8px", display: "flex", alignItems: "center", gap: 4 }}>
            <Icon name="mouse" size={13} />
            <BatteryGlyph pct={active.battery} charging={active.charging} showLabel />
          </div>
          <span style={{ fontSize: 12, color: "var(--jr-fg-muted)" }}>Reflects active device · live</span>
        </div>
      </div>
    </JRAppShell>
  );
};

// ─────────────────────────────────────────────────────────────
// One paired-device row (active or otherwise)
// ─────────────────────────────────────────────────────────────

const DeviceCard = ({ device, isActive }) => {
  const band = bandFor(device.battery, device.charging);
  return (
    <div className={`jr-device-card ${isActive ? "is-active" : ""}`}>
      <div className="jr-device-card-img">
        <Icon name="mouse" size={36} color={isActive ? "var(--jr-accent)" : "var(--jr-fg)"} />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="jr-device-card-title">{device.name}</div>
        <div className="jr-device-card-meta">{device.sub} · Channel {device.host} · {device.dpi.toLocaleString()} dpi</div>
        <div className="jr-device-card-tags">
          {isActive ? (
            <span className="jr-tag is-accent"><span className="jr-dot" style={{ width: 6, height: 6 }} /> Active</span>
          ) : (
            <span className="jr-tag">Inactive</span>
          )}
          <span className="jr-tag">
            {device.connection === "bluetooth" ? <Icon name="bluetooth" size={10} /> : <Icon name="wifi" size={10} />}
            {device.connection === "bluetooth" ? "Bluetooth" : "Bolt"}
          </span>
          <span className="jr-tag">{device.rssi} dBm</span>
          {band !== "ok" && (
            <span className={`jr-tag ${band === "low" || band === "critical" ? "is-warn" : ""}`}>
              {device.charging ? "Charging" : band === "critical" ? "Critical battery" : "Low battery"}
            </span>
          )}
        </div>
      </div>
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-end", gap: 12 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <BatteryGlyph pct={device.battery} charging={device.charging} showLabel size="lg" />
        </div>
        <div style={{ display: "flex", gap: 6 }}>
          {!isActive && (
            <button className="jr-btn">Activate</button>
          )}
          <button className="jr-icon-btn" title="More">
            <Icon name="kebab" size={14} />
          </button>
        </div>
      </div>
    </div>
  );
};

window.DevicesPage = DevicesPage;
