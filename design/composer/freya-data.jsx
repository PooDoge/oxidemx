/* global window */
// ───────────────────────────────────────────────────────────────
// OxideMX · Freya rebuild — sample data + theme
// Models the three design specs:
//  · Projects · Conversations · Worktrees model
//  · Flow delivery to chat (artifact cards, per-conversation run state)
//  · Conductor correctness (the stages() concurrency contract)
// ───────────────────────────────────────────────────────────────

const THEME = {
  // surfaces — refined OxideMX, app-scale (calmer than the overlay)
  crust: "#0a0c10", mantle: "#0f1117", base: "#121418",
  surface0: "#1a1d24", surface1: "#242832", surface2: "#2e3440",
  overlay0: "#404654",
  // text
  text: "#f0f4f8", subtext1: "#c8d0dc", subtext0: "#9aa5b5", faint: "#5d6675",
  // accent
  accent: "#00d4ff", accent2: "#0abdc6", accentDim: "#0891a8",
  // semantic
  green: "#00e676", yellow: "#ffd54f", red: "#ff5252", blue: "#4a9eff",
  mauve: "#b388ff", pink: "#ff80ab", peach: "#ffab40", teal: "#0abdc6",
  // hairlines
  hair: "rgba(255,255,255,0.06)",
  hairStrong: "rgba(255,255,255,0.10)",

  // ── named compositional tokens (replace every raw hex in styles) ──
  // desktop backdrop (the wallpaper the window floats on)
  bg0: "#060a12", bg1: "#0a1018", bg2: "#070b11", bgGlow1: "#14203320",
  // surface-y panes that aren't part of the elevation ramp
  codeBg: "#070a0e",       // inline code / artifact previews
  editorBg: "#0a0d12",     // built-in code editor pane
  logBg: "#06080c",        // conductor live-log popover
  scrim: "rgba(5,7,11,0.7)",   // modal backdrop
  // native (Adwaita) titlebar — fixed greys, independent of accent
  adwBar0: "#34373b", adwBar1: "#2c2f33", adwBorder: "rgba(0,0,0,0.45)",
  adwInset: "rgba(255,255,255,0.05)", adwBtnBg: "rgba(255,255,255,0.10)",
  adwBtnText: "#e3e3e3", adwIcon: "#c8c8c8", adwTitle: "#eaeaea",
  adwLogo0: "#00d4ff", adwLogo1: "#0891a8",
  // window-frame chrome shadows / borders
  winBorderNative: "rgba(0,0,0,0.6)", winRing: "rgba(255,255,255,0.04)",
  // named black-shadow tokens (so no raw rgba() shadow colors remain in styles)
  shadowDeep: "rgba(0,0,0,0.62)",   // window drop shadow
  shadowMed: "rgba(0,0,0,0.55)",    // popover / floating cards
  shadowSoft: "rgba(0,0,0,0.5)",    // tooltips, pill, secondary depth
};

// Alpha levels in use across the design (hex-alpha suffixes promoted to
// named tokens like accent_14, yellow_55, mauve_1a — value is the exact
// 8-digit hex so the render is bit-identical).
const ALPHAS = ["0c","10","12","14","16","1a","1c","1f","22","26","33","3a","40","44","50","55","66","88"];
const ALPHA_PALETTE = [
  "crust","mantle","base","surface0","surface1","surface2","overlay0",
  "text","subtext1","subtext0","faint",
  "accent","accent2","accentDim",
  "green","yellow","red","blue","mauve","pink","peach","teal",
];
function alphaVars(name, hex) {
  const o = {};
  for (const a of ALPHAS) o[name + "_" + a] = hex + a;
  return o;
}
// Compose the full theme for a given accent set. Generates the per-color
// alpha tokens AFTER the accent override so accent_NN tracks the Tweak.
function makeTheme(accentSet) {
  const t = { ...THEME, ...accentSet };
  for (const k of ALPHA_PALETTE) {
    if (typeof t[k] === "string" && t[k][0] === "#") Object.assign(t, alphaVars(k, t[k]));
  }
  return t;
}

// alternate accents for the Tweaks panel
const ACCENTS = {
  cyan:   { accent: "#00d4ff", accent2: "#0abdc6", accentDim: "#0891a8" },
  violet: { accent: "#b388ff", accent2: "#8b6cff", accentDim: "#7a5bd0" },
  amber:  { accent: "#ffab40", accent2: "#ff8f3f", accentDim: "#cf8232" },
  lime:   { accent: "#7be06a", accent2: "#52c24a", accentDim: "#46a23e" },
};

// ── Projects (registry) ─────────────────────────────────────────
const PROJECTS = [
  {
    id: "personal", name: "Personal", icon: "sparkle", dir: null,
    sub: "assistant chats · no repo", conversations: 4,
  },
  {
    id: "oxidemx-phase1", name: "oxidemx-phase1", icon: "chip",
    dir: "~/code/oxidemx-phase1",
    sub: "phase1-local-llm-gateway", conversations: 9, active: true, running: 2,
  },
  {
    id: "freya-fork", name: "freya", icon: "folder",
    dir: "~/code/freya",
    sub: "0.4.0-rc.23", conversations: 3,
  },
  {
    id: "agentd", name: "agentd", icon: "terminal",
    dir: "~/code/agentd",
    sub: "main", conversations: 2,
  },
];

// ── Conversations under the active project ──────────────────────
const CONVERSATIONS = [
  {
    id: "c-flow-delivery", title: "Flow delivery to chat — artifact cards",
    model: "claude-sonnet", updated: "now", tokens: "48.2k",
    state: "working", worktree: "worktree-flow-delivery", active: true,
  },
  {
    id: "c-conductor", title: "Conductor stage-ordering fix",
    model: "claude-sonnet", updated: "6m", tokens: "31.0k",
    state: "delivered", worktree: "worktree-conductor",
  },
  {
    id: "c-worktrees", title: "Projects · conversations · worktrees model",
    model: "claude-opus", updated: "22m", tokens: "112k",
    state: "idle", worktree: null,
  },
  {
    id: "c-oxide-resolver", title: ".oxide walk-up config resolver",
    model: "claude-sonnet", updated: "1h", tokens: "27.4k",
    state: "idle", worktree: "worktree-resolver",
  },
  {
    id: "c-use-agentd", title: "use_agentd config-revert bug",
    model: "claude-haiku", updated: "3h", tokens: "9.1k",
    state: "failed", worktree: null,
  },
  {
    id: "c-bubbles", title: "Per-conversation run state",
    model: "claude-sonnet", updated: "yesterday", tokens: "54.8k",
    state: "idle", worktree: null,
  },
];

// ── Active conversation transcript ──────────────────────────────
// User → assistant → live agent-activity → flow-delivered result + artifact cards
const TRANSCRIPT = [
  {
    kind: "user",
    text: "Run research-digest on the conductor-correctness audit and post the findings here, with the artifacts inline.",
  },
  {
    kind: "assistant",
    text: "Launching **research-digest** in this conversation's worktree. I'll deliver the full digest plus artifacts when the run finishes — you'll see live stage progress in the Run panel.",
  },
  // the live, conversation-scoped activity bubble (per-conversation run state)
  {
    kind: "activity",
    flow: "research-digest",
    runId: "run_8f3ac1",
    stage: "stress-test",
    detail: "reflecting on digest — checking claim support",
    elapsed: "1m 12s",
  },
  // the auto-delivered result (terseness fix: full handoff artifact, not a paraphrase)
  {
    kind: "delivery",
    flow: "research-digest",
    ok: true,
    steps: 5,
    runId: "run_8f3ac1",
    body:
      "## Conductor correctness — digest\n\n" +
      "The scheduler is **correct**: a step is ready when every id in its `needs` is `done`, " +
      "and concurrent-ready steps run in parallel via a `JoinSet`. The defect is authoring-side — " +
      "a `reflect` step that declares `needs=[]` and relies only on `target=` becomes an **entry node**, " +
      "so it runs at t=0 and the printed `order:` is misleading.\n\n" +
      "**Two HIGH-severity flows** need the fix; `validate` should reject the foot-gun going forward.",
    artifacts: [
      {
        name: "ANSWER.md", kind: "markdown", lines: 42, bytes: "3.1 KB",
        preview:
          "# Findings\n\n1. research-digest · stress-test → needs=[\"digest\"]\n2. system-doctor · review → needs=[\"diagnose\"]\n\nHarden `validate`: a reflect target MUST appear in `needs`.",
      },
      {
        name: "stages.json", kind: "code", lines: 11, bytes: "412 B",
        preview:
          "{\n  \"research-digest\": [\n    [\"ingest\"],\n    [\"digest\", \"claims\"],\n    [\"stress-test\"],\n    [\"answer\"]\n  ]\n}",
      },
      {
        name: "claims.json", kind: "code", lines: 24, bytes: "1.4 KB", collapsed: true,
        preview: "[ { \"claim\": \"scheduler is correct\", \"support\": \"supervisor.rs JoinSet\" } ]",
      },
    ],
  },
];

// ── The conductor stages() contract for the active run ──────────
// research-digest → [[ingest],[digest,claims],[stress-test],[answer]]
const RUN = {
  flow: "research-digest",
  runId: "run_8f3ac1",
  status: "running",          // running | success | failed
  elapsed: "1m 12s",
  conversationId: "c-flow-delivery",
  stages: [
    { label: "Stage 0", tag: "entry", steps: [
      { id: "ingest", kind: "task", state: "done", ms: 8200 },
    ]},
    { label: "Stage 1", tag: "parallel", steps: [
      { id: "digest", kind: "task", state: "done", ms: 21400 },
      { id: "claims", kind: "task", state: "done", ms: 18700 },
    ]},
    { label: "Stage 2", tag: "", steps: [
      { id: "stress-test", kind: "reflect", state: "running", ms: null, note: "needs=[\"digest\"]" },
    ]},
    { label: "Stage 3", tag: "", steps: [
      { id: "answer", kind: "task", state: "pending", ms: null },
    ]},
  ],
  terminal: [
    { t: "info", s: "conductor: research-digest · run_8f3ac1 (worktree-flow-delivery)" },
    { t: "ok",   s: "stage 0  ingest        done   8.2s" },
    { t: "ok",   s: "stage 1  digest        done  21.4s   ┐ parallel" },
    { t: "ok",   s: "stage 1  claims        done  18.7s   ┘" },
    { t: "run",  s: "stage 2  stress-test   running…      reflect · target=digest" },
    { t: "dim",  s: "stage 3  answer        pending" },
  ],
};

// ── Worktree state for the active conversation ──────────────────
const WORKTREE = {
  branch: "worktree-flow-delivery",
  base: "head",
  path: ".oxide/worktrees/flow-delivery",
  changed: true,
  files: [
    { path: "overlay-rs/src/app/update.rs", add: 41, del: 12 },
    { path: "oxidemx-agent-core/src/tools.rs", add: 28, del: 4 },
    { path: "agentd/src/run_bridge.rs", add: 9, del: 2 },
  ],
};

// ── Resolved .oxide config (walk-up merge) ──────────────────────
const OXIDE_LAYERS = [
  { scope: "working dir", path: ".oxide/settings.toml", note: "closest · wins", root: false },
  { scope: "repo root", path: "~/code/oxidemx-phase1/.oxide/settings.toml", note: "root = true", root: true },
  { scope: "user global", path: "~/.config/oxidemx", note: "underlay", root: false },
];
const OXIDE_RESOLVED = {
  model: "claude-sonnet",
  permissions: { allow: ["edit", "shell:git", "shell:cargo"], deny: ["shell:rm -rf", "net:*"] },
  skills: 6, agents: 3, flows: 5, mcp: 2,
};

window.FX = {
  THEME, ACCENTS, PROJECTS, CONVERSATIONS, TRANSCRIPT,
  RUN, WORKTREE, OXIDE_LAYERS, OXIDE_RESOLVED,
  makeTheme,
};
