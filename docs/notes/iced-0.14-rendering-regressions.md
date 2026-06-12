# iced 0.14 rendering regressions — upstream research (2026-06-12)

Scope: the stale-layer-at-idle bug, the doubled-offset mesh clipping, transparent-quad alpha
staleness, pin strategy, and Wayland gotchas. Researched against iced-rs/iced issues/PRs,
0.14.0 release notes, master history through June 2026, and pop-os forks.

## TL;DR

- **No exact upstream issue exists** for our headline bug (stale per-layer GPU content on
  screen at idle while `window::screenshot` is pristine). It belongs to a *well-documented
  bug class* in the 0.14 wgpu renderer — per-layer prepared-buffer caches getting out of
  sync with the layer stack — with three sibling instances already on file (#2700, #3272,
  #3138). We should file our reproducer; the screenshot-vs-surface divergence is strong
  evidence upstream doesn't have yet.
- **Master does not fix any of this.** The relevant fix PRs (#3292, #3148, #3291, #3157)
  are still **open/unmerged** as of 2026-06-12, and master moved to wgpu 28 (Jan 2026,
  [#3219](https://github.com/iced-rs/iced/pull/3219)) then wgpu 29 (May 2026,
  [#3323](https://github.com/iced-rs/iced/pull/3323)) — incompatible with our wgpu 27 pin.
- **Verdict: stay on 0.14.0 + app-level cache busters.** There is no rev or fork worth
  pinning to today.

## 1. The bug class: layer-indexed GPU caches served stale

Since the 0.14 renderer rework, `iced_wgpu` keeps *per-layer* prepared GPU state (quad
instance buffers, mesh vertex buffers, text atlas entries, image instances) indexed by the
layer's position in `graphics::layer::Stack`. Two 0.14 features made the stack layout
*dynamic between frames*:

- **Layer merging** — [#3033](https://github.com/iced-rs/iced/pull/3033) (merged 2025-08-17,
  shipped in 0.14.0): layers A/B merge when bounds match and draw order permits. Merge
  decisions can flip frame-to-frame as content changes (e.g. a quad fading to alpha 0, a
  canvas that stops animating), changing layer counts/indices.
- **Reactive rendering** — [#2662](https://github.com/iced-rs/iced/pull/2662): redraws only
  happen on update or explicit `request_redraw`; widgets/diffing decide. An opt-out cargo
  feature exists: `unconditional-rendering`.

Any code path that skips or reorders a layer **without advancing the prepared-buffer
indices** then draws *the previous frame's GPU buffers* for every subsequent layer. Known
instances of exactly this pattern:

- [#2700](https://github.com/iced-rs/iced/issues/2700) — "Layer rendering breaks if one is
  not drawn at any point": zero-area or off-screen layers skipped without bumping
  `quad_layer`/`mesh_layer`/`text_layer` indices → corrupted/stale subsequent layers.
  **Fixed in 0.14.0** by [#2701](https://github.com/iced-rs/iced/pull/2701).
- [#3272](https://github.com/iced-rs/iced/issues/3272) — same pattern resurfaced in the
  **image** pipeline: early `return` in `image::State::prepare` (introduced by the pixel
  snapping commits `0fe99b19`/`1463ec84`) skips layer finalization, "leaving **stale
  instances** in the buffer vector". **Open**; fix
  [#3292](https://github.com/iced-rs/iced/pull/3292) (`return` → `continue`) **unmerged**.
- [#3138](https://github.com/iced-rs/iced/issues/3138) — "gl backend causes major graphical
  glitches" on resize with per-frame meshes; reporter suspects "faulty caching" in the
  pipeline. **Open, no fix.** Matches our GL repro.
- [#3147](https://github.com/iced-rs/iced/issues/3147) — canvas `draft()`/`paste()` frames
  drawn in wrong order on wgpu only. Fix [#3148](https://github.com/iced-rs/iced/pull/3148)
  **open/unmerged**.
- [#3173](https://github.com/iced-rs/iced/issues/3173) — canvas `Cache` regression vs
  0.13.1 (flicker/white content, worse with size). **Open.**

Our symptom set maps cleanly: ~10 shader layers + 2 canvas layers + quads/text produce many
merge boundaries; when one layer's content goes static or empty (fade reaches α=0, mesh
animation ends), the stack layout shifts and index-keyed caches serve the old buffers.
`window::screenshot` uses the offscreen/headless path
([#2857](https://github.com/iced-rs/iced/pull/2857)-era), which builds fresh layer state
every call — hence always pristine. Interactive resize forces full re-prepare (surface
reconfigure) — hence temporarily correct. Per-frame geometry jitter keeps a layer's merge
slot "live" — hence cache busters work.

**Doubled-offset mesh clipping** (canvas inside `align_x`-centered container): no exact
issue found. Nearest neighbors: [#3291](https://github.com/iced-rs/iced/pull/3291) (open —
mesh clip bounds corrupted by the transform/snap pipeline, NaN for infinite rects),
[#3113](https://github.com/iced-rs/iced/issues/3113) (container background ignores clip
under `Pin` negative offset), and the pixel-snapping rework
([#2962](https://github.com/iced-rs/iced/pull/2962) + commits above) that already caused
#3272. The snap/transform path applying the container translation to *both* the mesh
transform and its clip rect is consistent with all of these. **Worth filing too.**

**Transparent-quad alpha staleness** is not a separate upstream bug — it's the same stale
layer mechanism (quad buffer frozen at mid-fade alpha). Window-level transparency itself
was fixed pre-release ([#2727](https://github.com/iced-rs/iced/issues/2727), closed
2025-01).

## 2. Pin strategy

| Option | Verdict |
|---|---|
| iced master (June 2026) | **No.** wgpu 29 (breaks our `wgpu = "27"` lockstep + shader::Program code), and #3292/#3148/#3291/#3157 are *still unmerged* — master gains us nothing on these bugs. |
| master rev pre-[#3219](https://github.com/iced-rs/iced/pull/3219) (last wgpu-27 rev, ~mid-Jan 2026) | Marginal. Carries only SHADER_F16 fixes ([#3164](https://github.com/iced-rs/iced/pull/3164)) and text hinting for fractional scale ([#3145](https://github.com/iced-rs/iced/pull/3145)) beyond 0.14.0. None of our bugs. Not worth losing crates.io. |
| pop-os/iced (`master`, 0.14-based; `prev-master-0.14.0-dev`) | **No.** Heavily diverged shell (sctk/layer-shell integration for libcosmic), same `iced_wgpu` core — does not carry the missing fixes; cosmic has its own fractional-scale regressions ([cosmic-comp#1050](https://github.com/pop-os/cosmic-comp/issues/1050)). Only interesting if we ever want their sctk layer-shell shell wholesale. |
| Fork + cherry-pick | Viable later: fork `iced-rs/iced` at tag `0.14.0`, cherry-pick [#3292](https://github.com/iced-rs/iced/pull/3292) (1-line class fix, image path) and [#3148](https://github.com/iced-rs/iced/pull/3148) if we hit those exact paths. Neither fixes the merge-layout staleness, so don't bother until our own upstream issue gets a fix commit to pick. |

**Release outlook:** no 0.14.1 exists six months post-release; iced ships minors roughly
yearly and patch releases rarely. Expect our fixes only after an upstream issue with a
minimal repro exists. Realistic horizon: 0.15 (wgpu 29), late 2026 at the earliest —
which forces a wgpu 27→29 port of our shader stack anyway.

## 3. Recommended actions

1. **Immediate (keep):** 0.14.0 + per-layer cache busters. Risks: ~0 — sub-pixel geometry
   jitter is invisible and only defeats the merge/prepare cache; cost is a few redundant
   uploads/frame. Additionally:
   - Avoid layers that *become* empty/zero-area/fully-transparent: clamp fades to α≥0.004
     (1/255) instead of 0, keep placeholder primitives mounted — this stabilizes the layer
     stack layout, attacking the root cause rather than per-layer freshness.
   - Test the `unconditional-rendering` cargo feature (opt-out of #2662). It won't fix
     index desync by itself (our evidence shows redraws alone don't refresh stale layers)
     but changes redraw cadence and is the first thing upstream will ask about.
2. **Short-term:** file two upstream issues with minimal repros: (a) stale layer content at
   idle, screenshot-pristine, resize-fresh, GL+Vulkan, linking #2700/#3272/#3138 as the
   sibling pattern; (b) doubled container offset in mesh clip bounds under `align_x`,
   linking #3291/#3113. Subscribe to #3292/#3148/#3173/#3138.
3. **No pin change now.** Re-evaluate when (a) our filed issue gets a fix commit (then
   fork-at-0.14.0 + cherry-pick), or (b) 0.15 lands (then port wgpu 27→29 in lockstep).

## 4. Wayland gotchas catalogue (relevant to overlay-rs)

- **Programmatic resize is a no-op for layout:** `window::resize` scales content without
  layout recompute — [#3156](https://github.com/iced-rs/iced/issues/3156) (open); fix PR
  [#3157](https://github.com/iced-rs/iced/pull/3157) injects a resize event (unmerged).
  Underlying winit `request_inner_size` on Wayland is async + gated on the compositor ack;
  same-size requests are silently dropped. Workaround: treat resize as async, drive layout
  off the `window::Event::Resized` we actually receive.
- **AlwaysOnTop:** winit `WindowLevel::AlwaysOnTop` is unimplemented on Wayland (no xdg
  protocol for it) — keep-above must come from the compositor side (our GNOME extension)
  or layer-shell. Don't trust the iced/winit call to do anything.
- **Surface Lost/Outdated:** 0.14.0 includes the reconfigure-on-`SurfaceError` fix
  ([#3067](https://github.com/iced-rs/iced/pull/3067)) — relevant to NVIDIA suspend/VT
  switches; if we see permanent black after resume, it's not this.
- **NVIDIA + Vulkan on Wayland compositors** can fail outright where GL works
  ([niri#1910](https://github.com/niri-wm/niri/issues/1910)) — keep the
  `WGPU_BACKEND=gl` escape hatch documented, but note GL has its own caching glitches
  ([#3138](https://github.com/iced-rs/iced/issues/3138)).
- **Fractional scaling:** text metrics hinting for non-integral scale factors landed
  *after* 0.14.0 ([#3145](https://github.com/iced-rs/iced/pull/3145), master-only) — minor
  text blur/shimmer at 1.25/1.5 scale is expected on 0.14.0. Scale factor at window-open
  is also master-only ([#3279](https://github.com/iced-rs/iced/pull/3279)); on 0.14.0 the
  first frame may render at scale 1.0 before `ScaleFactorChanged` arrives — re-layout on
  that event, never cache the startup scale.
- **Multi-window on other platforms:** unfocused windows can starve of presents
  ([#3320](https://github.com/iced-rs/iced/issues/3320), Windows) — watch for an analogous
  frame-callback gating if we ever run multiple Wayland surfaces; Wayland frame callbacks
  stop for occluded surfaces by design.
- **Monitor standby** can freeze redraw scheduling ([#1870](https://github.com/iced-rs/iced/issues/1870),
  ancient, still open) — relevant for an always-running overlay; nudge with a
  `request_redraw` on `Event::Focused`/output-change.

## 5. Compatibility notes for our feature set

- 0.14.0 ↔ wgpu **27.0** ([#3097](https://github.com/iced-rs/iced/pull/3097)) — our pin is
  correct; any iced bump must move wgpu in lockstep (28 after #3219, 29 after #3323).
- `canvas`, `markdown`, `highlighter`, `text_editor`, `window::screenshot`, shader
  `Program` all exist on master with API drift: `Compositor::screenshot` lost its surface
  arg ([#2672](https://github.com/iced-rs/iced/pull/2672), already in 0.14.0); highlighter
  has an open double-init bug ([#3226](https://github.com/iced-rs/iced/issues/3226));
  canvas `draw_image` >2 MiB RGBA was broken until [#3256](https://github.com/iced-rs/iced/issues/3256)/
  [#3336](https://github.com/iced-rs/iced/pull/3336) (master-only fixes).


## Addendum (2026-06-12, late): full bisect results — the shell, not the layers

Controlled single-instance bisects (daemon stopped, signal-driven Show,
portal captures) eliminated every app-level suspect:

| Case | Result |
|---|---|
| Boot-time self-show, idle 20 s | disc visible, window transparent ✓ |
| Boot show + signal re-show (full Show flow) | disc fresh, window surround OPAQUE BLACK |
| …with iced window tasks skipped (move_to/minimize/focus) | still black |
| …with extension MoveOverlay skipped | still black |
| …with ripple + focused-class query also skipped (bare `state.show()`) | still black |
| Signal-only show (no boot show), Vulkan | ENTIRE window black, disc never composited |
| Signal-only show, **GL backend** | identical — rules out NVIDIA/Vulkan WSI and the empty-damage theory |

Conclusion: content rendered around map time reaches the compositor;
content first rendered on an already-idle window often never does, and
"empty" idle windows composite opaque black rather than transparent.
Combined with the (separately confirmed and mitigated) stale layer
caches, this is a defect cluster in iced 0.14's winit shell + present
path for our window lifecycle (long-lived, transparent, always-on-top,
show/hide cycles) — not in our painting, not in the driver, not in the
GNOME extension (pure `move_frame`/`raise`).

App-level mitigation is exhausted. The remaining paths are owning the
shell/renderer: (a) fork iced 0.14 in-repo, rip out conditional
rendering and layer caching (unconditional full prepare+draw+present
each RedrawRequested — we run 60 Hz anyway), wire via
`[patch.crates-io]`; (b) evaluate waycrate's exwlshelleventloop
architecture (their `layershellev`/`iced_layershell` replace winit
outright — layer-shell itself is a no-go on GNOME/Mutter, but their
event-loop-owning shell pattern is the blueprint if (a) doesn't
suffice). The `OXIDEMX_SHOW_SKIP` / `OXIDEMX_VISION_NOSHOW` gates used
for the bisect are kept in-tree for regression testing the fork.
