# oxide-app

Freya desktop app for OxideMX — standalone Cargo workspace (3 crates).

## Crates

| Crate | Purpose |
|---|---|
| `oxide-client` | Transport layer: `Transport` trait + `UdsTransport` over agentd UDS |
| `oxide-ui` | Reusable Freya component library (design tokens, animated wrappers) |
| `oxide-freya` | App entry point: window shell, navigation regions, page composition |

## Build Recipe (Atomic Fedora / Bazzite host)

Freya pulls in `skia-bindings` via `freya-engine`. The Skia binaries download
automatically via network; no manual download step is needed. The compile step
is slow on first run (5–15 min). Subsequent incremental builds are fast.

**The host lacks the unversioned `.so` linker stubs** (those are in `-devel` RPMs,
which are unavailable on an atomic-Fedora host without distrobox). Work around
this by creating a local symlink dir and pointing `LIBRARY_PATH` at it:

```bash
mkdir -p /tmp/oxidemx-lib-links
ln -sf /usr/lib64/libEGL.so.1        /tmp/oxidemx-lib-links/libEGL.so
ln -sf /usr/lib64/libGL.so.1         /tmp/oxidemx-lib-links/libGL.so
ln -sf /usr/lib64/libGLESv2.so.2     /tmp/oxidemx-lib-links/libGLESv2.so
ln -sf /usr/lib64/libfreetype.so.6   /tmp/oxidemx-lib-links/libfreetype.so
ln -sf /usr/lib64/libfontconfig.so.1 /tmp/oxidemx-lib-links/libfontconfig.so
ln -sf /usr/lib64/libwayland-egl.so.1 /tmp/oxidemx-lib-links/libwayland-egl.so
```

### Build

```bash
cd oxide-app
LIBRARY_PATH=/tmp/oxidemx-lib-links cargo build --bin oxide-freya
```

### Run

```bash
cd oxide-app
LIBRARY_PATH=/tmp/oxidemx-lib-links cargo run --bin oxide-freya
```

A 1200×800 window titled "OxideMX" with a dark background and centered
"OxideMX — hello Freya" label appears.

### Test (headless — no display needed)

```bash
cd oxide-app
LIBRARY_PATH=/tmp/oxidemx-lib-links cargo test --package oxide-freya
```

## Workspace isolation

This workspace has its own `target/` and its own `Cargo.lock`. It does NOT
share the phase1 workspace target dir or `/tmp/oxidemx-host-target`. Verify:

```bash
cd oxide-app
cargo metadata --no-deps --format-version 1 | python3 -c \
  "import sys,json; [print(p['name']) for p in json.load(sys.stdin)['packages']]"
# expected: oxide-client  oxide-ui  oxide-freya
```

## Freya path dependency

Freya v0.4.0-rc.23 is consumed via path deps pointing at the local clone:
`/run/media/system/fastdrive/repos/freya`. See `oxide-app/API-NOTES.md` for
the verified API surface.
