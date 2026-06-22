# Freya v0.4.0-rc.23 — Verified API Notes

Verified against the cloned source at `/run/media/system/fastdrive/repos/freya`
(commit checked 2026-06-22) and confirmed by a successful build + passing smoke
test. This file is the authoritative API tiebreaker for Tasks 2–11.

## Sub-crates resolved

Path deps used in `oxide-app/Cargo.toml`:

| Dep key | Path |
|---|---|
| `freya` | `crates/freya` |
| `freya-testing` | `crates/freya-testing` |

`freya` re-exports everything needed through its `prelude`. No need to depend
directly on `freya-core`, `freya-components`, `freya-animation`, `freya-router`,
`freya-winit`, or `torin` — all their public APIs flow through `freya::prelude::*`.

`freya-testing` is a separate crate (not re-exported by `freya`) and must be an
explicit dev-dependency when writing headless tests.

## Launch shape

```rust
use freya::prelude::*;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();       // keep the runtime alive for spawn()
    launch(
        LaunchConfig::new().with_window(
            WindowConfig::new(app)
                .with_size(1200., 800.)
                .with_title("OxideMX"),
        ),
    )
}

fn app() -> impl IntoElement { ... }
```

- `LaunchConfig` and `WindowConfig` are in `freya_winit::config`, re-exported via
  `freya::prelude`.
- The root component signature is `fn() -> impl IntoElement` (zero args).
- `launch` (in `freya::prelude`) wraps `freya_winit::launch` and auto-adds the
  performance-overlay plugin in debug builds.

## Element builders

All builders are in `freya::prelude::*` (sourced from `freya_core::prelude`).
The `rsx!` macro does NOT exist in rc.23 — use the builder API exclusively.

### `rect()`

Builder struct: `Rect`. Implements `LayoutExt`, `ContainerExt`, `StyleExt`,
`TextStyleExt`, `AccessibilityExt`.

Key methods (all take `&mut self -> Self` except where noted):

```rust
rect()
    .width(Size::fill())          // Size::fill() = fill available width
    .height(Size::fill())         // Size::fill() = fill available height
    .expanded()                   // shorthand: width(fill) + height(fill)
    .direction(Direction::Vertical | Direction::Horizontal)
    .main_align(Alignment::Center | Alignment::Start | Alignment::End)
    .cross_align(Alignment::Center | Alignment::Start | Alignment::End)
    .spacing(f32)                 // gap between children
    .padding(Gaps::new_all(f32))
    .background((r: u8, g: u8, b: u8))   // Color::from via tuple
    .background(Color::WHITE)
    .color(Color::WHITE)          // text color (inherited by children)
    .font_size(f32)               // via TextStyleExt; impl Into<FontSize>
    .corner_radius(CornerRadius::new_all(f32))
    .on_mouse_up(move |_| { ... })
    .child(impl IntoElement)
    .children(impl IntoIterator<Item = Element>)
    .maybe_child(Option<impl IntoElement>)
```

### `label()`

Builder struct: `Label`. Implements `TextStyleExt`, `ContainerExt`, `LayoutExt`.

```rust
label()
    .text("hello")              // impl Into<Cow<'static, str>>
    .font_size(24.0)            // via TextStyleExt; impl Into<FontSize>
    .color(Color::WHITE)        // via TextStyleExt; text color
    .max_lines(Some(3))         // or None for unlimited
    .line_height(Some(1.5))     // or None for default
```

NOTE: `label()` does NOT have a `.background()` method — background is on `rect`.
Text/color/font_size are inherited from ancestor `rect` if not set on the label.

### Sizes

```rust
Size::fill()      // fill available space
Size::px(f32)     // fixed pixel size
Size::flex(1.0)   // flex fraction
```

Shorthand: `.expanded()` on `rect()` calls `.width(Size::fill()).height(Size::fill())`.

### Colors

```rust
Color::WHITE
Color::BLACK
Color::from_rgb(r: u8, g: u8, b: u8)
Color::from_argb(a: u8, r: u8, g: u8, b: u8)
(r: u8, g: u8, b: u8)   // impl Into<Color> tuple
```

Passing a bare `(5, 7, 11)` literal requires explicit type annotation or
casting `(5u8, 7u8, 11u8)` to help the compiler resolve the `Into<Color>` impl.

## State and reactivity

```rust
let mut s = use_state(|| initial_value);
*s.read()          // immutable deref
*s.write()         // mutable deref — schedules re-render
s.set(value)       // replace the whole value
s.peek()           // read without subscribing
```

Async spawn:

```rust
spawn(async move {
    // ... await something ...
    *s.write() = new_value;  // signals re-render
});
```

`use_future(|| async { ... })` — runs once on mount.
`use_side_effect(closure)` — runs after every render.

## Testing (freya-testing)

```rust
use freya_testing::prelude::*;  // pulls in freya::prelude::* + TestingRunner + launch_test

// Simple:
let mut t = launch_test(app);
t.sync_and_update();

// Find a label by text:
let found = t.find(|_, el| {
    Label::try_downcast(el)
        .filter(|l| l.text.as_ref().contains("OxideMX"))
});
assert!(found.is_some());
```

`Label::try_downcast(element: &dyn ElementExt) -> Option<LabelElement>` — the
element type passed to the matcher closure is `&dyn ElementExt`. `LabelElement`
has a `text: Cow<'static, str>` field.

`Rect::try_downcast(element: &dyn ElementExt) -> Option<RectElement>`.

`TestingRunner::new(app, Size2D::new(w, h), |runner| { ... inject contexts ... }, scale)
    -> (TestingRunner, ...)`.

## Feature flags

The `freya` crate has `default = ["winit"]`. No extra features needed for the
hello-window app. To use routing: add `features = ["router"]` on the `freya`
dep. Router then available as `freya::router::*` or via `freya_router::*`.

## Build recipe (atomic Fedora host — no devel RPMs)

Host has the versioned `.so.X` runtime libs but NOT the unversioned `.so`
linker stubs (those ship in `-devel` RPMs). Fix: local symlink dir + `LIBRARY_PATH`.

```bash
mkdir -p /tmp/oxidemx-lib-links
ln -sf /usr/lib64/libEGL.so.1        /tmp/oxidemx-lib-links/libEGL.so
ln -sf /usr/lib64/libGL.so.1         /tmp/oxidemx-lib-links/libGL.so
ln -sf /usr/lib64/libGLESv2.so.2     /tmp/oxidemx-lib-links/libGLESv2.so
ln -sf /usr/lib64/libfreetype.so.6   /tmp/oxidemx-lib-links/libfreetype.so
ln -sf /usr/lib64/libfontconfig.so.1 /tmp/oxidemx-lib-links/libfontconfig.so
ln -sf /usr/lib64/libwayland-egl.so.1 /tmp/oxidemx-lib-links/libwayland-egl.so

LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build --bin oxide-freya
LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test --package oxide-freya
```

Skia binaries download automatically on first build (needs network + `clang` is
NOT required — prebuilt binaries are fetched). First build: ~5–15 min.
Subsequent incremental builds: seconds.

The `claude_development` distrobox is NOT required on this machine — the
host-side symlink workaround is sufficient.

## What freya re-exports (summary)

Through `freya::prelude::*`:
- All element builders: `rect()`, `label()`, `svg()`, `paragraph()`
- All builder traits: `ContainerExt`, `StyleExt`, `TextStyleExt`, `LayoutExt`, etc.
- Element types: `Label`, `LabelElement`, `Rect`, `RectElement`, etc.
- Layout types from `torin`: `Alignment`, `Direction`, `Size`, `Gaps`, `Position`
- Color: `Color`
- State: `use_state`, `spawn`, `use_future`, `use_side_effect`
- Launch: `launch`, `LaunchConfig`, `WindowConfig`
- Component trait: `Component`, `IntoElement`, `Element`
- Event types: `MouseEventData`, `KeyboardEventData`, etc.
