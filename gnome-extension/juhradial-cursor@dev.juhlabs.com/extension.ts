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
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

// ---------------------------------------------------------------------------
// Types for GJS / Mutter objects that @girs typings do not fully model.
// We use minimal structural interfaces rather than `any` so that callers
// stay type-safe without pulling in broader ambient declarations.
// ---------------------------------------------------------------------------

/** Minimal surface of a MetaWindow we actually use. */
interface MetaWindow {
    get_gtk_application_id?(): string | null | undefined;
    get_wm_class?(): string | null | undefined;
    get_wm_class_instance?(): string | null | undefined;
    get_sandboxed_app_id?(): string | null | undefined;
    get_window_type?(): number;
    get_frame_rect(): { x: number; y: number; width: number; height: number };
    move_frame(user_op: boolean, x: number, y: number): void;
    raise(): void;
    unminimize(): void;
    activate(timestamp: number): void;
    readonly minimized: boolean;
}

/** Minimal surface of a MetaWindowActor we use. */
interface MetaWindowActor {
    get_meta_window?(): MetaWindow | null;
}

/** The rect struct returned by display.get_monitor_geometry(). */
interface MonitorGeometry {
    x: number;
    y: number;
    width: number;
    height: number;
}

/** Subset of the Mutter/GJS global display object. */
interface MetaDisplay {
    get_n_monitors(): number;
    get_monitor_geometry(index: number): MonitorGeometry;
    get_primary_monitor(): number;
    get_focus_window?(): MetaWindow | null;
}

/** GJS global object subset (Shell.Global). */
interface ShellGlobal {
    readonly display: MetaDisplay | null;
    get_pointer(): [number, number];
    get_current_time(): number;
    get_window_actors(): MetaWindowActor[];
}

// GJS injects `global` as a module-level ambient — cast it to our typed interface.
declare const global: ShellGlobal;

// ---------------------------------------------------------------------------
// Module-level helpers — stateless, no extension instance needed.
// ---------------------------------------------------------------------------

/**
 * Return the monitor index for a logical-pixel point (x, y).
 *
 * `display.get_monitor_index_for_rect()` requires a real
 * MetaRectangle / MtkRectangle GObject — plain JS objects fail
 * with "not a subclass of GObject_Struct", and `Meta.Rectangle`
 * was removed in GNOME 50. Rather than depend on whichever module
 * happens to expose the constructor in this Mutter version, we
 * just walk the monitor list ourselves: the geometry returned by
 * `display.get_monitor_geometry()` is a plain struct we *can*
 * read (no constructor needed for output structs), so a simple
 * containment loop replaces the Mutter helper. Works on every
 * supported GNOME version (45+).
 */
function monitorIndexForPoint(x: number, y: number): number {
    const display: MetaDisplay | null = global.display;
    if (!display) return -1;
    const n: number = display.get_n_monitors();
    for (let i = 0; i < n; i++) {
        const g: MonitorGeometry = display.get_monitor_geometry(i);
        if (x >= g.x && x < g.x + g.width &&
            y >= g.y && y < g.y + g.height) {
            return i;
        }
    }
    return display.get_primary_monitor();  // fallback when point is off-screen
}

const DBUS_IFACE: string = `
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
function findWindowByAppId(appId: string): MetaWindow | null {
    const actors: MetaWindowActor[] = global.get_window_actors();
    for (const actor of actors) {
        const win: MetaWindow | null | undefined = actor.get_meta_window?.();
        if (!win) continue;
        const candidates: Array<string | null | undefined> = [
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
function monitorGeometry(idx: number): MonitorGeometry | null {
    const display: MetaDisplay | null = global.display;
    if (!display) return null;
    const count: number = display.get_n_monitors();
    let resolved: number = idx;
    if (resolved < 0) resolved = display.get_primary_monitor();
    if (resolved < 0 || resolved >= count) return null;
    return display.get_monitor_geometry(resolved);
}

// ---------------------------------------------------------------------------
// Extension class
// ---------------------------------------------------------------------------

export default class JuhRadialCursorExtension extends Extension {
    private _dbusId: number | null = null;
    private _registrationId: number | null = null;

    override enable(): void {
        const nodeInfo: Gio.DBusNodeInfo = Gio.DBusNodeInfo.new_for_xml(DBUS_IFACE);

        this._dbusId = Gio.bus_own_name(
            Gio.BusType.SESSION,
            'org.juhradial.CursorHelper',
            Gio.BusNameOwnerFlags.NONE,
            (connection: Gio.DBusConnection) => {
                this._registrationId = connection.register_object(
                    '/org/juhradial/CursorHelper',
                    nodeInfo.interfaces[0],
                    (
                        _connection: Gio.DBusConnection,
                        _sender: string,
                        _path: string,
                        _iface: string,
                        method: string,
                        params: GLib.Variant,
                        invocation: Gio.DBusMethodInvocation,
                    ): void => {
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

    private _dispatch(
        method: string,
        params: GLib.Variant,
        invocation: Gio.DBusMethodInvocation,
    ): void {
        switch (method) {
            case 'GetCursorPosition': {
                const [x, y]: [number, number] = global.get_pointer();
                invocation.return_value(new GLib.Variant('(ii)', [x, y]));
                return;
            }
            case 'MoveOverlay': {
                const [appId, x, y, monitor]: [string, number, number, number] =
                    params.deep_unpack() as [string, number, number, number];
                const success: boolean = this._moveOverlay(appId, x, y, monitor);
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
                const [appId]: [string] = params.deep_unpack() as [string];
                const win: MetaWindow | null = findWindowByAppId(appId);
                if (win) {
                    const ts: number = global.get_current_time();
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
                const display: MetaDisplay | null = global.display;
                const out: Array<[number, number, number, number, number]> = [];
                if (display) {
                    const n: number = display.get_n_monitors();
                    for (let i = 0; i < n; i++) {
                        const g: MonitorGeometry = display.get_monitor_geometry(i);
                        out.push([i, g.x, g.y, g.width, g.height]);
                    }
                }
                invocation.return_value(new GLib.Variant('(a(iiii))', [out]));
                return;
            }
            case 'GetFocusedWindowClass': {
                const [ignoreAppId]: [string] = params.deep_unpack() as [string];
                const cls: string | null = this._focusedWindowClass(ignoreAppId);
                invocation.return_value(new GLib.Variant('(s)', [cls ?? '']));
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
    private _focusedWindowClass(ignoreAppId: string): string | null {
        const display: MetaDisplay | null = global.display;
        if (!display) return null;
        const focus: MetaWindow | null | undefined = display.get_focus_window?.();

        const tryClass = (win: MetaWindow | null | undefined): string | null => {
            if (!win) return null;
            const candidates: Array<string | null | undefined> = [
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

        const isIgnored = (win: MetaWindow | null | undefined): boolean => {
            if (!win || !ignoreAppId) return false;
            const candidates: Array<string | null | undefined> = [
                win.get_gtk_application_id?.(),
                win.get_wm_class?.(),
                win.get_wm_class_instance?.(),
                win.get_sandboxed_app_id?.(),
            ];
            return candidates.some((c) => c && c === ignoreAppId);
        };

        if (focus && !isIgnored(focus)) {
            const cls: string | null = tryClass(focus);
            if (cls) return cls;
        }

        // Either no focus_window (rare) or focus is on the overlay
        // itself (toggle mode). Walk window actors top-down for the
        // first eligible app window.
        const actors: MetaWindowActor[] = global.get_window_actors();
        for (let i = actors.length - 1; i >= 0; i--) {
            const win: MetaWindow | null | undefined = actors[i].get_meta_window?.();
            if (!win) continue;
            if (isIgnored(win)) continue;
            // Skip non-app surfaces (DESKTOP, DOCK, etc.) — only
            // NORMAL / DIALOG windows make sense as a "currently
            // active app".
            const type: number | undefined = win.get_window_type?.();
            if (type !== Meta.WindowType.NORMAL && type !== Meta.WindowType.DIALOG) {
                continue;
            }
            const cls: string | null = tryClass(win);
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
    private _moveOverlay(
        appId: string,
        x: number,
        y: number,
        monitor: number,
    ): boolean {
        const win: MetaWindow | null = findWindowByAppId(appId);
        if (!win) return false;

        let absX: number = x;
        let absY: number = y;
        if (monitor >= 0) {
            const g: MonitorGeometry | null = monitorGeometry(monitor);
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
        const frame: { x: number; y: number; width: number; height: number } =
            win.get_frame_rect();
        let targetMon: MonitorGeometry | null = null;
        if (monitor >= 0) {
            targetMon = monitorGeometry(monitor);
        } else {
            const idx: number = monitorIndexForPoint(absX, absY);
            targetMon = monitorGeometry(idx);
        }
        if (targetMon) {
            const maxX: number = targetMon.x + targetMon.width  - frame.width;
            const maxY: number = targetMon.y + targetMon.height - frame.height;
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

    override disable(): void {
        if (this._dbusId !== null) {
            Gio.bus_unown_name(this._dbusId);
            this._dbusId = null;
        }
        this._registrationId = null;
    }
}
