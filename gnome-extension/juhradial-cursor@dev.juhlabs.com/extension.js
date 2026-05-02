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
 *
 * SPDX-License-Identifier: GPL-3.0
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

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
 * Resolve an `idx` into a Meta.Rectangle. `idx = -1` → primary.
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
                const [appId] = params.deep_unpack();
                const win = findWindowByAppId(appId);
                if (win) {
                    win.raise();
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
        }
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
            const probe = new Meta.Rectangle({
                x: absX, y: absY, width: 1, height: 1,
            });
            const idx = global.display.get_monitor_index_for_rect(probe);
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
