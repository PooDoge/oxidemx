//! Live-preview widget for the editor: a non-layer-shell instance of
//! the radial widget that re-renders on every config change. Lets the
//! user see the menu update as they tweak labels, colours, and icons
//! without opening the real overlay.
//!
//! Driven from the same `RadialWidget` rendering code as the production
//! overlay, just embedded in a regular GtkBox instead of a layer-shell
//! window.

// TODO: instantiate RadialWidget, hook it to the editor's "current
// config" reactive store, and disable the input controllers (this is
// preview, not interactive).
