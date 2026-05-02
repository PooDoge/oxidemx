//! System tray icon. Provides quick access to the editor window, the
//! existing settings GUI, and "exit overlay" without forcing the user
//! to find the right `pkill` incantation.
//!
//! GTK4 dropped GtkStatusIcon; tray support comes via either
//! AppIndicator (libayatana-appindicator3) or KStatusNotifierItem over
//! D-Bus. The latter is what GNOME's AppIndicator extension speaks
//! anyway, so we should target SNI directly via zbus rather than
//! pulling in a C library.

// TODO: implement KStatusNotifierItem service — register on
//       org.kde.StatusNotifierItem-<pid>-1, expose items: "Edit menu",
//       "Settings…", "About", "Quit". The legacy Python overlay built
//       this on QSystemTrayIcon (overlay/juhradial-overlay.py:1208+);
//       the menu structure should match for muscle-memory parity.
