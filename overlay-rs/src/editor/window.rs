//! Editor main window. A regular GTK4 ApplicationWindow (NOT layer-shell —
//! the editor is a normal app window users can resize, move, and Alt-Tab
//! to). Hosts the per-slice panel, the icon picker, and the live preview
//! widget side-by-side.

// TODO: build the editor window. Approximate layout:
//
//   ┌──────────────────────────────────────────────────────────────┐
//   │ [Slice list] │  [Slice editor]            │  [Live preview]  │
//   │ • Top        │  Label: ____________       │                  │
//   │ • Top-Right  │  Type:  [exec ▾]           │   ╭──────╮       │
//   │ • Right      │  Cmd:   ____________       │   │  ◯   │       │
//   │ • …          │  Color: [██] [picker ▾]    │   ╰──────╯       │
//   │              │  Icon:  [icon picker]      │                  │
//   │ [+ Add]      │  Submenu: [☐ enable]       │  [Open menu now] │
//   └──────────────────────────────────────────────────────────────┘
//
// Save → write config.json → daemon's existing inotify reload picks it
// up → overlay's config watcher refreshes the slice cache.
