# Widget Plugin Foundations Implementation Plan (Plan 1 of 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the wire-format crate (`oxidemx-widget-proto`) and the config schema-v3 changes (`oxidemx-shared`) that every later widget-plugin piece depends on.

**Architecture:** A new dependency-light crate holds the host↔widget message types (postcard-encoded, envelope-tagged for forward compat), the Scene display-list, and the `widget.json` manifest schema. `oxidemx-shared` gains `WidgetSource::Custom`, the two-bag `WidgetStore` (`global` + `instances`), `schema_version: 3`, settings resolution, v2→v3 migration, and instance-key helpers. No runtime, no UI — everything is unit-testable with `cargo test`.

**Tech Stack:** Rust 2021 workspace, serde, postcard 1.x (alloc), serde_json. Spec: `docs/superpowers/specs/2026-06-12-widget-plugin-system-design.md` (rev 2).

**Conventions:** Run all commands from the repo root (`juhradial-mx/`). The workspace builds with stable Rust. Match existing test style in `oxidemx-shared/src/config.rs` (`#[cfg(test)] mod tests` at file bottom, plain asserts, no test deps).

---

### Task 1: `oxidemx-widget-proto` crate skeleton

**Files:**
- Create: `oxidemx-widget-proto/Cargo.toml`
- Create: `oxidemx-widget-proto/src/lib.rs`
- Modify: `Cargo.toml` (workspace members list, ~line 16)

- [ ] **Step 1: Create the crate manifest**

`oxidemx-widget-proto/Cargo.toml`:

```toml
[package]
name = "oxidemx-widget-proto"
version = "0.1.0"
edition = "2021"
description = "Wire types shared by the OxideMX widget host and widget guests"

[features]
default = ["manifest"]
# widget.json parsing — host/settings only; guests build with
# default-features = false to keep the wasm small.
manifest = ["dep:serde_json"]

[dependencies]
serde = { version = "1", features = ["derive"] }
postcard = { version = "1", features = ["alloc"] }
serde_json = { version = "1", optional = true }
```

- [ ] **Step 2: Create lib.rs with the API version constant and module stubs**

`oxidemx-widget-proto/src/lib.rs`:

```rust
//! Wire types shared by the OxideMX widget host (overlay-rs / settings-rs)
//! and widget guests (compiled to wasm32-wasip1).
//!
//! Everything crossing the wasm boundary is postcard-encoded inside a
//! tagged [`Envelope`] so unknown message kinds can be skipped instead of
//! failing the decode — that is the forward-compatibility story within an
//! `API_VERSION` (see the spec §2 / §8).

pub mod envelope;
pub mod event;
pub mod scene;
pub mod settings;
#[cfg(feature = "manifest")]
pub mod manifest;

pub use envelope::Envelope;
pub use event::{Event, HostCmd};
pub use scene::{Color, PathOp, Prim, Scene, Stroke, TextAlign, TextWeight, WedgeGeom};
pub use settings::SettingValue;
#[cfg(feature = "manifest")]
pub use manifest::{OptionSpec, SliceMeta, WidgetManifest};

/// Boundary API version. The host refuses widgets whose manifest
/// `api_version` is outside its supported range (just `== 1` for now).
pub const API_VERSION: u32 = 1;
```

Create empty placeholder files so the crate compiles module-by-module as
tasks land: `envelope.rs`, `event.rs`, `scene.rs`, `settings.rs`,
`manifest.rs` each containing only `//! see Task N` for now (they are filled
in Tasks 2–4; the lib.rs `pub use` lines for not-yet-written items can be
commented in as each module lands — start with all five modules present but
empty and the `pub use` block commented out, uncommenting per task).

- [ ] **Step 3: Add the crate to the workspace**

In root `Cargo.toml`, add to `members`:

```toml
    "oxidemx-widget-proto",
```

- [ ] **Step 4: Verify it builds**

Run: `cargo check -p oxidemx-widget-proto`
Expected: success (warnings about unused modules are fine).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml oxidemx-widget-proto
git commit -m "feat(widget-proto): crate skeleton + workspace member"
```

---

### Task 2: Scene display-list types + postcard round-trip

**Files:**
- Create: `oxidemx-widget-proto/src/scene.rs` (replace stub)
- Modify: `oxidemx-widget-proto/src/lib.rs` (uncomment scene re-exports)

- [ ] **Step 1: Write the failing test** (bottom of `scene.rs`, with the types referenced but not yet written above it — or write types and test together and rely on Step 2's red run via a deliberate `todo!()`; simplest honest TDD here: write the test first in the file, watch it fail to compile)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scene() -> Scene {
        Scene {
            prims: vec![
                Prim::Text {
                    x: 0.0, y: -6.0, content: "14°".into(), size: 18.0,
                    color: Color::Palette("yellow".into()),
                    weight: TextWeight::Bold, align: TextAlign::Center,
                },
                Prim::Sparkline {
                    x: -21.0, y: 4.0, w: 42.0, h: 11.0,
                    points: vec![0.3, 0.5, 0.4, 0.9],
                    color: Color::Rgba(255, 171, 107, 255),
                },
                Prim::Path {
                    ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(4.0, 4.0), PathOp::Close],
                    stroke: Some(Stroke { color: Color::Palette("teal".into()), width: 1.5 }),
                    fill: None,
                },
                Prim::Group {
                    dx: 2.0, dy: 2.0, scale: 0.5,
                    children: vec![Prim::Image { x: 0.0, y: 0.0, w: 16.0, h: 16.0, asset: "assets/sun.png".into() }],
                },
            ],
        }
    }

    #[test]
    fn scene_postcard_round_trip() {
        let scene = sample_scene();
        let bytes = postcard::to_allocvec(&scene).unwrap();
        let back: Scene = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(scene, back);
    }

    #[test]
    fn scene_caps_are_enforced() {
        let mut scene = Scene { prims: vec![] };
        for _ in 0..(MAX_SCENE_PRIMS + 1) {
            scene.prims.push(Prim::Text {
                x: 0.0, y: 0.0, content: "x".into(), size: 8.0,
                color: Color::Rgba(0, 0, 0, 255),
                weight: TextWeight::Regular, align: TextAlign::Left,
            });
        }
        assert!(scene.validate().is_err());
    }
}
```

- [ ] **Step 2: Run it to make sure it fails**

Run: `cargo test -p oxidemx-widget-proto scene`
Expected: compile error — the types don't exist yet.

- [ ] **Step 3: Implement the types above the tests**

```rust
//! Retained display list returned by a widget's `render()`.
//! Coordinates are wedge-local: origin at the icon anchor, +y down.

use serde::{Deserialize, Serialize};

/// Geometry of the slot the widget is rendering into, plus its hover
/// progress (0.0 = idle, 1.0 = fully hovered). Sent with every render call.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WedgeGeom {
    pub width: f32,
    pub height: f32,
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub angle_start: f32,
    pub angle_end: f32,
    pub hovered: f32,
}

/// Encoded-scene byte cap (spec §8): one strike if exceeded.
pub const MAX_SCENE_BYTES: usize = 64 * 1024;
/// Primitive-count cap, counted recursively through groups.
pub const MAX_SCENE_PRIMS: usize = 2048;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Color {
    /// Key into the user's active theme palette ("yellow", "teal", …).
    Palette(String),
    Rgba(u8, u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextWeight { Regular, Medium, Semibold, Bold }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign { Left, Center, Right }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Prim {
    Arc {
        cx: f32, cy: f32, radius: f32,
        start_angle: f32, end_angle: f32,
        stroke: Option<Stroke>, fill: Option<Color>,
    },
    Path {
        ops: Vec<PathOp>,
        stroke: Option<Stroke>, fill: Option<Color>,
    },
    Text {
        x: f32, y: f32, content: String, size: f32,
        color: Color, weight: TextWeight, align: TextAlign,
    },
    Sparkline {
        x: f32, y: f32, w: f32, h: f32,
        /// Normalised samples in 0.0..=1.0.
        points: Vec<f32>, color: Color,
    },
    /// Static image from the widget bundle's `assets/` dir; the host
    /// decodes and caches it.
    Image { x: f32, y: f32, w: f32, h: f32, asset: String },
    /// Translated/scaled subtree.
    Group { dx: f32, dy: f32, scale: f32, children: Vec<Prim> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Scene {
    pub prims: Vec<Prim>,
}

impl Scene {
    /// Recursive primitive count (groups count as 1 + children).
    pub fn prim_count(&self) -> usize {
        fn count(prims: &[Prim]) -> usize {
            prims.iter().map(|p| match p {
                Prim::Group { children, .. } => 1 + count(children),
                _ => 1,
            }).sum()
        }
        count(&self.prims)
    }

    /// Enforce the spec §8 caps. The host treats `Err` as a strike.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.prim_count();
        if n > MAX_SCENE_PRIMS {
            return Err(format!("scene has {n} prims (max {MAX_SCENE_PRIMS})"));
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Run the tests and make sure they pass**

Run: `cargo test -p oxidemx-widget-proto scene`
Expected: 2 passed.

- [ ] **Step 5: Commit**

```bash
git add oxidemx-widget-proto/src/scene.rs oxidemx-widget-proto/src/lib.rs
git commit -m "feat(widget-proto): Scene display list + caps + postcard round-trip"
```

---

### Task 3: Events, host commands, and the forward-compatible envelope

**Files:**
- Create: `oxidemx-widget-proto/src/event.rs` (replace stub)
- Create: `oxidemx-widget-proto/src/envelope.rs` (replace stub)
- Create: `oxidemx-widget-proto/src/settings.rs` (replace stub)
- Modify: `oxidemx-widget-proto/src/lib.rs` (uncomment re-exports)

Postcard is not self-describing — an unknown enum variant index is a hard
decode error. Forward compat therefore comes from the **envelope**: every
message crosses the boundary as `(tag: u32, payload: bytes)`. Receivers that
don't know a tag skip the message instead of erroring; known tags decode
their payload into the typed enum. Tags are append-only constants.

- [ ] **Step 1: Write the failing tests** (in `envelope.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, HostCmd};

    #[test]
    fn event_round_trips_through_envelope() {
        let events = vec![
            Event::Init,
            Event::Timer("refresh".into()),
            Event::MenuOpened { page: "Apps".into() },
            Event::MenuClosed,
            Event::SliceVisible,
            Event::SliceHidden,
            Event::Hover { entering: true },
            Event::Click,
            Event::Scroll { delta: -1.0 },
            Event::SettingsChanged,
            Event::HttpResponse { id: "w".into(), status: 200, body: b"{}".to_vec() },
        ];
        for ev in events {
            let bytes = encode_event(&ev).unwrap();
            let back = decode_event(&bytes).unwrap();
            assert_eq!(Some(ev), back);
        }
    }

    #[test]
    fn unknown_event_tag_is_skipped_not_an_error() {
        let env = Envelope { tag: 0xDEAD_BEEF, payload: vec![1, 2, 3] };
        let bytes = postcard::to_allocvec(&env).unwrap();
        assert_eq!(decode_event(&bytes).unwrap(), None);
    }

    #[test]
    fn host_cmd_round_trips_through_envelope() {
        let cmds = vec![
            HostCmd::SetTimer { id: "refresh".into(), secs: 900 },
            HostCmd::CancelTimer { id: "refresh".into() },
            HostCmd::HttpGet { id: "w".into(), url: "https://api.open-meteo.com/v1".into() },
            HostCmd::OpenUrl("https://example.com".into()),
            HostCmd::Exec("playerctl play-pause".into()),
            HostCmd::HapticPulse("tick".into()),
            HostCmd::Log { level: 1, msg: "hello".into() },
        ];
        for cmd in cmds {
            let bytes = encode_cmd(&cmd).unwrap();
            let back = decode_cmd(&bytes).unwrap();
            assert_eq!(Some(cmd), back);
        }
    }

    #[test]
    fn unknown_cmd_tag_is_skipped_not_an_error() {
        let env = Envelope { tag: 0xFFFF_0001, payload: vec![] };
        let bytes = postcard::to_allocvec(&env).unwrap();
        assert_eq!(decode_cmd(&bytes).unwrap(), None);
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p oxidemx-widget-proto envelope`
Expected: compile error (types/functions missing).

- [ ] **Step 3: Implement `settings.rs`, `event.rs`, then `envelope.rs`**

`settings.rs`:

```rust
//! Resolved settings as delivered to a widget (init / SettingsChanged).
//! A sorted Vec of pairs, not a map — postcard-friendly and deterministic.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SettingValue {
    Str(String),
    Num(f64),
    Bool(bool),
    Location { name: String, lat: f64, lon: f64 },
}

pub type Settings = Vec<(String, SettingValue)>;
```

`event.rs`:

```rust
//! Host → widget events and widget → host commands.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    Init,
    Timer(String),
    MenuOpened { page: String },
    MenuClosed,
    SliceVisible,
    SliceHidden,
    Hover { entering: bool },
    Click,
    Scroll { delta: f32 },
    SettingsChanged,
    HttpResponse { id: String, status: u16, body: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HostCmd {
    SetTimer { id: String, secs: u64 },
    CancelTimer { id: String },
    HttpGet { id: String, url: String },
    OpenUrl(String),
    Exec(String),
    HapticPulse(String),
    Log { level: u8, msg: String },
}
```

`envelope.rs`:

```rust
//! Tagged transport envelope. Unknown tags are skipped, never an error —
//! this is the boundary's forward-compatibility mechanism (postcard enum
//! indices alone would hard-fail on unknown variants).

use crate::event::{Event, HostCmd};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub tag: u32,
    pub payload: Vec<u8>,
}

// Append-only tag spaces. Events: 0x0001_xxxx, commands: 0x0002_xxxx.
const TAG_EVENT: u32 = 0x0001_0000;
const TAG_CMD: u32 = 0x0002_0000;

pub fn encode_event(ev: &Event) -> postcard::Result<Vec<u8>> {
    let payload = postcard::to_allocvec(ev)?;
    postcard::to_allocvec(&Envelope { tag: TAG_EVENT, payload })
}

/// `Ok(None)` = unknown tag (skip); `Err` = corrupt bytes.
pub fn decode_event(bytes: &[u8]) -> postcard::Result<Option<Event>> {
    let env: Envelope = postcard::from_bytes(bytes)?;
    if env.tag != TAG_EVENT {
        return Ok(None);
    }
    Ok(Some(postcard::from_bytes(&env.payload)?))
}

pub fn encode_cmd(cmd: &HostCmd) -> postcard::Result<Vec<u8>> {
    let payload = postcard::to_allocvec(cmd)?;
    postcard::to_allocvec(&Envelope { tag: TAG_CMD, payload })
}

pub fn decode_cmd(bytes: &[u8]) -> postcard::Result<Option<HostCmd>> {
    let env: Envelope = postcard::from_bytes(bytes)?;
    if env.tag != TAG_CMD {
        return Ok(None);
    }
    Ok(Some(postcard::from_bytes(&env.payload)?))
}
```

(Design note for the engineer: a single tag per direction is enough because
the *enum* can grow at the tail within an api_version — old hosts that can't
decode a new trailing variant get a postcard `Err`, which the host already
treats as "drop this message, count nothing"; the distinct-tag mechanism is
reserved for genuinely new message *channels* a future api_version adds. The
unknown-tag tests pin the skip behavior.)

- [ ] **Step 4: Run the tests and make sure they pass**

Run: `cargo test -p oxidemx-widget-proto`
Expected: all pass (scene + envelope, 6 tests).

- [ ] **Step 5: Commit**

```bash
git add oxidemx-widget-proto/src
git commit -m "feat(widget-proto): Event/HostCmd + skip-unknown envelope + SettingValue"
```

---

### Task 4: `widget.json` manifest types + validation

**Files:**
- Create: `oxidemx-widget-proto/src/manifest.rs` (replace stub)
- Modify: `oxidemx-widget-proto/src/lib.rs` (uncomment re-exports)

- [ ] **Step 1: Write the failing tests** (bottom of `manifest.rs`; the JSON is the spec §5 weather example verbatim)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const WEATHER: &str = r#"{
      "id": "weather",
      "name": "Weather",
      "version": "1.4.0",
      "author": "JuhLabs",
      "api_version": 1,
      "entry": "widget.wasm",
      "icon": "icon.svg",
      "permissions": ["net:api.open-meteo.com", "open-url", "haptics"],
      "slice": { "refresh_ms": 900000, "fallback_icon": "weather-clear-symbolic" },
      "options": [
        { "key": "location", "type": "location", "label": "Location",
          "hint": "city name or \"lat,lon\"", "required": true },
        { "key": "units", "type": "enum", "label": "Units",
          "values": ["c", "f"], "default": "c" },
        { "key": "refresh", "type": "select", "label": "Refresh",
          "values": [300, 900, 1800, 3600], "default": 900, "unit": "s" }
      ]
    }"#;

    #[test]
    fn parses_weather_manifest() {
        let m: WidgetManifest = serde_json::from_str(WEATHER).unwrap();
        assert_eq!(m.id, "weather");
        assert_eq!(m.api_version, 1);
        assert_eq!(m.slice.refresh_ms, 900_000);
        assert_eq!(m.options.len(), 3);
        assert_eq!(m.options[0].kind, "location");
        assert!(m.options[0].required);
        assert_eq!(m.options[1].default, Some(serde_json::json!("c")));
        assert_eq!(m.options[2].unit.as_deref(), Some("s"));
        assert!(m.validate().is_ok());
    }

    #[test]
    fn minimal_manifest_gets_defaults() {
        let m: WidgetManifest = serde_json::from_str(
            r#"{ "id": "clock", "name": "Clock", "version": "0.1.0",
                 "author": "x", "api_version": 1,
                 "entry": "widget.wasm", "icon": "icon.svg" }"#,
        ).unwrap();
        assert!(m.permissions.is_empty());
        assert_eq!(m.slice.refresh_ms, 900_000);
        assert!(m.options.is_empty());
        assert!(m.validate().is_ok());
    }

    #[test]
    fn refresh_floor_is_clamped() {
        let m: WidgetManifest = serde_json::from_str(
            r#"{ "id": "spam", "name": "Spam", "version": "0.1.0",
                 "author": "x", "api_version": 1,
                 "entry": "widget.wasm", "icon": "icon.svg",
                 "slice": { "refresh_ms": 50 } }"#,
        ).unwrap();
        assert_eq!(m.effective_refresh_ms(), MIN_REFRESH_MS);
    }

    #[test]
    fn bad_ids_are_rejected() {
        for bad in ["", "Has Spaces", "UPPER", "emoji🙂", "a/b"] {
            let mut m: WidgetManifest = serde_json::from_str(
                r#"{ "id": "ok", "name": "X", "version": "0.1.0",
                     "author": "x", "api_version": 1,
                     "entry": "widget.wasm", "icon": "icon.svg" }"#,
            ).unwrap();
            m.id = bad.into();
            assert!(m.validate().is_err(), "id {bad:?} should be rejected");
        }
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p oxidemx-widget-proto manifest`
Expected: compile error.

- [ ] **Step 3: Implement the manifest types**

```rust
//! `widget.json` — the single source of truth for the picker tile, the
//! options card, and permissions (spec §5).

use serde::{Deserialize, Serialize};

/// Refresh floor (spec §8): timers and refresh_ms clamp to this.
pub const MIN_REFRESH_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    pub api_version: u32,
    pub entry: String,
    pub icon: String,
    /// "net:<host>", "open-url", "exec", "haptics".
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub slice: SliceMeta,
    #[serde(default)]
    pub options: Vec<OptionSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SliceMeta {
    #[serde(default = "default_refresh_ms")]
    pub refresh_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_icon: Option<String>,
}

impl Default for SliceMeta {
    fn default() -> Self {
        SliceMeta { refresh_ms: default_refresh_ms(), fallback_icon: None }
    }
}

fn default_refresh_ms() -> u64 { 900_000 }

/// One entry of `options[]`. `kind` stays a String so unknown types from
/// newer widgets render as a disabled "Update OxideMX" row instead of
/// failing the parse (spec §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionSpec {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<serde_json::Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxlen: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
}

impl WidgetManifest {
    /// Structural validation — id slug, non-empty entry/icon, option keys.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || !self.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        {
            return Err(format!("invalid widget id {:?} (allowed: [a-z0-9.-]+)", self.id));
        }
        if self.entry.is_empty() {
            return Err("manifest `entry` must name the wasm module".into());
        }
        if self.icon.is_empty() {
            return Err("manifest `icon` must name an icon file".into());
        }
        for o in &self.options {
            if o.key.is_empty() {
                return Err("option with empty key".into());
            }
        }
        Ok(())
    }

    /// `slice.refresh_ms` with the spec floor applied.
    pub fn effective_refresh_ms(&self) -> u64 {
        self.slice.refresh_ms.max(MIN_REFRESH_MS)
    }

    /// Manifest defaults as a JSON bag — layer 1 of settings resolution.
    pub fn defaults(&self) -> serde_json::Map<String, serde_json::Value> {
        self.options.iter()
            .filter_map(|o| o.default.clone().map(|d| (o.key.clone(), d)))
            .collect()
    }
}
```

- [ ] **Step 4: Run the tests and make sure they pass**

Run: `cargo test -p oxidemx-widget-proto`
Expected: all pass. Also run `cargo check -p oxidemx-widget-proto --no-default-features` — the guest build must compile without serde_json.

- [ ] **Step 5: Commit**

```bash
git add oxidemx-widget-proto/src
git commit -m "feat(widget-proto): widget.json manifest schema + validation + defaults bag"
```

---

### Task 5: `WidgetSource::Custom` + `WidgetScope` in oxidemx-shared

**Files:**
- Modify: `oxidemx-shared/src/config.rs:89-110` (WidgetConfig / WidgetSource)
- Modify: `overlay-rs/src/render/slices/widgets.rs:50` (Copy fallout)
- Modify: `settings-rs/src/tabs/buttons.rs:1504-1510` area (Copy fallout)
- Possibly: `overlay-rs/src/radial/painter.rs` (Copy fallout — fix as compiler directs)

- [ ] **Step 1: Write the failing tests** (append inside the existing `#[cfg(test)] mod tests` in `oxidemx-shared/src/config.rs`)

```rust
    #[test]
    fn custom_widget_slice_round_trips() {
        let json = r#"{
            "label": "Weather",
            "type": "widget",
            "widget": { "source": { "custom": "weather" },
                        "scope": "global",
                        "instance_key": "apps.slot4" },
            "color": "yellow"
        }"#;
        let s: Slice = serde_json::from_str(json).unwrap();
        let w = s.widget.as_ref().unwrap();
        assert_eq!(w.source, WidgetSource::Custom("weather".into()));
        assert_eq!(w.scope, WidgetScope::Global);
        assert_eq!(w.instance_key.as_deref(), Some("apps.slot4"));
        let back = serde_json::to_string(&s).unwrap();
        let s2: Slice = serde_json::from_str(&back).unwrap();
        assert_eq!(s.widget, s2.widget);
    }

    #[test]
    fn legacy_builtin_widget_slice_still_parses() {
        // Pre-v3 shape: no scope / instance_key fields.
        let json = r#"{
            "label": "CPU",
            "type": "widget",
            "widget": { "source": "cpu" }
        }"#;
        let s: Slice = serde_json::from_str(json).unwrap();
        let w = s.widget.as_ref().unwrap();
        assert_eq!(w.source, WidgetSource::Cpu);
        assert_eq!(w.scope, WidgetScope::Instance); // default
        assert_eq!(w.instance_key, None);
    }
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p oxidemx-shared custom_widget`
Expected: compile error (`WidgetScope`, `Custom` missing).

- [ ] **Step 3: Implement the schema change**

Replace `WidgetConfig` and `WidgetSource` in `oxidemx-shared/src/config.rs`:

```rust
/// Data binding for a live widget wedge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetConfig {
    pub source: WidgetSource,
    /// Optional format override for the big value (e.g. "{}%").
    /// `None` = the source's default formatting. Built-ins only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Where the options card writes (spec §6). Reads always merge
    /// defaults ← global ← instance regardless.
    #[serde(default)]
    pub scope: WidgetScope,
    /// Key into `AppConfig::widgets.instances` — `<page-slug>.slot<N>`,
    /// assigned by the settings editor when a custom widget is placed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_key: Option<String>,
}

/// Which level of the two-bag widget settings store an options card
/// writes to. Resolution always reads through both (spec §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetScope {
    #[default]
    Instance,
    Global,
}

/// Which live data feed a widget wedge renders.
///
/// NOTE: no longer `Copy` — `Custom` carries the installed widget id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetSource {
    Weather,
    Cpu,
    Memory,
    Network,
    Disk,
    TasksDue,
    MouseBattery,
    /// Installed plugin widget, by manifest id (e.g. "weather2").
    Custom(String),
}
```

(`{"custom": "weather"}` is the natural serde external tagging for a newtype
variant under `rename_all = "snake_case"` — the round-trip test pins it.)

- [ ] **Step 4: Fix the Copy fallout until the workspace compiles**

Run: `cargo check --workspace 2>&1 | head -50` and fix every error. Known sites:

- `overlay-rs/src/render/slices/widgets.rs:50`:
  `let source = slice.widget.as_ref().map(|w| w.source);`
  → `let source = slice.widget.as_ref().map(|w| w.source.clone());`
  Any `match source { Some(WidgetSource::…) }` arms stay valid; add a
  `Some(WidgetSource::Custom(_)) => {}` (or `_ => {}` if one already exists)
  arm where the compiler demands exhaustiveness — Plan 2 replaces it with
  real scene replay.
- `settings-rs/src/tabs/buttons.rs:1504-1510` (`WidgetSourceOption` array):
  if it's a `const`/`static` array of `WidgetSource` values it still
  compiles (all listed variants are field-less); if the compiler complains
  about `Copy` in surrounding code, replace implicit copies with `.clone()`.
- `overlay-rs/src/radial/painter.rs`: same treatment — `.clone()` where a
  `WidgetSource` was being copied, `Custom(_)` arm where matches must be
  exhaustive (render nothing for now).

Run: `cargo test -p oxidemx-shared && cargo check --workspace`
Expected: tests pass, workspace compiles.

- [ ] **Step 5: Commit**

```bash
git add oxidemx-shared/src/config.rs overlay-rs/src settings-rs/src
git commit -m "feat(shared): WidgetSource::Custom + WidgetScope + instance_key on WidgetConfig"
```

---

### Task 6: `WidgetStore` + `schema_version` + settings resolution

**Files:**
- Create: `oxidemx-shared/src/widgets.rs`
- Modify: `oxidemx-shared/src/lib.rs` (add `pub mod widgets;` + re-exports, next to the existing module list)
- Modify: `oxidemx-shared/src/config.rs:821-870` (AppConfig fields)

- [ ] **Step 1: Write the failing tests** (bottom of new `oxidemx-shared/src/widgets.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WidgetScope;
    use serde_json::json;

    fn bag(pairs: &[(&str, serde_json::Value)]) -> JsonBag {
        pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn resolution_merges_defaults_global_instance() {
        let defaults = bag(&[("units", json!("c")), ("refresh", json!(900))]);
        let mut store = WidgetStore::default();
        store.global.insert("weather".into(), bag(&[("units", json!("f"))]));
        store.instances.insert(
            "apps.slot4".into(),
            [( "weather".to_string(), bag(&[("refresh", json!(300))]) )].into_iter().collect(),
        );

        // instance scope: all three layers
        let r = store.resolve("weather", Some("apps.slot4"), WidgetScope::Instance, &defaults);
        assert_eq!(r.get("units"), Some(&json!("f")));    // global beat default
        assert_eq!(r.get("refresh"), Some(&json!(300)));  // instance beat global

        // global scope: instance layer skipped
        let r = store.resolve("weather", Some("apps.slot4"), WidgetScope::Global, &defaults);
        assert_eq!(r.get("refresh"), Some(&json!(900)));  // default survives
    }

    #[test]
    fn seed_instance_copies_resolved_values() {
        let defaults = bag(&[("units", json!("c"))]);
        let mut store = WidgetStore::default();
        store.global.insert("weather".into(), bag(&[("units", json!("f"))]));
        store.seed_instance("weather", "apps.slot4", &defaults);
        assert_eq!(
            store.instances["apps.slot4"]["weather"].get("units"),
            Some(&json!("f"))
        );
    }

    #[test]
    fn rekey_moves_the_whole_instance_bag() {
        let mut store = WidgetStore::default();
        store.instances.insert(
            "apps.slot4".into(),
            [("weather".to_string(), bag(&[("units", json!("f"))]))].into_iter().collect(),
        );
        store.rekey_instance("apps.slot4", "apps.slot2");
        assert!(!store.instances.contains_key("apps.slot4"));
        assert_eq!(store.instances["apps.slot2"]["weather"].get("units"), Some(&json!("f")));
    }

    #[test]
    fn instance_key_format() {
        assert_eq!(instance_key("Apps", 4), "apps.slot4");
        assert_eq!(instance_key("My Dev Page!", 0), "my-dev-page.slot0");
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p oxidemx-shared widgets`
Expected: compile error.

- [ ] **Step 3: Implement `widgets.rs`**

```rust
//! Two-bag widget settings store (spec §6/§7): `global` holds one bag per
//! widget id shared by every instance; `instances` holds partial override
//! bags keyed by `<page-slug>.slot<N>`. Reads always merge
//! defaults ← global ← instance; `scope` only selects the write target.

use crate::config::WidgetScope;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One settings bag — JSON object semantics, deterministic order.
pub type JsonBag = serde_json::Map<String, serde_json::Value>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WidgetStore {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub global: BTreeMap<String, JsonBag>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub instances: BTreeMap<String, BTreeMap<String, JsonBag>>,
}

impl WidgetStore {
    pub fn is_empty(&self) -> bool {
        self.global.is_empty() && self.instances.is_empty()
    }

    /// Effective settings for one widget instance (spec §6).
    pub fn resolve(
        &self,
        widget_id: &str,
        instance_key: Option<&str>,
        scope: WidgetScope,
        defaults: &JsonBag,
    ) -> JsonBag {
        let mut out = defaults.clone();
        if let Some(g) = self.global.get(widget_id) {
            for (k, v) in g {
                out.insert(k.clone(), v.clone());
            }
        }
        if scope == WidgetScope::Instance {
            if let Some(i) = instance_key
                .and_then(|key| self.instances.get(key))
                .and_then(|bags| bags.get(widget_id))
            {
                for (k, v) in i {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        out
    }

    /// Global→slice toggle: seed the instance bag as a copy of the current
    /// resolved values so it diverges from there (spec §6 table).
    pub fn seed_instance(&mut self, widget_id: &str, instance_key: &str, defaults: &JsonBag) {
        let resolved = self.resolve(widget_id, Some(instance_key), WidgetScope::Global, defaults);
        self.instances
            .entry(instance_key.to_string())
            .or_default()
            .insert(widget_id.to_string(), resolved);
    }

    /// Slice moved/swapped/page renamed — carry its override bag along.
    pub fn rekey_instance(&mut self, old: &str, new: &str) {
        if let Some(bags) = self.instances.remove(old) {
            self.instances.insert(new.to_string(), bags);
        }
    }
}

/// `<page-slug>.slot<N>` — human-readable instance key (spec §7).
pub fn instance_key(page_name: &str, slot: usize) -> String {
    format!("{}.slot{slot}", slugify(page_name))
}

fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true; // suppress leading dash
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "page".into() } else { out }
}
```

In `oxidemx-shared/src/lib.rs`, alongside the existing modules:

```rust
pub mod widgets;
```

(Match the file's existing re-export style — if other modules get `pub use`
lines, add `pub use widgets::{instance_key, JsonBag, WidgetStore};`.)

- [ ] **Step 4: Add the AppConfig fields**

In `AppConfig` (config.rs, after the `overlay` field at line ~869):

```rust
    /// Config schema version. Missing = 2 (pre-widget-store configs).
    /// Bumped to 3 by the one-shot migration in `migrate.rs`.
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    /// Two-bag widget settings store (global + per-instance). See
    /// `crate::widgets`.
    #[serde(default, skip_serializing_if = "WidgetStore::is_empty")]
    pub widgets: crate::widgets::WidgetStore,
```

And near the other free functions in config.rs:

```rust
fn default_schema_version() -> u32 { 2 }

/// Schema version written by this build of the tools.
pub const CURRENT_SCHEMA_VERSION: u32 = 3;
```

Add `use crate::widgets::WidgetStore;` to config.rs imports. Note
`AppConfig` derives `Default` via the struct's `#[derive(... Default)]` if
present — if it instead has a manual `Default` impl, add
`schema_version: default_schema_version(), widgets: WidgetStore::default(),`
there (check the file; `cargo check` will direct you).

Run: `cargo test -p oxidemx-shared && cargo check --workspace`
Expected: pass / compile.

- [ ] **Step 5: Commit**

```bash
git add oxidemx-shared/src
git commit -m "feat(shared): WidgetStore two-bag settings + resolution + schema_version field"
```

---

### Task 7: v2 → v3 migration

**Files:**
- Create: `oxidemx-shared/src/migrate.rs`
- Modify: `oxidemx-shared/src/lib.rs` (add `pub mod migrate;`)
- Modify: `oxidemx-shared/src/config.rs:980-992` (`load_from` applies the in-memory lift)

- [ ] **Step 1: Write the failing tests** (bottom of `migrate.rs`; uses a unique temp dir, no new deps)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "oxidemx-migrate-{}-{}", name, std::process::id()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d.join("config.json")
    }

    const V2: &str = r#"{
        "theme": "dracula",
        "overlay": {
            "weather_location": [59.91, 10.75],
            "weather_place": "Oslo, NO",
            "weather_celsius": true
        }
    }"#;

    #[test]
    fn in_memory_migration_lifts_weather_settings() {
        let mut cfg: AppConfig = serde_json::from_str(V2).unwrap();
        assert_eq!(cfg.schema_version, 2);
        assert!(migrate_to_v3(&mut cfg));
        assert_eq!(cfg.schema_version, 3);
        let bag = &cfg.widgets.global["weather"];
        assert_eq!(bag["location"]["lat"], serde_json::json!(59.91));
        assert_eq!(bag["location"]["name"], serde_json::json!("Oslo, NO"));
        assert_eq!(bag["units"], serde_json::json!("c"));
        // idempotent
        assert!(!migrate_to_v3(&mut cfg));
    }

    #[test]
    fn migration_does_not_clobber_existing_bags() {
        let mut cfg: AppConfig = serde_json::from_str(V2).unwrap();
        cfg.widgets.global.insert("weather".into(), {
            let mut b = crate::widgets::JsonBag::new();
            b.insert("units".into(), serde_json::json!("f"));
            b
        });
        migrate_to_v3(&mut cfg);
        // user's existing value wins over the lifted one
        assert_eq!(cfg.widgets.global["weather"]["units"], serde_json::json!("f"));
    }

    #[test]
    fn file_migration_writes_bak_and_rewrites() {
        let path = tmp("file");
        std::fs::write(&path, V2).unwrap();
        assert!(migrate_file_to_v3(&path).unwrap());
        let bak = path.with_extension("json.v2.bak");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), V2);
        let migrated: AppConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(migrated.schema_version, 3);
        // second run is a no-op
        assert!(!migrate_file_to_v3(&path).unwrap());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn load_from_serves_v3_view_without_touching_disk() {
        let path = tmp("load");
        std::fs::write(&path, V2).unwrap();
        let cfg = AppConfig::load_from(&path).unwrap();
        assert_eq!(cfg.schema_version, 3);
        assert!(cfg.widgets.global.contains_key("weather"));
        // file untouched
        assert_eq!(std::fs::read_to_string(&path).unwrap(), V2);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
```

- [ ] **Step 2: Run them to make sure they fail**

Run: `cargo test -p oxidemx-shared migrate`
Expected: compile error.

- [ ] **Step 3: Implement `migrate.rs`**

```rust
//! One-shot config schema migration v2 → v3 (spec §7): widget settings
//! move into the dedicated `widgets` store. The canonical case is the
//! weather widget's old global overlay fields.

use crate::config::{AppConfig, ConfigError, CURRENT_SCHEMA_VERSION};
use std::path::Path;

/// In-memory lift. Returns `true` if anything changed. Never removes the
/// legacy overlay fields (kept for one release; readers prefer the store).
pub fn migrate_to_v3(cfg: &mut AppConfig) -> bool {
    if cfg.schema_version >= CURRENT_SCHEMA_VERSION {
        return false;
    }
    if let Some((lat, lon)) = cfg.overlay.weather_location {
        let bag = cfg.widgets.global.entry("weather".to_string()).or_default();
        bag.entry("location".to_string()).or_insert_with(|| {
            serde_json::json!({
                "name": cfg.overlay.weather_place.clone().unwrap_or_default(),
                "lat": lat,
                "lon": lon,
            })
        });
        bag.entry("units".to_string()).or_insert_with(|| {
            serde_json::json!(if cfg.overlay.weather_celsius { "c" } else { "f" })
        });
    }
    cfg.schema_version = CURRENT_SCHEMA_VERSION;
    true
}

/// On-disk migration: writes `config.json.v2.bak` with the original bytes,
/// then atomically rewrites the file as v3. Idempotent. Call this from the
/// settings app at startup, before its first save.
pub fn migrate_file_to_v3(path: &Path) -> Result<bool, ConfigError> {
    let original = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(ConfigError::Io(e)),
    };
    let mut cfg: AppConfig = serde_json::from_str(&original).map_err(ConfigError::Parse)?;
    if !migrate_to_v3(&mut cfg) {
        return Ok(false);
    }
    let bak = path.with_extension("json.v2.bak");
    std::fs::write(&bak, &original).map_err(ConfigError::Io)?;
    let json = serde_json::to_string_pretty(&cfg).map_err(ConfigError::Parse)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(ConfigError::Io)?;
    std::fs::rename(&tmp, path).map_err(ConfigError::Io)?;
    Ok(true)
}
```

Add `pub mod migrate;` to `oxidemx-shared/src/lib.rs`.

- [ ] **Step 4: Apply the in-memory lift in `load_from` and verify**

In `AppConfig::load_from` (config.rs ~line 990), after
`cfg.radial_menu.normalize_pages();` add:

```rust
        crate::migrate::migrate_to_v3(&mut cfg);
```

(`ConfigError` and `CURRENT_SCHEMA_VERSION` must be `pub` — they already
are / were made so in Task 6.)

Run: `cargo test -p oxidemx-shared && cargo check --workspace`
Expected: all tests pass (including the pre-existing config tests — if
`parses_minimal_config` style tests assert on serialized output, the new
`schema_version` field will appear; adjust those asserts only if they break,
keeping their intent).

- [ ] **Step 5: Commit**

```bash
git add oxidemx-shared/src
git commit -m "feat(shared): v2→v3 config migration — weather lift, .v2.bak, in-memory view"
```

---

### Task 8: Workspace hygiene pass

**Files:**
- Modify: whatever clippy flags

- [ ] **Step 1: Run the full test suite**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: everything green (pre-existing failures unrelated to these crates
should be noted, not fixed here).

- [ ] **Step 2: Clippy the touched crates**

Run: `cargo clippy -p oxidemx-widget-proto -p oxidemx-shared -- -D warnings`
Fix anything it raises (typical: needless clones in tests, `or_default`).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "chore: clippy sweep for widget-proto + shared schema changes"
```

(Skip the commit if there was nothing to fix.)

---

## Self-review notes

- **Spec coverage (Plan 1 scope = spec §15 steps 1–2):** proto types §8
  (Scene/Event/HostCmd/envelope/SettingValue) ✓ Tasks 2–3; manifest +
  option types §5 ✓ Task 4; config schema §7 (Custom source, scope,
  instance_key, WidgetStore, schema_version, resolution, seeding, rekey,
  migration incl. `.v2.bak` + weather lift) ✓ Tasks 5–7. Signature
  verification, wasmi runtime, HTTP cache, PDK → Plan 2. All UI → Plan 3.
- **Type consistency:** `JsonBag` = `serde_json::Map<String, Value>`
  everywhere; `WidgetScope` lives in config.rs (widgets.rs imports it);
  `instance_key` helper produces the same `apps.slot4` shape the Task 5
  test fixture uses; `CURRENT_SCHEMA_VERSION` defined Task 6, used Task 7.
- **Known judgment calls baked in:** missing `schema_version` defaults to 2
  so the migration triggers exactly once; legacy weather fields are kept
  (readers prefer the store) to avoid breaking an older overlay reading a
  migrated file.
