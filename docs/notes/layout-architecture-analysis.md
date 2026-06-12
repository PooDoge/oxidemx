# Layout & hit-testing architecture analysis (overlay-rs)

2026-06-12. Research + diagnosis only — no code changed. Companion to `lessons.md`.
File refs use the post-rename paths (`radial.rs` → `radial/mod.rs`).

## 1. Current architecture map

One iced 0.14 xdg-toplevel, created frameless/topmost at **chat size** (width ≥ 484, height ≥ 560,
from `chat_shell::effective_window_size`) and never resized except a grip-commit
(`app.rs::run`, comment at top: per-frame Wayland resizes desync the wgpu surface).
The GNOME extension positions it (`MoveOverlay` D-Bus call in `app.rs` Show handler);
the disc lives permanently in the window's **top-left 484×484 square**.

Three rendering/interaction layers share one `Stack` (`app.rs::view`, ~l.1132–1746):

| Layer | What | Geometry owner | Hit-test owner |
|---|---|---|---|
| 12 wgpu `Shader` widgets (drop_shadow, aurora, sdf_ring, disc_bevel, specular_sweep, slice_bevel, center_dome, hover_glow, hover_tilt, ripple, page_fx, dispatch_burst) | `render/*.rs` programs, each `Length::Fixed(484)` | each shader block in `view()` re-derives normalised radii from `geometry.rs` constants (`/ half_extent`, ±6 px insets, `26.0` icon-bg literal) | none (paint-only) |
| 2 `canvas::Program` painters | disc `Painter` (`radial/mod.rs` ~l.1626) and `CapsPainter` (`chat_shell.rs` ~l.235) | disc: `geometry.rs` constants + `Geometry::default()`; caps: `cap_rects`/`puck_geom` lerps | `Painter::update` → `input::slice_index_at` + `subitem_at`; `CapsPainter::update` → `hit_grip`/`hit_drag`/puck-circle distance checks |
| Regular iced widgets | `chat_ui/` (header, threads strip, body, footer, memories, tasks) | iced flex layout (`column!`/`row!`/`container`), with hard-coded paddings mirroring cap constants (`EDGE_PAD + HEADER_H`, `14.0 + 32.0 + 10.0` puck slot in `header.rs`) | iced widget tree (buttons capture their own presses) |

Geometry truth is therefore split four ways: `geometry.rs` constants, per-shader-block
normalisation arithmetic in `view()`, `chat_shell::cap_rects`/`puck_geom` at morph t,
and `chat_ui` paddings that *re-state* the cap geometry numerically. State-side hit
testing (`RadialState::update_pointer`, `handoff.rs` activation rule) is a fifth consumer.

## 2. Diagnosed structural issues

**a) No single source of geometry truth.** The wedge inner/outer radii appear as
`CENTER_ZONE_RADIUS + 6.0` / `MENU_RADIUS − 6.0` in four shader blocks of `app.rs::view`
*and* as `RING_INNER_INSET`/`RING_OUTER_INSET` in `radial/mod.rs`. `ICON_BG_RADIUS = 26.0`
is deliberately duplicated as a literal in `app.rs` (comment ~l.1252 says re-exporting was
avoided). Slice bisector math (`idx * 2π/n − π/2`) is re-derived in hover_glow, hover_tilt,
and `dispatch_burst::slice_origin`. Any radius tweak is currently a multi-file hunt.

**b) Hit constants drift from paint code — already, measurably.** The submenu arc is
*rendered* at `SUBITEM_RENDER_SPREAD_DEG = 18°` but *hit-tested* at
`SUBITEM_HIT_SPREAD_DEG = 15°` (`radial/mod.rs` ~l.35) — the hit circles for off-bisector
sub-items are centred on points where nothing is drawn (inherited from the Python overlay,
but it's a live geometric error, worst on 4+-item submenus). Likewise `CapsPainter`
hit-tests the puck against `puck_geom(t) + HEADER_PUCK_HIT_SLOP` while `header.rs`
reserves its widget slot with the independent literal `14.0 + 32.0 + 10.0`; `hit_grip`'s
`GRIP_ZONE = 22.0` is unrelated to the grip glyph actually painted (10 px arms at −5,−5).
Nothing fails the build when these diverge.

**c) Stack + aligned-container mesh-clipping bug.** Centering the 484 px disc stack in a
wide chat window via an aligned container makes iced 0.14 clip canvas **mesh** layers with
a doubled offset — wedge/dome fills vanish while text/images survive (`app.rs` ~l.1652,
commit 7fb5f08). Workaround: disc pinned top-left forever, which then forces `cap_rects`
to sweep from the top-left square, makes the Show handler offset by disc-half rather than
window-half, and bakes the asymmetry into every morph formula. A rendering bug has become
a layout architecture.

**d) Fixed-484 assumptions everywhere.** `WINDOW_SIZE` is derived (geometry.rs) but
consumed as an absolute by all 12 shader widgets (`Length::Fixed`), the disc canvas,
`input::HALF`, and the Show-centering math. A user-configurable menu radius (the stated
intent of `geometry::Geometry`) currently cannot happen: `Geometry` is a dead-code wrapper
that nothing parameterises. There is no `responsive`/window-size-driven path for the disc.

**e) Glyph-in-button centering is font-dependent guesswork.** `header.rs`/`footer.rs`
center text glyphs (✕ ✱ ◔ ＋ ➤ ■) in 32/40 px buttons via `center()` + `line_height(1.0)`
+ per-glyph size tuning. Optical centering varies per glyph and per font fallback;
emoji fail silently (lessons.md). Meanwhile the project *already owns* a tinting SVG
raster pipeline (`render/icons.rs` + `oxidemx-icons`) used by the disc — the chat chrome
just doesn't use it.

**f) Per-frame full tessellation.** Neither painter uses `canvas::Cache`; `Frame::new` +
full path building runs on every 16 ms tick while visible, even when only a shader uniform
(ripple progress) changed. Static geometry (wedge outlines, parked caps) is re-tessellated
identically frame after frame.

**g) The window is its own positioning system.** xdg-toplevel + extension `MoveOverlay`
+ `RaiseOverlay` for focus + "never resize" rule + monitor-clamp on persist are all
consequences of GNOME's positioning model (see §3d) — costs accepted knowingly, but worth
restating: the overlay's screen placement, focus, and z-order all live outside the app.

## 3. What comparable projects do, and recommendations

Ranked by payoff ÷ effort. Web survey (June 2026): COSMIC/libcosmic, iced 0.14 +
issue tracker, egui, GNOME Shell/Clutter, Waybar/eww, iced_layershell, taffy.
The one rule every healthy system surveyed enforces and we violate: **a single
per-frame geometric model, derived from layout-provided bounds, feeds both painting
and hit-testing.**

### R1 — Single per-frame `SceneGeometry` (high payoff, low effort) — DO NOW
egui's core invariant: `ui.allocate_response(size, sense)` returns a `Response` whose
`rect` is **both** what you paint into and what hover/click state is computed against —
a widget cannot disagree with itself about where it is. Clutter is the retained-mode
version: every actor's `pick()` uses the same allocation + transform pipeline as
`paint()`, and fancy GPU output never participates in picking (GNOME Shell blog,
"The Art of (Not) Painting Pixels"). taffy embedders (Bevy UI, Blitz/Dioxus, Zed's GPUI
fork) follow the same shape: one layout engine produces rects; paint and hit-test both
consume them. Port the idea without adopting any framework: extend `geometry::Geometry`
from a dead constants wrapper into a `SceneGeometry` computed once per frame from
`(bounds/win_size, menu_alpha, morph_t, slot_count)` — the polar analogue of taffy's
`Layout` output:

```rust
pub struct SceneGeometry {
    pub disc: DiscGeom,      // cx, cy, outer_r, inner_r, icon_r, icon_bg_r, ring insets
    pub wedges: Vec<Wedge>,  // bisector_rad, half_sweep, per-slot
    pub submenu_items: Vec<Circle>,  // ONE spread constant, used by paint AND hit
    pub caps: CapRects, pub puck: Circle, pub grip: Rect,
    pub norm: NormFactors,   // half_extent divisions for shader uniforms
}
```
Both painters' `draw()` and `update()`, `RadialState::update_pointer`, and every shader
block in `view()` consume it. The 18°/15° submenu drift dies by construction (or is kept
as an explicit `hit_slop` field — a decision, not an accident). `26.0` and the ±6 insets
get one home. Fits LLM-assisted development: one ~150-line `geometry/scene.rs`, pure
functions, trivially unit-testable (the `cap_rects` test style already exists).

### R2 — Hit regions generated alongside paint (high payoff, low effort) — DO NOW
With R1 in place, add a tiny region enum instead of inline distance math:

```rust
pub enum Region { Circle { c: Point, r: f32 }, Annulus {..}, Wedge {..}, Rect(Rectangle) }
pub struct HitMap { regions: Vec<(Region, Target)> }  // Target = Slice(i) | SubItem(i,j) | Puck | Grip | HeaderDrag | CenterZone
```
`SceneGeometry` emits the `HitMap`; `Painter::update`/`CapsPainter::update` become
"first region containing p wins" loops. This is the egui Response model minus immediate
mode — no framework change, and `update()` shrinks to event→message routing. The hit map
is also a free debug overlay (draw the regions translucent under a dev env var) and a
free test target ("every painted interactive thing has a region").

### R3 — Icons: SVG/canvas, not font glyphs (medium payoff, low effort) — DO NEXT
COSMIC applets use `cosmic::widget::icon` (themed SVG) rather than glyphs precisely for
deterministic sizing. The fix here is internal reuse: route header/footer buttons through
`oxidemx-icons` → `iced::widget::image`/`svg` at exact 18–20 px inside the 32/40 px
buttons (the `IconCache` already memoises handles). Per-glyph size/line-height tuning
disappears; emoji-coverage trap (lessons.md) disappears. The ✕/➤/■ set is ~6 tiny SVGs
in `assets/`.

### R4 — `canvas::Cache` split static/dynamic (medium payoff, low effort) — DO NEXT
iced's canvas is built around `Cache` (tessellate once, reuse until `clear()`); 0.14
even added `Cache::draw_with_bounds` (iced #3035) for exactly this. Split each
painter's output: static-per-state geometry (wedge fills/strokes/icons at current page,
parked caps) into a `Cache` cleared only on state change (page, theme, highlight target,
morph-active), per-frame bits (highlight tweens, travelling caps mid-morph, badge) drawn
fresh. During idle-open (the common case: menu up, nothing moving) tessellation cost drops
to ~zero; the 60 Hz tick already short-circuits in `update()`, this makes `draw()` match.
Caveat: during the morph everything is animated anyway — cache only helps parked states,
which is exactly when the chat is up for minutes.

### R5 — Track iced upstream for clipping + bounds fixes (low effort, watch) — DO NEXT
The mesh-clip bug (§2c) is an iced renderer bug class, not a design truth, and it is an
actively-moving area upstream: #2216 (canvas cursor hit-test offset when the canvas isn't
at the window origin), #3040 (multiple canvases render wrong except the first), PR #2882
(clip bounds not transformed, tiny-skia), PR #2738 (shader widget render-pass **viewport**
wasn't set to the widget's clip bounds — scissor right, NDC mapping wrong), #2700
(zero/offscreen layer bounds break subsequent layers). Every one of these is a component
assuming window-space or stale bounds — hand-maintained pixel constants on our side
reproduce exactly the bug class iced keeps fixing. Two escapes: (a) since the repo
vendors libcosmic (System76's iced fork), identify and cherry-pick/patch the offset-mesh
clip; (b) when fixed, delete the top-left pinning and centre the disc via an aligned
container, simplifying `cap_rects` to symmetric math. Until then R1 confines the
asymmetry to one constructor. Adopt now regardless: trust the `bounds` handed to
`draw()`/`update()` (or `iced::widget::responsive`, which passes the available `Size` at
layout time) and derive everything from it — kills the fixed-484 `Length::Fixed` on the
12 shader widgets in favour of sizing from the computed disc rect.

### R6 — Layer-shell: evaluate honestly, don't migrate (informational) — DO LATER
Waybar/eww (via gtk-layer-shell) and COSMIC's panel get anchoring, exclusive zones, and
compositor-assigned sizing from `zwlr_layer_shell_v1` — a layer-shell client passes 0 for
an axis and the compositor assigns it in the configure event; clients never self-position.
`iced_layershell` (waycrate/exwlshelleventloop) brings this to iced. **But Mutter
explicitly refuses to implement wlr-layer-shell for third-party apps (mutter#973,
gnome-shell#1141), and iced_layershell has no fallback — it does not work on GNOME.**
On GNOME the only options are extension-assisted xdg (what we do) or living inside the
Shell as an extension. So: the xdg+extension design is the *correct* architecture on
GNOME, not a hack — keep all positioning/focus privilege in the extension and treat the
window's size/position as inputs that can change under us at any time (which
`Message::WindowResized` already does). *Optional later*: feature-gate an
`iced_layershell` backend so wlroots/KDE compositor users get native anchoring and the
extension becomes GNOME-only glue — only worth it if those users materialise. One COSMIC
idea worth stealing regardless: applets receive `suggested_size()` hints and wrap content
in an Autosize widget — size flows top-down from exactly one authority.

### R7 — egui-style immediate mode or retained scene-graph? — DO LATER (verdict: neither, mostly)
A full egui embed (or migration) buys the allocate/Response unification we already get
from R1+R2 at far lower cost, and loses iced's widget set (text_editor, scrollable,
markdown) that chat_ui leans on. A retained scene-graph (Clutter-style actors with
transforms + pick) is what you'd build if the disc grew nested, independently-animating,
reparentable elements — it doesn't have that today; `anim.rs` tracks + `SceneGeometry`
cover current needs. Revisit only if the editor (`editor/` stubs) turns into a free-form
direct-manipulation canvas.

### R8 — taffy for the chat panel — DO LATER (verdict: skip for now)
taffy is a standalone CSS block/flex/grid solver built to be embedded by frameworks that
*have no layout engine* (Bevy UI replaced stretch with it, bevy#4716; Blitz/Dioxus Native;
Zed maintains a GPUI fork). chat_ui already sits inside iced's flex layout, which handles
every column/row/fill case used — embedding taffy would add a second source of layout
truth, the exact disease this doc is about. The one real candidate, the header bar's
manual puck-slot spacer (`14.0+32.0+10.0` in `header.rs`), is fixed by R1 exporting
`puck_slot_width()`, not by a layout engine. Reconsider only if the chat panel grows
CSS-grid-shaped needs (wrapping/spanning card dashboard) — taffy's
`TaffyTree::compute_layout` + readback-rects API embeds cleanly if that day comes.

## 4. Plan

**Do now (1–2 sessions, pure refactor, no visual change):**
1. `overlay-rs/src/geometry/scene.rs`: `SceneGeometry` + `HitMap` (R1+R2). Small file,
   pure functions, unit tests asserting paint-radius == hit-radius for every region.
2. Migrate consumers mechanically: `radial/mod.rs::{update_pointer, Painter}`,
   `chat_shell.rs::{cap_rects callers, CapsPainter}`, the shader blocks in `app.rs::view`
   (uniform values come from `scene.norm`). Delete duplicated literals as they fall out.
3. Resolve the 18°/15° submenu spread: pick one number (or explicit slop) and encode it once.

**Do next:**
4. SVG icon set for chat chrome buttons through `oxidemx-icons` (R3).
5. `canvas::Cache` split in both painters (R4); measure with the existing vision-shot hook.
6. Identify/patch the mesh-clip bug in the vendored iced; on fix, un-pin the disc and
   simplify `cap_rects` (R5).

**Do later / watch:**
7. Optional `iced_layershell` backend for wlroots compositors (R6).
8. Re-evaluate scene-graph need when `editor/` becomes real (R7); taffy only on concrete
   grid-shaped requirements (R8).

Every step is a small single-purpose module or a mechanical consumer migration —
the shape that survives LLM-assisted editing: short files, one concept per file,
constants with exactly one definition, and tests that fail when paint and hit drift.

## Key references

- iced bounds/clipping bug class: iced-rs/iced #2216, #3040, #2700; PRs #2882, #2738;
  0.14 release notes (#3033 layer merging, #3035 `Cache::draw_with_bounds`);
  `iced::widget::responsive` docs.
- egui `Response`/`allocate_response` docs (paint rect == hit rect invariant).
- Clutter `Actor::pick` docs + GNOME Shell blog "The Art of (Not) Painting Pixels"
  (GPU output is pick-transparent; picking is CPU-side geometry under shared transforms).
- wlr-layer-shell protocol; gtk4-layer-shell; Waybar (wlroots-only); GNOME refusal:
  mutter#973, gnome-shell#1141; waycrate/exwlshelleventloop (`iced_layershell`).
- libcosmic book "Panel Applets" (`suggested_size`, Autosize widget); cosmic-panel
  applet system (layer-shell client + per-process applets over a nested Wayland server).
- taffy (DioxusLabs); Bevy adoption bevy#4716; Zed GPUI fork.
