# OxideMX MX — GNOME Shell extensions

Two extensions live here:

- **`oxidemx-cursor@dev.juhlabs.com/`** — cursor + window-positioning helper for the radial overlay and popup. Exposes `org.oxidemx.CursorHelper`.
- **`oxidemx-indicator@dev.juhlabs.com/`** — top-bar battery indicator + libadwaita prefs + stack-supervisor surface for the MX device.

Both extensions are written in **TypeScript**. GNOME Shell loads `.js`, so `.ts` is compiled to `.js` alongside each source file. The `.js` files are gitignored — `.ts` is the source of truth.

## Building

The build assumes Node.js is available. On atomic Fedora (Bazzite) install Node inside a distrobox; do **not** layer it via rpm-ostree.

```bash
# inside the distrobox (one-time):
cd gnome-extension
npm install

# every time .ts files change:
cd gnome-extension
npm run build
# or
npm run watch
```

Or use the project-level helper:

```bash
./dev-install-ext.sh   # compiles TS, installs both extensions, restarts GNOME Shell (X11) or prompts for re-login (Wayland)
```

## Editor setup

VS Code with the built-in TypeScript language server. Open the workspace at the *repo root* — the extensions inherit type information from `gnome-extension/tsconfig.json`.

## Type checking without producing .js

```bash
cd gnome-extension
npm run check
```

Useful for CI gates.
