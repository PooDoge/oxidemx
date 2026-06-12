/* global React, ReactDOM, DesignCanvas, DCSection, DCArtboard, TweaksPanel, TweakSection, TweakSlider, TweakRadio, TweakSelect, TweakToggle, useTweaks, Icon, Popup, TopBar, IndicatorStrip, DevicesPage, IndicatorPopupPage, ExtPrefsDialog, BatteryGlyph, BatteryRing, DEVICES, RadialMenu, RADIAL_THEMES, AIChatWindow */

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "accent": "orange",
  "battery": 30,
  "charging": false,
  "connection": "bluetooth",
  "device": "mx4",
  "host": 1,
  "gaming": false,
  "haptics": true,
  "radial": true,
  "popupMode": "simple",
  "displayMode": "both",
  "showMouseGlyph": true,
  "tintGlyph": true,
  "hostLabel": "hostname",
  "showHostButtons": true,
  "volumeOnScroll": true,
  "radialTheme": "oxidemx",
  "chatW": 520,
  "chatH": 700,
  "cardCommand": true,
  "cardTask": true,
  "cardMemory": true
}/*EDITMODE-END*/;

const App = () => {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);

  // Apply the accent to the document root so all variants react.
  React.useEffect(() => {
    document.documentElement.dataset.accent = t.accent;
  }, [t.accent]);

  // Live device that reflects the tweaks panel (battery/connection/etc).
  const liveDevice = React.useMemo(() => {
    const base = DEVICES.find(d => d.id === t.device) || DEVICES[0];
    return {
      ...base,
      battery: t.battery,
      charging: t.charging,
      connection: t.connection,
      host: t.host,
    };
  }, [t.device, t.battery, t.charging, t.connection, t.host]);

  const liveDevices = DEVICES.map(d => d.id === t.device ? liveDevice : d);

  return (
    <>
      <DesignCanvas>
        {/* ────────────── INDICATOR (TOP BAR CONTEXT) ────────────── */}
        <DCSection id="indicator" title="GNOME top-bar indicator">
          <DCArtboard id="topbar-closed" label="Closed · in context" width={1280} height={720}>
            <Desktop>
              <TopBar device={liveDevice} contextLabel="Firefox" />
              <DesktopBody />
            </Desktop>
          </DCArtboard>

          <DCArtboard id="topbar-open" label="Popup open · default state" width={1280} height={900}>
            <Desktop>
              <TopBar device={liveDevice} popupOpen gaming={t.gaming} haptics={t.haptics} radial={t.radial} contextLabel="Firefox" />
              <DesktopBody dimmed />
            </Desktop>
          </DCArtboard>
        </DCSection>

        {/* ────────────── POPUP STATES ────────────── */}
        <DCSection id="popup-states" title="Popup — states &amp; variants">
          <DCArtboard id="popup-default" label="Default" width={420} height={720}>
            <Floating>
              <Popup deviceId={t.device} devices={liveDevices} gaming={t.gaming} haptics={t.haptics} radial={t.radial} />
            </Floating>
          </DCArtboard>

          <DCArtboard id="popup-dropdown" label="Device dropdown open" width={420} height={720}>
            <Floating>
              <Popup deviceId={t.device} devices={liveDevices} dropdownOpen gaming={t.gaming} haptics={t.haptics} radial={t.radial} />
            </Floating>
          </DCArtboard>

          <DCArtboard id="popup-power" label="Power user — with DPI slider" width={420} height={820}>
            <Floating>
              <Popup deviceId={t.device} devices={liveDevices} gaming={t.gaming} haptics={t.haptics} radial={t.radial} variant="power" />
            </Floating>
          </DCArtboard>

          <DCArtboard id="popup-compact" label="Compact — minimal" width={420} height={620}>
            <Floating>
              <Popup deviceId={t.device} devices={liveDevices} gaming={t.gaming} haptics={t.haptics} radial={t.radial} variant="compact" />
            </Floating>
          </DCArtboard>

          <DCArtboard id="popup-charging" label="Charging — MX Anywhere 3S" width={420} height={720}>
            <Floating>
              <Popup deviceId="anywhere" devices={DEVICES} gaming={false} haptics />
            </Floating>
          </DCArtboard>

          <DCArtboard id="popup-disconnected" label="Disconnected" width={420} height={620}>
            <Floating>
              <Popup
                deviceId="mx4"
                devices={[{ ...DEVICES[0], connection: "off", battery: 0 }, ...DEVICES.slice(1)]}
                gaming={false}
                haptics
              />
            </Floating>
          </DCArtboard>
        </DCSection>

        {/* ────────────── INDICATOR STATES ────────────── */}
        <DCSection id="indicator-states" title="Indicator — battery states up close">
          <DCArtboard id="indicator-row" label="Every battery + connection state" width={720} height={680}>
            <div style={{ background: "var(--jr-bg-deepest)", height: "100%", borderRadius: 8 }}>
              <IndicatorStrip
                items={[
                  { label: "Healthy", pct: 78, note: "78% · default fg, no urgency", color: undefined },
                  { label: "Low (≤ 30%)", pct: 30, note: "Switches to yellow once 30% is hit", color: "var(--jr-yellow)" },
                  { label: "Critical (≤ 15%)", pct: 8, note: "Pulses red; system notification fires once", color: "var(--jr-red)" },
                  { label: "Charging", pct: 42, charging: true, note: "Green fill + lightning glyph", color: "var(--jr-green)" },
                ]}
              />
            </div>
          </DCArtboard>
        </DCSection>

        {/* ────────────── RADIAL MENU PAGES ────────────── */}
        <DCSection id="radial-pages" title="Radial menu · new pages (theme-aware)">
          <DCArtboard id="radial-device" label="Device settings page · power submenu open" width={760} height={620}>
            <RadialStage themeId={t.radialTheme}>
              <RadialMenu themeId={t.radialTheme} pageId="device" pageIndex={1} submenuFor={2} caption="Hover Power → child items pop out beyond the ring" />
            </RadialStage>
          </DCArtboard>
          <DCArtboard id="radial-device-mouse" label="Device settings · mouse submenu open" width={760} height={620}>
            <RadialStage themeId={t.radialTheme}>
              <RadialMenu themeId={t.radialTheme} pageId="device" pageIndex={1} submenuFor={3} caption="Quick mouse settings: DPI · SmartShift · Haptics · Gaming" />
            </RadialStage>
          </DCArtboard>
          <DCArtboard id="radial-widgets" label="Splice widgets page · live data in wedges" width={760} height={620}>
            <RadialStage themeId={t.radialTheme}>
              <RadialMenu themeId={t.radialTheme} pageId="widgets" pageIndex={2} hover={1} caption="Weather · CPU · RAM · Net · Disk · Tasks · Task Mgr · Battery" />
            </RadialStage>
          </DCArtboard>
          <DCArtboard id="radial-apps" label="Apps page (current) — for reference" width={760} height={620}>
            <RadialStage themeId={t.radialTheme}>
              <RadialMenu themeId={t.radialTheme} pageId="apps" pageIndex={0} hover={6} caption="Existing launcher page, re-skinned by the same theme tokens" />
            </RadialStage>
          </DCArtboard>
        </DCSection>

        {/* ────────────── PAGE-CYCLE → AI CHAT FLOW ────────────── */}
        <DCSection id="ai-flow" title="Page-cycle → AI chat · activation flow">
          <DCArtboard id="flow-1" label="1 · Scrolling pages — dot 4 reached" width={620} height={580}>
            <RadialStage themeId={t.radialTheme}>
              <RadialMenu themeId={t.radialTheme} pageId="apps" pageIndex={3} centerActive caption="Wheel reaches AI page — center ring lights up and STAYS armed" />
            </RadialStage>
          </DCArtboard>
          <DCArtboard id="flow-2" label="2 · Disc morphs — chat opens, ring survives" width={620} height={760}>
            <RadialStage themeId={t.radialTheme}>
              <AIChatWindow themeId={t.radialTheme} width={484} height={640} showCommandCard={false} showTaskCard={false} showMemoryCard={false}
                caption="Cap morph from chat_shell.rs — page puck lands in the header" />
            </RadialStage>
          </DCArtboard>
          <DCArtboard id="flow-3" label="3 · Cursor leaves center — chat stays active" width={620} height={760}>
            <RadialStage themeId={t.radialTheme} cursor>
              <AIChatWindow themeId={t.radialTheme} width={484} height={640} showCommandCard={false} showTaskCard={false} showMemoryCard={false}
                caption="Click / mouse-out no longer dismisses — scroll over the puck still cycles pages" />
            </RadialStage>
          </DCArtboard>
        </DCSection>

        {/* ────────────── AI CHAT REDESIGN ────────────── */}
        <DCSection id="ai-chat" title="AI chat window · redesign (resizable, agent features)">
          <DCArtboard id="chat-main" label="Agent conversation — command + task + memory cards" width={t.chatW + 120} height={t.chatH + 120}>
            <RadialStage themeId={t.radialTheme}>
              <AIChatWindow themeId={t.radialTheme} width={t.chatW} height={t.chatH}
                showCommandCard={t.cardCommand} showTaskCard={t.cardTask} showMemoryCard={t.cardMemory} />
            </RadialStage>
          </DCArtboard>
          <DCArtboard id="chat-resize" label="Resizing — grip + live size badge" width={760} height={880}>
            <RadialStage themeId={t.radialTheme}>
              <AIChatWindow themeId={t.radialTheme} width={640} height={760} resizing
                showCommandCard showTaskCard showMemoryCard={false}
                caption="Drag the corner grip — min 420×560, grows freely, persists per theme" />
            </RadialStage>
          </DCArtboard>
          <DCArtboard id="chat-memories" label="Memories — retention management" width={t.chatW + 120} height={t.chatH + 120}>
            <RadialStage themeId={t.radialTheme}>
              <AIChatWindow themeId={t.radialTheme} width={t.chatW} height={t.chatH} view="memories" />
            </RadialStage>
          </DCArtboard>
        </DCSection>

        {/* ────────────── CONFIG APP — DEVICES PAGE ────────────── */}
        <DCSection id="config-app" title="Settings app · Devices page">
          <DCArtboard id="devices-page" label="Devices page" width={1280} height={840}>
            <DevicesPage activeId={t.device} devices={liveDevices} />
          </DCArtboard>
        </DCSection>

        {/* ────────────── INDICATOR POPUP TAB (NEW) ────────────── */}
        <DCSection id="indicator-popup-tab" title="Settings app · new Indicator Popup tab">
          <DCArtboard id="indicator-popup-simple" label="Simple mode" width={1280} height={1280}>
            <IndicatorPopupPage
              activeDevice={liveDevice}
              mode="simple"
              hostLabelStyle={t.hostLabel}
              showHostButtons={t.showHostButtons}
              volumeOnScroll={t.volumeOnScroll}
              setMode={(v) => setTweak('popupMode', v)}
            />
          </DCArtboard>

          <DCArtboard id="indicator-popup-power" label="Power User mode" width={1280} height={1620}>
            <IndicatorPopupPage
              activeDevice={liveDevice}
              mode="power"
              hostLabelStyle={t.hostLabel}
              showHostButtons={t.showHostButtons}
              volumeOnScroll={t.volumeOnScroll}
              setMode={(v) => setTweak('popupMode', v)}
            />
          </DCArtboard>

          <DCArtboard id="indicator-popup-live" label={`Live (current tweaks · ${t.popupMode})`} width={1280} height={t.popupMode === 'power' ? 1620 : 1280}>
            <IndicatorPopupPage
              activeDevice={liveDevice}
              mode={t.popupMode}
              hostLabelStyle={t.hostLabel}
              showHostButtons={t.showHostButtons}
              volumeOnScroll={t.volumeOnScroll}
              setMode={(v) => setTweak('popupMode', v)}
            />
          </DCArtboard>
        </DCSection>

        {/* ────────────── GNOME EXTENSION PREFS DIALOG ────────────── */}
        <DCSection id="ext-prefs" title="GNOME extension preferences (icon-only)">
          <DCArtboard id="ext-prefs-window" label="Extension prefs · single page" width={720} height={1200}>
            <div style={{ width: "100%", height: "100%", padding: 20, background: "radial-gradient(60% 60% at 50% 30%, #1a1a1a 0%, #0a0a0a 80%)", display: "grid", placeItems: "start center" }}>
              <ExtPrefsDialog
                displayMode={t.displayMode}
                showMouseGlyph={t.showMouseGlyph}
                tintGlyph={t.tintGlyph}
                previewBattery={t.battery}
              />
            </div>
          </DCArtboard>
        </DCSection>
      </DesignCanvas>

      <TweaksPanel title="Tweaks">
        <TweakSection label="Radial + AI chat">
          <TweakSelect label="Theme" value={t.radialTheme}
            options={Object.entries(RADIAL_THEMES).map(([id, th]) => ({ value: id, label: th.label }))}
            onChange={(v) => setTweak('radialTheme', v)} />
          <TweakSlider label="Chat width" value={t.chatW} min={420} max={760} step={10}
            onChange={(v) => setTweak('chatW', v)} />
          <TweakSlider label="Chat height" value={t.chatH} min={560} max={920} step={10}
            onChange={(v) => setTweak('chatH', v)} />
          <TweakToggle label="Command card" value={t.cardCommand} onChange={(v) => setTweak('cardCommand', v)} />
          <TweakToggle label="Task card" value={t.cardTask} onChange={(v) => setTweak('cardTask', v)} />
          <TweakToggle label="Memory card" value={t.cardMemory} onChange={(v) => setTweak('cardMemory', v)} />
        </TweakSection>

        <TweakSection label="Accent">
          <TweakRadio label="Brand accent" value={t.accent} options={[
            { value: "orange", label: "Orange" },
            { value: "cyan",   label: "Cyan" },
          ]} onChange={(v) => setTweak('accent', v)} />
        </TweakSection>

        <TweakSection label="Active device">
          <TweakSelect label="Device" value={t.device}
            options={DEVICES.map(d => ({ value: d.id, label: d.name }))}
            onChange={(v) => setTweak('device', v)} />
          <TweakSelect label="Channel" value={t.connection} options={[
            { value: "bluetooth", label: "Bluetooth" },
            { value: "unifying",  label: "Bolt receiver" },
            { value: "off",       label: "Disconnected" },
          ]} onChange={(v) => setTweak('connection', v)} />
          <TweakRadio label="Easy-Switch host" value={t.host} options={[
            { value: 1, label: "1" }, { value: 2, label: "2" }, { value: 3, label: "3" },
          ]} onChange={(v) => setTweak('host', v)} />
        </TweakSection>

        <TweakSection label="Battery">
          <TweakSlider label="Level" value={t.battery} min={0} max={100} step={1} unit="%"
            onChange={(v) => setTweak('battery', v)} />
          <TweakToggle label="Charging" value={t.charging} onChange={(v) => setTweak('charging', v)} />
        </TweakSection>

        <TweakSection label="Quick toggles">
          <TweakToggle label="Gaming mode" value={t.gaming} onChange={(v) => setTweak('gaming', v)} />
          <TweakToggle label="Haptic feedback" value={t.haptics} onChange={(v) => setTweak('haptics', v)} />
          <TweakToggle label="Radial overlay" value={t.radial} onChange={(v) => setTweak('radial', v)} />
        </TweakSection>

        <TweakSection label="Indicator Popup tab">
          <TweakRadio label="Mode" value={t.popupMode} options={[
            { value: "simple", label: "Simple" },
            { value: "power",  label: "Power User" },
          ]} onChange={(v) => setTweak('popupMode', v)} />
          <TweakRadio label="Host labels" value={t.hostLabel} options={[
            { value: "hostname", label: "Hostname" },
            { value: "channel",  label: "Channel" },
          ]} onChange={(v) => setTweak('hostLabel', v)} />
          <TweakToggle label="Show host buttons" value={t.showHostButtons} onChange={(v) => setTweak('showHostButtons', v)} />
          <TweakToggle label="Volume on scroll"  value={t.volumeOnScroll}  onChange={(v) => setTweak('volumeOnScroll', v)} />
        </TweakSection>

        <TweakSection label="Extension prefs">
          <TweakRadio label="Display" value={t.displayMode} options={[
            { value: "percent", label: "%" },
            { value: "icon",    label: "Icon" },
            { value: "both",    label: "Both" },
          ]} onChange={(v) => setTweak('displayMode', v)} />
          <TweakToggle label="Show mouse glyph" value={t.showMouseGlyph} onChange={(v) => setTweak('showMouseGlyph', v)} />
          <TweakToggle label="Tint glyph by battery" value={t.tintGlyph} onChange={(v) => setTweak('tintGlyph', v)} />
        </TweakSection>
      </TweaksPanel>
    </>
  );
};

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────

const Desktop = ({ children }) => (
  <div className="jr-desktop" style={{ width: "100%", height: "100%", display: "flex", flexDirection: "column" }}>
    {children}
  </div>
);

const DesktopBody = ({ dimmed }) => (
  <div style={{ flex: 1, position: "relative", overflow: "hidden" }}>
    {/* Mock desktop content */}
    <div style={{ position: "absolute", inset: 0, background: dimmed ? "rgba(0,0,0,0.32)" : "transparent" }} />
    <div style={{ position: "absolute", left: 32, top: 32, color: "rgba(255,255,255,0.45)", fontSize: 13 }}>
      <div style={{ fontWeight: 600, fontSize: 15, color: "rgba(255,255,255,0.7)" }}>~/Documents</div>
      <div style={{ marginTop: 6 }}>3 folders · 27 items</div>
    </div>
    {/* Fake app window so the indicator has visual anchor */}
    <div style={{
      position: "absolute",
      left: 80, top: 90,
      width: 520, height: "calc(100% - 160px)",
      background: "rgba(14, 16, 22, 0.82)",
      borderRadius: 12,
      border: "1px solid rgba(255,255,255,0.05)",
      backdropFilter: "blur(4px)",
    }}>
      <div style={{ height: 40, borderBottom: "1px solid rgba(255,255,255,0.05)", display: "flex", alignItems: "center", padding: "0 14px", gap: 6 }}>
        <div style={{ width: 12, height: 12, borderRadius: 6, background: "#444" }} />
        <div style={{ width: 12, height: 12, borderRadius: 6, background: "#444" }} />
        <div style={{ width: 12, height: 12, borderRadius: 6, background: "#444" }} />
      </div>
      <div style={{ padding: 18, color: "rgba(255,255,255,0.4)", fontSize: 13 }}>
        <div style={{ fontSize: 18, fontWeight: 600, color: "rgba(255,255,255,0.55)", marginBottom: 12 }}>Document</div>
        {Array.from({ length: 14 }).map((_, i) => (
          <div key={i} style={{ height: 8, background: "rgba(255,255,255,0.04)", borderRadius: 4, margin: "8px 0", width: `${60 + (i*7)%40}%` }} />
        ))}
      </div>
    </div>
  </div>
);

const Floating = ({ children }) => (
  <div style={{ width: "100%", height: "100%", display: "grid", placeItems: "center", background: "radial-gradient(60% 60% at 50% 40%, #14171f 0%, #07080B 80%)" }}>
    {children}
  </div>
);

// Stage for radial/chat artboards — desktop-ish backdrop tinted by the active radial theme
const RadialStage = ({ themeId = "oxidemx", cursor = false, children }) => {
  const T = RADIAL_THEMES[themeId] || RADIAL_THEMES.oxidemx;
  return (
    <div style={{
      width: "100%", height: "100%", display: "grid", placeItems: "center",
      background: `radial-gradient(70% 60% at 50% 38%, ${T.mantle} 0%, ${T.crust} 85%)`,
      position: "relative", overflow: "hidden", borderRadius: 8,
    }}>
      {children}
      {cursor && (
        <svg width="22" height="22" viewBox="0 0 24 24" style={{ position: "absolute", right: "12%", bottom: "18%", filter: "drop-shadow(0 2px 4px rgba(0,0,0,0.6))" }}>
          <path d="M5 3l14 9-6 1 3 6-2 1-3-6-4 4z" fill="#fff" stroke="#000" strokeWidth="1" />
        </svg>
      )}
    </div>
  );
};

const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(<App />);
