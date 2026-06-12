/* global React, Icon, RADIAL_THEMES */

// ─────────────────────────────────────────────────────────────
// OxideMX AI chat window — redesign of the arc-shell chat.
//
// Reads the same theme palette as the radial disc, keeps the
// flattened-cap header/footer language from chat_shell.rs, adds:
//  · resizable (corner grip; window grows from 484×640 freely)
//  · aurora shader backdrop tinted by theme accent
//  · page-status puck in the header (the radial's center dome
//    survives the morph as a page indicator + scroll target)
//  · agent feature cards: command execution, scheduled tasks,
//    memory writes
// ─────────────────────────────────────────────────────────────

const AIChatWindow = ({
  themeId = "oxidemx",
  width = 520,
  height = 700,
  view = "chat",           // chat | memories
  showCommandCard = true,
  showTaskCard = true,
  showMemoryCard = true,
  resizing = false,
  caption,
}) => {
  const T = RADIAL_THEMES[themeId] || RADIAL_THEMES.oxidemx;

  return (
    <div style={{ position: "relative", width, height, flex: `0 0 ${width}px` }}>
      {/* window body */}
      <div style={{
        position: "absolute", inset: 0,
        borderRadius: 24,
        background: T.base,
        border: `1px solid ${T.surface1}`,
        boxShadow: `0 24px 60px rgba(0,0,0,0.6), 0 0 0 1px rgba(0,0,0,0.4), 0 0 42px ${T.accent}14`,
        overflow: "hidden",
        display: "flex", flexDirection: "column",
        fontFamily: "var(--jr-font)",
        color: T.text,
      }}>
        {/* aurora shader backdrop */}
        <div style={{
          position: "absolute", inset: 0, pointerEvents: "none",
          background: `
            radial-gradient(55% 38% at 18% -6%, ${T.accent}26 0%, transparent 65%),
            radial-gradient(45% 30% at 85% 4%, ${T.accent2 || T.accent}1f 0%, transparent 60%),
            radial-gradient(60% 42% at 50% 108%, ${T.accent}1a 0%, transparent 60%)`,
        }} />

        {/* ── header arc ── */}
        <div style={{
          position: "relative",
          height: 52, flex: "0 0 52px",
          margin: "8px 8px 0",
          borderRadius: "18px 18px 10px 10px",
          background: `linear-gradient(180deg, ${T.surface1} 0%, ${T.surface0} 100%)`,
          border: `1px solid rgba(255,255,255,0.06)`,
          borderBottom: `1px solid ${T.accent}40`,
          display: "flex", alignItems: "center", gap: 10,
          padding: "0 8px 0 14px",
        }}>
          {/* page puck — survives the morph */}
          <div style={{
            width: 32, height: 32, borderRadius: 16, flex: "0 0 32px",
            background: `radial-gradient(circle at 36% 30%, ${T.surface2}, ${T.crust})`,
            border: `1.5px solid ${T.accent}`,
            boxShadow: `0 0 10px ${T.accent}66`,
            display: "grid", placeItems: "center", position: "relative",
          }} title="Scroll to cycle pages — chat stays active">
            <div style={{ display: "flex", gap: 3, marginTop: 2 }}>
              {[0,1,2,3].map(i => (
                <span key={i} style={{
                  width: i === 3 ? 5 : 3, height: i === 3 ? 5 : 3, borderRadius: 3,
                  background: i === 3 ? T.accent : T.overlay0,
                  boxShadow: i === 3 ? `0 0 4px ${T.accent}` : "none",
                  alignSelf: "center",
                }} />
              ))}
            </div>
          </div>

          <div style={{ minWidth: 0 }}>
            <div style={{ fontWeight: 600, fontSize: 14.5, letterSpacing: "-0.01em", lineHeight: 1.1 }}>
              AI Assistant
            </div>
            <div style={{ fontSize: 10.5, color: T.subtext0, display: "flex", alignItems: "center", gap: 5 }}>
              <span style={{ width: 6, height: 6, borderRadius: 3, background: T.green, boxShadow: `0 0 5px ${T.green}` }} />
              claude-haiku · 3 tools armed
            </div>
          </div>

          {/* drag pill */}
          <div style={{
            position: "absolute", left: "50%", top: 9, transform: "translateX(-50%)",
            width: 44, height: 4, borderRadius: 2, background: T.overlay0, opacity: 0.5,
          }} />

          <div style={{ marginLeft: "auto", display: "flex", gap: 4, alignItems: "center" }}>
            <HeaderBtn T={T} icon="brain" active={view === "memories"} title="Memories" />
            <HeaderBtn T={T} icon="clock" title="Scheduled tasks" />
            <HeaderBtn T={T} icon="plus" title="New chat" />
            <div style={{
              width: 30, height: 30, borderRadius: 15,
              background: T.text, color: T.crust,
              display: "grid", placeItems: "center", cursor: "pointer", marginLeft: 2,
            }}>
              <svg width="11" height="11" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
                <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" />
              </svg>
            </div>
          </div>
        </div>

        {/* ── thread strip ── */}
        <div style={{
          display: "flex", gap: 6, padding: "10px 16px 4px", alignItems: "center",
          position: "relative", flex: "0 0 auto",
        }}>
          <Chip T={T} active>Menu setup</Chip>
          <Chip T={T}>Research</Chip>
          <Chip T={T} dim>+ New</Chip>
          <div style={{ marginLeft: "auto", fontSize: 10.5, color: T.subtext0, display: "flex", alignItems: "center", gap: 5 }}>
            <Icon name="sparkle" size={11} style={{ color: T.accent }} />
            Flash mode
          </div>
        </div>

        {/* ── content ── */}
        {view === "memories" ? (
          <MemoriesView T={T} />
        ) : (
          <div style={{ flex: 1, minHeight: 0, overflow: "hidden", position: "relative", padding: "8px 16px 0", display: "flex", flexDirection: "column", gap: 10 }}>

            {/* user msg */}
            <Bubble T={T} who="user">
              Dim the screen to 40% tonight at 22:00, and remember I prefer warm light after sunset.
            </Bubble>

            {/* assistant msg with agent cards */}
            <Bubble T={T} who="ai">
              Done — two things set up:
            </Bubble>

            {showCommandCard && (
              <AgentCard T={T} icon="terminal" tone="green" title="Command executed" meta="brightnessctl · exit 0">
                <code style={{ fontFamily: "var(--jr-mono)", fontSize: 11, color: T.subtext1, display: "block", whiteSpace: "pre" }}>
{`$ brightnessctl set 40% --device=intel_backlight
Updated device 'intel_backlight':
  Current brightness: 38400 (40%)`}
                </code>
              </AgentCard>
            )}

            {showTaskCard && (
              <AgentCard T={T} icon="clock" tone="accent" title="Task scheduled" meta="systemd user timer"
                actions={[{ label: "Edit" }, { label: "Run now" }]}>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <div style={{ fontSize: 12.5, color: T.text }}>
                    Set brightness 40% + night light
                    <div style={{ fontSize: 11, color: T.subtext0, marginTop: 2 }}>Daily at 22:00 · next run in 6h 28m</div>
                  </div>
                  <div style={{ marginLeft: "auto" }}>
                    <MiniSwitch T={T} on />
                  </div>
                </div>
              </AgentCard>
            )}

            {showMemoryCard && (
              <AgentCard T={T} icon="brain" tone="mauve" title="Memory saved" meta="retention: until changed"
                actions={[{ label: "View all" }, { label: "Forget" }]}>
                <div style={{ fontSize: 12.5, color: T.subtext1, fontStyle: "italic" }}>
                  “Prefers warm light after sunset”
                </div>
              </AgentCard>
            )}

            <Bubble T={T} who="ai">
              Want me to also lower the mouse DPI for evening browsing? You usually drop it to 800 after 22:00.
            </Bubble>

            {/* scroll fade */}
            <div style={{ position: "absolute", left: 0, right: 0, bottom: 0, height: 18, background: `linear-gradient(transparent, ${T.base})`, pointerEvents: "none" }} />
          </div>
        )}

        {/* ── footer arc ── */}
        <div style={{
          position: "relative", flex: "0 0 auto",
          margin: "6px 8px 8px",
          borderRadius: "10px 10px 18px 18px",
          background: `linear-gradient(180deg, ${T.surface0} 0%, ${T.surface1} 100%)`,
          border: `1px solid rgba(255,255,255,0.06)`,
          borderTop: `1px solid ${T.accent}33`,
          padding: "8px 10px 10px",
        }}>
          {/* activity line */}
          <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10.5, color: T.subtext0, padding: "0 4px 6px" }}>
            <span style={{ width: 5, height: 5, borderRadius: 3, background: T.accent, boxShadow: `0 0 5px ${T.accent}`, animation: "none" }} />
            Scheduling task — writing systemd unit…
            <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 4, color: T.subtext0 }}>
              <Icon name="stop" size={9} /> Esc to stop
            </span>
          </div>
          <div style={{ display: "flex", gap: 8, alignItems: "flex-end" }}>
            <div style={{
              flex: 1, minHeight: 40, borderRadius: 12,
              background: T.crust,
              border: `1px solid ${T.surface2}`,
              padding: "10px 12px", fontSize: 13, color: T.subtext0,
            }}>
              Ask, or describe an automation…
            </div>
            <div style={{
              width: 40, height: 40, borderRadius: 12, flex: "0 0 40px",
              background: T.accent, color: T.crust,
              display: "grid", placeItems: "center",
              boxShadow: `0 4px 12px ${T.accent}55`,
            }}>
              <Icon name="send" size={17} strokeWidth={2} />
            </div>
          </div>
        </div>

        {/* resize grip */}
        <div style={{
          position: "absolute", right: 5, bottom: 5, width: 18, height: 18,
          color: resizing ? T.accent : T.overlay0, cursor: "nwse-resize",
        }}>
          <svg width="18" height="18" viewBox="0 0 18 18" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
            <path d="M15 9v6h-6" fill="none" opacity="0.9" />
            <path d="M15 13.5L13.5 15" opacity="0.6" />
          </svg>
        </div>
      </div>

      {/* resize ghost overlay */}
      {resizing && (
        <>
          <div style={{
            position: "absolute", inset: 0, borderRadius: 24,
            border: `1.5px dashed ${T.accent}`, opacity: 0.7, pointerEvents: "none",
          }} />
          <div style={{
            position: "absolute", right: -8, bottom: -28,
            fontSize: 11, fontFamily: "var(--jr-mono)", color: T.accent,
            background: T.crust, border: `1px solid ${T.surface2}`,
            padding: "3px 8px", borderRadius: 6,
          }}>{width} × {height}</div>
        </>
      )}

      {caption && (
        <div style={{
          position: "absolute", left: "50%", bottom: -36, transform: "translateX(-50%)",
          fontSize: 12, color: T.subtext0, whiteSpace: "nowrap",
        }}>{caption}</div>
      )}
    </div>
  );
};

// ─────────────────────────────────────────────────────────────
// Pieces
// ─────────────────────────────────────────────────────────────

const HeaderBtn = ({ T, icon, title, active }) => (
  <div title={title} style={{
    width: 30, height: 30, borderRadius: 9,
    background: active ? `${T.accent}22` : "transparent",
    border: active ? `1px solid ${T.accent}55` : "1px solid transparent",
    display: "grid", placeItems: "center",
    color: active ? T.accent : T.subtext0, cursor: "pointer",
  }}>
    <Icon name={icon} size={15} />
  </div>
);

const Chip = ({ T, children, active, dim }) => (
  <div style={{
    padding: "5px 12px", borderRadius: 999, fontSize: 11.5, fontWeight: 500,
    background: active ? `${T.accent}1f` : dim ? "transparent" : T.surface0,
    color: active ? T.accent : dim ? T.subtext0 : T.subtext1,
    border: `1px solid ${active ? T.accent + "66" : dim ? T.surface1 : "transparent"}`,
    cursor: "pointer", whiteSpace: "nowrap",
  }}>{children}</div>
);

const Bubble = ({ T, who, children }) => (
  <div style={{
    alignSelf: who === "user" ? "flex-end" : "flex-start",
    maxWidth: "82%",
    background: who === "user" ? `${T.accent}1c` : T.surface0,
    border: `1px solid ${who === "user" ? T.accent + "3a" : "rgba(255,255,255,0.05)"}`,
    borderRadius: who === "user" ? "14px 14px 4px 14px" : "14px 14px 14px 4px",
    padding: "9px 13px", fontSize: 13, lineHeight: 1.5, color: T.text,
  }}>{children}</div>
);

const AgentCard = ({ T, icon, tone = "accent", title, meta, children, actions = [] }) => {
  const c = T[tone] || T.accent;
  return (
    <div style={{
      alignSelf: "flex-start", width: "92%",
      background: T.mantle,
      border: `1px solid ${T.surface1}`,
      borderLeft: `2.5px solid ${c}`,
      borderRadius: 12, overflow: "hidden",
    }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: `1px solid ${T.surface0}` }}>
        <span style={{ color: c, display: "inline-flex" }}><Icon name={icon} size={13} /></span>
        <span style={{ fontSize: 11.5, fontWeight: 600, color: T.text }}>{title}</span>
        <span style={{ fontSize: 10.5, color: T.subtext0, marginLeft: "auto", fontFamily: "var(--jr-mono)" }}>{meta}</span>
      </div>
      <div style={{ padding: "9px 12px" }}>{children}</div>
      {actions.length > 0 && (
        <div style={{ display: "flex", gap: 6, padding: "0 12px 9px" }}>
          {actions.map((a, i) => (
            <span key={i} style={{
              fontSize: 10.5, fontWeight: 500, color: T.subtext1,
              border: `1px solid ${T.surface2}`, borderRadius: 6, padding: "3px 9px", cursor: "pointer",
            }}>{a.label}</span>
          ))}
        </div>
      )}
    </div>
  );
};

const MiniSwitch = ({ T, on }) => (
  <div style={{
    width: 32, height: 18, borderRadius: 9, position: "relative",
    background: on ? T.accent : T.surface2, transition: "background 120ms",
  }}>
    <div style={{
      position: "absolute", top: 2, left: on ? 16 : 2, width: 14, height: 14,
      borderRadius: 7, background: on ? T.crust : T.subtext0,
    }} />
  </div>
);

// ─────────────────────────────────────────────────────────────
// Memories management view
// ─────────────────────────────────────────────────────────────

const MEMORIES = [
  { text: "Prefers warm light after sunset", scope: "Display", retention: "Until changed", pinned: true,  age: "just now" },
  { text: "Drops mouse DPI to 800 for evening browsing", scope: "Mouse", retention: "Auto · 90d", pinned: false, age: "2d" },
  { text: "Main monitor is the Dell U2723QE on DP-1", scope: "System", retention: "Until changed", pinned: true, age: "2w" },
  { text: "Dislikes confirmation dialogs for volume changes", scope: "Behavior", retention: "Auto · 90d", pinned: false, age: "3w" },
  { text: "Works 9–17 CET; avoid scheduled restarts in that window", scope: "Schedule", retention: "Until changed", pinned: true, age: "1mo" },
];

const MemoriesView = ({ T }) => (
  <div style={{ flex: 1, minHeight: 0, overflow: "hidden", padding: "8px 16px 0", display: "flex", flexDirection: "column", gap: 8 }}>
    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
      <div style={{
        flex: 1, borderRadius: 10, background: T.crust, border: `1px solid ${T.surface2}`,
        padding: "7px 11px", fontSize: 12, color: T.subtext0,
      }}>Search memories…</div>
      <div style={{ fontSize: 10.5, color: T.subtext0, whiteSpace: "nowrap" }}>5 stored · 12 KB</div>
    </div>

    {MEMORIES.map((m, i) => (
      <div key={i} style={{
        background: T.mantle, border: `1px solid ${T.surface1}`, borderRadius: 10,
        padding: "9px 12px", display: "flex", gap: 10, alignItems: "flex-start",
      }}>
        <span style={{ color: m.pinned ? T.yellow : T.overlay0, marginTop: 1 }}>
          <Icon name="pin" size={12} />
        </span>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 12.5, color: T.text, lineHeight: 1.4 }}>{m.text}</div>
          <div style={{ display: "flex", gap: 8, marginTop: 4, fontSize: 10, color: T.subtext0 }}>
            <span style={{ color: T.accent }}>{m.scope}</span>
            <span>{m.retention}</span>
            <span>· {m.age}</span>
          </div>
        </div>
        <span style={{ color: T.subtext0, cursor: "pointer" }}><Icon name="trash" size={12} /></span>
      </div>
    ))}

    <div style={{ fontSize: 10.5, color: T.subtext0, padding: "2px 2px 8px", lineHeight: 1.5 }}>
      Auto-retention: unpinned memories expire after 90 days unused. Pinned memories persist until you change or delete them.
    </div>
  </div>
);

window.AIChatWindow = AIChatWindow;
