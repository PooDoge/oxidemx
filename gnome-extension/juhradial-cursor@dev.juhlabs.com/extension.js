/**
 * JuhRadial Cursor Helper - GNOME Shell Extension
 *
 * Two services for the JuhRadial MX overlay on GNOME Wayland:
 *
 *   1) Cursor query — exposes `global.get_pointer()` over D-Bus so
 *      the daemon and overlay can read the cursor position without
 *      xdotool / Shell.Eval (neither works on stable Wayland).
 *
 *   2) Window positioning — Mutter does NOT advertise
 *      `wlr-layer-shell`, so a regular Wayland app can't place its
 *      own toplevel at an arbitrary screen point. The overlay is
 *      now built with iced + winit (xdg-shell) and asks this
 *      extension to position its window after presentation by
 *      calling `MoveOverlay(app_id, x, y, monitor_index)`. Mutter
 *      lets *extensions* (which run inside the compositor) call
 *      `Meta.Window.move_frame()` directly, bypassing the
 *      protocol-level restriction.
 *
 * D-Bus interface:
 *   Name: org.juhradial.CursorHelper
 *   Path: /org/juhradial/CursorHelper
 *   Methods:
 *     GetCursorPosition() -> (i x, i y)            # Mutter-stage logical pixels
 *     MoveOverlay(s app_id, i x, i y, i monitor)   # absolute logical px,
 *                                                  # monitor=-1 → primary
 *                                                  # returns: b success
 *     RaiseOverlay(s app_id) -> b success
 *     ListMonitors() -> a(iiii)                    # [(idx, x, y, w, h), ...]
 *     GetFocusedWindowClass(s ignore_app_id) -> s  # WM_CLASS / app_id of
 *                                                  # currently focused window;
 *                                                  # empty string when none.
 *                                                  # Skips windows matching
 *                                                  # `ignore_app_id` so the
 *                                                  # overlay's own focus
 *                                                  # doesn't poison the
 *                                                  # result.
 *
 * SPDX-License-Identifier: GPL-3.0
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

// `display.get_monitor_index_for_rect()` requires a real
// MetaRectangle / MtkRectangle GObject — plain JS objects fail
// with "not a subclass of GObject_Struct", and `Meta.Rectangle`
// was removed in GNOME 50. Rather than depend on whichever module
// happens to expose the constructor in this Mutter version, we
// just walk the monitor list ourselves: the geometry returned by
// `display.get_monitor_geometry()` is a plain struct we *can*
// read (no constructor needed for output structs), so a simple
// containment loop replaces the Mutter helper. Works on every
// supported GNOME version (45+).
function monitorIndexForPoint(x, y) {
    const display = global.display;
    if (!display) return -1;
    const n = display.get_n_monitors();
    for (let i = 0; i < n; i++) {
        const g = display.get_monitor_geometry(i);
        if (x >= g.x && x < g.x + g.width &&
            y >= g.y && y < g.y + g.height) {
            return i;
        }
    }
    return display.get_primary_monitor();  // fallback when point is off-screen
}

const DBUS_IFACE = `
<node>
  <interface name="org.juhradial.CursorHelper">
    <method name="GetCursorPosition">
      <arg type="i" direction="out" name="x"/>
      <arg type="i" direction="out" name="y"/>
    </method>
    <method name="MoveOverlay">
      <arg type="s" direction="in" name="app_id"/>
      <arg type="i" direction="in" name="x"/>
      <arg type="i" direction="in" name="y"/>
      <arg type="i" direction="in" name="monitor"/>
      <arg type="b" direction="out" name="success"/>
    </method>
    <method name="RaiseOverlay">
      <arg type="s" direction="in" name="app_id"/>
      <arg type="b" direction="out" name="success"/>
    </method>
    <method name="ListMonitors">
      <arg type="a(iiii)" direction="out" name="monitors"/>
    </method>
    <method name="GetFocusedWindowClass">
      <arg type="s" direction="in" name="ignore_app_id"/>
      <arg type="s" direction="out" name="class"/>
    </method>
  </interface>
</node>`;

/**
 * Find the topmost MetaWindow whose WM_CLASS / Gtk-Application-Id /
 * sandboxed-app-id matches `appId`. We try every identifier Mutter
 * exposes because winit's xdg-shell window doesn't necessarily set
 * gtk_application_id (it's a winit wl_surface, not a GApplication).
 *
 * Returns the MetaWindow or null.
 */
function findWindowByAppId(appId) {
    const actors = global.get_window_actors();
    for (const actor of actors) {
        const win = actor.get_meta_window();
        if (!win) continue;
        const candidates = [
            win.get_gtk_application_id?.(),
            win.get_wm_class?.(),
            win.get_wm_class_instance?.(),
            win.get_sandboxed_app_id?.(),
        ];
        for (const c of candidates) {
            if (c && c === appId) return win;
        }
    }
    return null;
}

/**
 * Resolve an `idx` into a rectangle ({x, y, width, height}).
 * `idx = -1` → primary monitor.
 * Returns null when the index is out of range.
 */
function monitorGeometry(idx) {
    const display = global.display;
    if (!display) return null;
    const count = display.get_n_monitors();
    let resolved = idx;
    if (resolved < 0) resolved = display.get_primary_monitor();
    if (resolved < 0 || resolved >= count) return null;
    return display.get_monitor_geometry(resolved);
}

export default class JuhRadialCursorExtension {
    _dbusId = null;
    _registrationId = null;

    enable() {
        const nodeInfo = Gio.DBusNodeInfo.new_for_xml(DBUS_IFACE);

        this._dbusId = Gio.bus_own_name(
            Gio.BusType.SESSION,
            'org.juhradial.CursorHelper',
            Gio.BusNameOwnerFlags.NONE,
            (connection) => {
                this._registrationId = connection.register_object(
                    '/org/juhradial/CursorHelper',
                    nodeInfo.interfaces[0],
                    (connection, _sender, _path, _iface, method, params, invocation) => {
                        try {
                            this._dispatch(method, params, invocation);
                        } catch (e) {
                            log(`[juhradial-cursor] ${method} failed: ${e}`);
                            invocation.return_error_literal(
                                Gio.DBusError, Gio.DBusError.FAILED, String(e),
                            );
                        }
                    },
                    null,
                    null,
                );
            },
            null,
            null,
        );
    }

    _dispatch(method, params, invocation) {
        switch (method) {
            case 'GetCursorPosition': {
                const [x, y] = global.get_pointer();
                invocation.return_value(new GLib.Variant('(ii)', [x, y]));
                return;
            }
            case 'MoveOverlay': {
                const [appId, x, y, monitor] = params.deep_unpack();
                const success = this._moveOverlay(appId, x, y, monitor);
                invocation.return_value(new GLib.Variant('(b)', [success]));
                return;
            }
            case 'RaiseOverlay': {
                // Raise + activate (un-minimise + switch workspace + focus).
                // We must use win.activate(timestamp) — Wayland's focus-
                // stealing prevention blocks app-side focus requests, but
                // the Shell process is privileged and `activate` is the
                // canonical Mutter API for "bring this window forward and
                // give it keyboard focus". Used by every legitimate
                // launcher / dock / app switcher.
                const [appId] = params.deep_unpack();
                const win = findWindowByAppId(appId);
                if (win) {
                    const ts = global.get_current_time();
                    if (win.minimized) {
                        win.unminimize();
                    }
                    win.raise();
                    win.activate(ts);
                    invocation.return_value(new GLib.Variant('(b)', [true]));
                } else {
                    invocation.return_value(new GLib.Variant('(b)', [false]));
                }
                return;
            }
            case 'ListMonitors': {
                const display = global.display;
                const out = [];
                if (display) {
                    const n = display.get_n_monitors();
                    for (let i = 0; i < n; i++) {
                        const g = display.get_monitor_geometry(i);
                        out.push([i, g.x, g.y, g.width, g.height]);
                    }
                }
                invocation.return_value(new GLib.Variant('(a(iiii))', [out]));
                return;
            }
            case 'GetFocusedWindowClass': {
                const [ignoreAppId] = params.deep_unpack();
                const cls = this._focusedWindowClass(ignoreAppId);
                invocation.return_value(new GLib.Variant('(s)', [cls || '']));
                return;
            }
        }
    }

    /**
     * Resolve the currently-focused MetaWindow into a usable
     * application class string (WM_CLASS preferred, then GTK app id,
     * then sandbox app id). The caller passes its own app_id in
     * `ignoreAppId`; if focus happens to be on that window we walk
     * the window-actor list in stacking order to find the next
     * focused-eligible window underneath. Returns null when no
     * suitable window is focused.
     */
    _focusedWindowClass(ignoreAppId) {
        const display = global.display;
        if (!display) return null;
        const focus = display.get_focus_window?.();
        const tryClass = (win) => {
            if (!win) return null;
            const candidates = [
                win.get_wm_class?.(),
                win.get_gtk_application_id?.(),
                win.get_sandboxed_app_id?.(),
                win.get_wm_class_instance?.(),
            ];
            for (const c of candidates) {
                if (c) return String(c);
            }
            return null;
        };
        const isIgnored = (win) => {
            if (!win || !ignoreAppId) return false;
            const candidates = [
                win.get_gtk_application_id?.(),
                win.get_wm_class?.(),
                win.get_wm_class_instance?.(),
                win.get_sandboxed_app_id?.(),
            ];
            return candidates.some((c) => c && c === ignoreAppId);
        };
        if (focus && !isIgnored(focus)) {
            const cls = tryClass(focus);
            if (cls) return cls;
        }
        // Either no focus_window (rare) or focus is on the overlay
        // itself (toggle mode). Walk window actors top-down for the
        // first eligible app window.
        const actors = global.get_window_actors();
        for (let i = actors.length - 1; i >= 0; i--) {
            const win = actors[i].get_meta_window?.();
            if (!win) continue;
            if (isIgnored(win)) continue;
            // Skip non-app surfaces (DESKTOP, DOCK, etc.) — only
            // NORMAL / DIALOG windows make sense as a "currently
            // active app".
            const type = win.get_window_type?.();
            if (type !== Meta.WindowType.NORMAL && type !== Meta.WindowType.DIALOG) {
                continue;
            }
            const cls = tryClass(win);
            if (cls) return cls;
        }
        return null;
    }

    /**
     * Position the overlay's xdg-toplevel at (x, y) in stage logical
     * pixels. If `monitor` is non-negative the coordinates are
     * interpreted as monitor-local and clamped to that monitor's
     * geometry; otherwise (x, y) are treated as absolute stage
     * coords.
     *
     * Returns true if the move was attempted, false if the window
     * couldn't be found.
     */
    _moveOverlay(appId, x, y, monitor) {
        const win = findWindowByAppId(appId);
        if (!win) return false;

        let absX = x;
        let absY = y;
        if (monitor >= 0) {
            const g = monitorGeometry(monitor);
            if (g) {
                absX = g.x + x;
                absY = g.y + y;
            }
        }

        // Pick the monitor for clamping based on the *requested*
        // position, not the window's current frame. Without this,
        // `get_monitor_index_for_rect(win.get_frame_rect())` always
        // returns whichever monitor Mutter put the window on at
        // startup, so subsequent moves to a different monitor get
        // clamped right back to primary.
        const frame = win.get_frame_rect();
        let targetMon = null;
        if (monitor >= 0) {
            targetMon = monitorGeometry(monitor);
        } else {
            const idx = monitorIndexForPoint(absX, absY);
            targetMon = monitorGeometry(idx);
        }
        if (targetMon) {
            const maxX = targetMon.x + targetMon.width  - frame.width;
            const maxY = targetMon.y + targetMon.height - frame.height;
            absX = Math.max(targetMon.x, Math.min(absX, maxX));
            absY = Math.max(targetMon.y, Math.min(absY, maxY));
        }

        // user_op = false because this isn't an interactive drag —
        // suppresses Mutter's "save the position so on next launch
        // we restore it" heuristic which would pin the menu in
        // place across activations.
        win.move_frame(false, absX, absY);
        win.raise();
        return true;
    }

    disable() {
        if (this._dbusId) {
            Gio.bus_unown_name(this._dbusId);
            this._dbusId = null;
        }
        this._registrationId = null;
    }
}
