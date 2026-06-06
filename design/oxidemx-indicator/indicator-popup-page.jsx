/* global React, Icon, JRAppShell, JRPageHeader, JRSectionHead, BatteryGlyph, DEVICES */

// ─────────────────────────────────────────────────────────────
// "Indicator Popup" tab — sits under "Point & Scroll" in nav
//
// Controls everything that appears in the popup that opens when
// you click the GNOME indicator. The popup itself is rendered by
// the OxideMX daemon (not by GJS), so all of this lives in the
// main settings app, not the extension prefs.
// ─────────────────────────────────────────────────────────────

const ALL_QUICK_TOGGLES = [
  { id: "gaming",    label: "Gaming Mode",     icon: "controller", desc: "Hides the radial, bumps DPI" },
  { id: "haptics",   label: "Haptic Feedback", icon: "haptic",     desc: "Click-and-hold ticks" },
  { id: "radial",    label: "Radial Overlay",  icon: "palette",    desc: "Toggle the radial menu" },
  { id: "flow",      label: "Flow",            icon: "grid",       desc: "Cross-device scroll & paste" },
  { id: "smart",     label: "SmartShift",      icon: "switch",     desc: "Free-spin scroll wheel" },
  { id: "highlight", label: "Cursor highlight",icon: "point",      desc: "Pulse ring on shake" },
];

const ALL_QUICK_SLIDERS = [
  { id: "dpi",     label: "Pointer DPI",        icon: "point",   range: "200 – 6,400 dpi" },
  { id: "scroll",  label: "Scroll sensitivity", icon: "switch",  range: "1 – 10" },
  { id: "haptic_i",label: "Haptic intensity",   icon: "haptic",  range: "Off – Strong" },
  { id: "accel",   label: "Pointer acceleration", icon: "pulse", range: "-1.0 – 1.0" },
];

const DEFAULT_SIMPLE_TOGGLES = ["gaming", "haptics", "radial"];
const DEFAULT_POWER_TOGGLES = ["gaming", "haptics", "radial", "flow"];
const DEFAULT_POWER_SLIDERS = ["dpi", "scroll"];

// ─────────────────────────────────────────────────────────────

const IndicatorPopupPage = ({
  activeDevice,
  mode = "simple",
  showHostButtons = true,
  hostLabelStyle = "hostname",
  volumeOnScroll = true,
  setMode,
  // mutable lists (defaults applied when not passed)
  simpleToggles = DEFAULT_SIMPLE_TOGGLES,
  powerToggles = DEFAULT_POWER_TOGGLES,
  powerSliders = DEFAULT_POWER_SLIDERS,
}) => {
  const isPower = mode === "power";
  const enabledToggleIds = isPower ? powerToggles : simpleToggles;
  const availableToggles = ALL_QUICK_TOGGLES.filter(q => !enabledToggleIds.includes(q.id));

  const enabledSliders = isPower ? powerSliders : [];
  const availableSliders = isPower ? ALL_QUICK_SLIDERS.filter(q => !enabledSliders.includes(q.id)) : [];

  return (
    <JRAppShell activeNavId="indicator" activeDevice={activeDevice}>
      <JRPageHeader
        title="Indicator Popup"
        sub="Controls what appears when you click the OxideMX icon in the GNOME top bar. The popup itself is rendered by OxideMX — not by the GNOME extension — so it shares this app's theming and reacts to everything you change here."
      />

      {/* ── MODE ─────────────────────────────────────────── */}
      <JRSectionHead title="Mode" />
      <div className="jr-mode-toggle" style={{ marginTop: 6 }}>
        <button
          className={`jr-mode-btn ${!isPower ? "is-active" : ""}`}
          onClick={() => setMode && setMode("simple")}
        >
          <div className="jr-mode-btn-title">
            <Icon name="check" size={14} style={{ visibility: isPower ? "hidden" : "visible" }} />
            Simple
          </div>
          <div className="jr-mode-btn-sub">
            Battery, device, and a small set of on/off toggles. Best for everyday use.
          </div>
        </button>
        <button
          className={`jr-mode-btn ${isPower ? "is-active" : ""}`}
          onClick={() => setMode && setMode("power")}
        >
          <div className="jr-mode-btn-title">
            <Icon name="check" size={14} style={{ visibility: isPower ? "visible" : "hidden" }} />
            Power User
          </div>
          <div className="jr-mode-btn-sub">
            Adds sliders (DPI, scroll, haptics) and signal / DPI / channel details. Reorder anything.
          </div>
        </button>
      </div>

      {/* ── EASY-SWITCH ──────────────────────────────────── */}
      <JRSectionHead title="Easy-Switch host buttons" />
      <div className="jr-pref-card">
        <div className="jr-pref-row">
          <div className="jr-pref-body">
            <div className="jr-pref-label">Show host buttons</div>
            <div className="jr-pref-sub">Render the three Easy-Switch hosts as a segmented control at the top of the popup.</div>
          </div>
          <div className={`jr-switch ${showHostButtons ? "is-on" : ""}`} />
        </div>
        <div className="jr-pref-row">
          <div className="jr-pref-body">
            <div className="jr-pref-label">Label style</div>
            <div className="jr-pref-sub">Use the paired hostname when OxideMX can read it; fall back to “Channel 1/2/3” for unnamed hosts.</div>
          </div>
          <div className="jr-radio-group">
            <button className={`jr-radio-btn ${hostLabelStyle === "hostname" ? "is-active" : ""}`}>Hostname → Channel</button>
            <button className={`jr-radio-btn ${hostLabelStyle === "channel" ? "is-active" : ""}`}>Channel only</button>
          </div>
        </div>

        {/* preview of the segment */}
        <div className="jr-pref-row is-stacked">
          <div style={{ fontSize: 11, letterSpacing: "0.08em", textTransform: "uppercase", color: "var(--jr-fg-dim)", fontWeight: 600 }}>
            Preview
          </div>
          <div className="jr-seg" style={{ maxWidth: 360 }}>
            {[1,2,3].map(h => {
              const hostInfo = activeDevice.hosts?.[h-1];
              const hostName = hostLabelStyle === "channel" ? null : hostInfo?.name;
              return (
                <div key={h} className={`jr-seg-btn ${h === activeDevice.host ? "is-active" : ""}`}>
                  <div className="jr-seg-btn-host">{hostName ? `Host ${h}` : "Channel"}</div>
                  <div style={{ fontSize: hostName ? 12 : 16, fontWeight: 600, letterSpacing: "-0.01em", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                    {hostName || h}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* ── QUICK TOGGLES (always present) ──────────────── */}
      <JRSectionHead
        title="Quick toggles"
        right={<span style={{ fontSize: 12 }}>{enabledToggleIds.length} enabled · {availableToggles.length} available</span>}
      />
      <div style={{ marginTop: 6 }}>
        {enabledToggleIds.map((id, idx) => {
          const t = ALL_QUICK_TOGGLES.find(x => x.id === id);
          if (!t) return null;
          return (
            <ReorderRow
              key={id}
              t={t}
              kind="TOGGLE"
              isFirst={idx === 0}
              isLast={idx === enabledToggleIds.length - 1}
            />
          );
        })}

        {availableToggles.length > 0 && (
          <>
            <div style={{ fontSize: 11, letterSpacing: "0.08em", textTransform: "uppercase", color: "var(--jr-fg-dim)", fontWeight: 600, marginTop: 18, marginBottom: 8 }}>
              Available
            </div>
            {availableToggles.map(t => (
              <AddRow key={t.id} t={t} kind="TOGGLE" />
            ))}
          </>
        )}
      </div>

      {/* ── QUICK SLIDERS (power-user only) ─────────────── */}
      {isPower && (
        <>
          <JRSectionHead
            title="Quick sliders"
            right={<span style={{ fontSize: 12 }}>Power User only</span>}
          />
          <div style={{ marginTop: 6 }}>
            {enabledSliders.map((id, idx) => {
              const t = ALL_QUICK_SLIDERS.find(x => x.id === id);
              if (!t) return null;
              return (
                <ReorderRow
                  key={id}
                  t={t}
                  kind="SLIDER"
                  isFirst={idx === 0}
                  isLast={idx === enabledSliders.length - 1}
                  meta={t.range}
                />
              );
            })}
            {availableSliders.length > 0 && (
              <>
                <div style={{ fontSize: 11, letterSpacing: "0.08em", textTransform: "uppercase", color: "var(--jr-fg-dim)", fontWeight: 600, marginTop: 18, marginBottom: 8 }}>
                  Available
                </div>
                {availableSliders.map(t => (
                  <AddRow key={t.id} t={t} kind="SLIDER" meta={t.range} />
                ))}
              </>
            )}
          </div>
        </>
      )}

      {/* ── INTERACTIONS ────────────────────────────────── */}
      <JRSectionHead title="Interactions" />
      <div className="jr-pref-card">
        <div className="jr-pref-row">
          <div className="jr-pref-body">
            <div className="jr-pref-label">Adjust system volume on scroll while popup is focused</div>
            <div className="jr-pref-sub">When the popup has focus, the mouse wheel raises or lowers system volume in 5% steps. Releases focus on click-away.</div>
          </div>
          <div className={`jr-switch ${volumeOnScroll ? "is-on" : ""}`} />
        </div>
        <div className="jr-pref-row">
          <div className="jr-pref-body">
            <div className="jr-pref-label">Close on action</div>
            <div className="jr-pref-sub">Dismiss the popup after a toggle or slider change.</div>
          </div>
          <div className="jr-switch" />
        </div>
        <div className="jr-pref-row">
          <div className="jr-pref-body">
            <div className="jr-pref-label">Animations</div>
            <div className="jr-pref-sub">Fade / slide as the popup mounts and unmounts.</div>
          </div>
          <div className="jr-switch is-on" />
        </div>
      </div>

      <div style={{ height: 32 }} />
    </JRAppShell>
  );
};

// ─────────────────────────────────────────────────────────────
// Row pieces
// ─────────────────────────────────────────────────────────────

const ReorderRow = ({ t, kind, isFirst, isLast, meta }) => (
  <div className="jr-reorder-row">
    <div className="jr-reorder-handle">
      <button className={`jr-reorder-arrow ${isFirst ? "is-disabled" : ""}`} title="Move up">
        <Icon name="chevronDown" size={12} style={{ transform: "rotate(180deg)" }} />
      </button>
      <button className={`jr-reorder-arrow ${isLast ? "is-disabled" : ""}`} title="Move down">
        <Icon name="chevronDown" size={12} />
      </button>
    </div>
    <div className="jr-reorder-row-icon">
      <Icon name={t.icon} size={14} />
    </div>
    <div style={{ flex: 1, minWidth: 0 }}>
      <div className="jr-reorder-row-label">{t.label}</div>
      <div style={{ fontSize: 12, color: "var(--jr-fg-muted)", marginTop: 2 }}>{meta || t.desc}</div>
    </div>
    <span className="jr-reorder-row-kind">{kind}</span>
    <button className="jr-icon-btn" title="Remove">
      <Icon name="trash" size={13} />
    </button>
  </div>
);

const AddRow = ({ t, kind, meta }) => (
  <div className="jr-add-row">
    <div className="jr-reorder-row-icon" style={{ background: "rgba(255,255,255,0.04)" }}>
      <Icon name={t.icon} size={14} />
    </div>
    <div style={{ flex: 1, minWidth: 0 }}>
      <div style={{ fontSize: 13, color: "var(--jr-fg)" }}>{t.label}</div>
      <div style={{ fontSize: 12, color: "var(--jr-fg-muted)", marginTop: 2 }}>{meta || t.desc}</div>
    </div>
    <span className="jr-reorder-row-kind">{kind}</span>
    <button className="jr-add-btn" title="Add">
      <Icon name="plus" size={14} />
    </button>
  </div>
);

window.IndicatorPopupPage = IndicatorPopupPage;
