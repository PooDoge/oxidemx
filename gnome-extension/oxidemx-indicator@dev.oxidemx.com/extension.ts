/**
 * OxideMX Indicator — GNOME Shell extension entry point.
 *
 * Composes a PanelMenu.Button that shows the connected MX device's battery
 * level in the top bar (and/or Dash to Panel). Dispatches left-click events
 * according to the click-behavior GSettings key, provides a right-click
 * context menu, and surfaces the OxideMX stack-health state via tooltip
 * and icon styling.
 *
 * Lifecycle:
 *   enable()  — builds the button, wires all subscribers, adds to panel,
 *               starts BatteryClient + Supervisor.
 *   disable() — tears everything down cleanly; all GObject signals are
 *               disconnected and every field is reset to null so the GC
 *               can collect the extension's heap.
 *
 * SPDX-License-Identifier: GPL-3.0
 */

import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import * as BoxPointer from 'resource:///org/gnome/shell/ui/boxpointer.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

import { IndicatorSettings } from './lib/settings.js';
import { BatteryClient, CriticalNotifier, DeviceState } from './lib/battery.js';
import { Supervisor, StackHealth } from './lib/supervisor.js';
import { IndicatorPlacement } from './lib/placement.js';
import { bandFor, colorForBand, formatPctLabel, clampPct } from './lib/format.js';
import { OxideMXPopup } from './lib/popup.js';

// ---------------------------------------------------------------------------
// Types for GJS / Mutter objects that @girs typings do not fully model.
// ---------------------------------------------------------------------------

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
    set_skip_taskbar?(skip: boolean): void;
    hide_from_window_list?(): void;
}

interface MetaWindowActor {
    get_meta_window?(): MetaWindow | null;
}

interface MonitorGeometry {
    x: number;
    y: number;
    width: number;
    height: number;
}

interface MetaDisplay {
    get_n_monitors(): number;
    get_monitor_geometry(index: number): MonitorGeometry;
    get_primary_monitor(): number;
    get_focus_window?(): MetaWindow | null;
}

interface ShellGlobal {
    readonly display: MetaDisplay | null;
    get_pointer(): [number, number];
    get_current_time(): number;
    get_window_actors(): MetaWindowActor[];
}

declare const global: ShellGlobal;

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
    return display.get_primary_monitor();
}

function windowMatchesAppId(win: MetaWindow, appId: string): boolean {
    let alternativeNames: string[] = [];
    if (appId === "org.oxidemx.overlay") {
        alternativeNames = ["oxidemx-overlay", "oxidemx-overlay"];
    } else if (appId === "org.oxidemx.popup") {
        alternativeNames = ["oxidemx-popup", "oxidemx-popup"];
    }
    const candidates: Array<string | null | undefined> = [
        win.get_gtk_application_id?.(),
        win.get_wm_class?.(),
        win.get_wm_class_instance?.(),
        win.get_sandboxed_app_id?.(),
    ];
    for (const c of candidates) {
        if (c && (c === appId || alternativeNames.includes(c))) {
            return true;
        }
    }
    return false;
}

function findWindowByAppId(appId: string): MetaWindow | null {
    const actors: MetaWindowActor[] = global.get_window_actors();
    for (const actor of actors) {
        const win: MetaWindow | null | undefined = actor.get_meta_window?.();
        if (!win) continue;
        if (windowMatchesAppId(win, appId)) {
            return win;
        }
    }
    return null;
}

function monitorGeometry(idx: number): MonitorGeometry | null {
    const display: MetaDisplay | null = global.display;
    if (!display) return null;
    const count: number = display.get_n_monitors();
    let resolved: number = idx;
    if (resolved < 0) resolved = display.get_primary_monitor();
    if (resolved < 0 || resolved >= count) return null;
    return display.get_monitor_geometry(resolved);
}

const DBUS_IFACE: string = `
<node>
  <interface name="org.oxidemx.CursorHelper">
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
      <arg type="a(iiiii)" direction="out" name="monitors"/>
    </method>
    <method name="GetFocusedWindowClass">
      <arg type="s" direction="in" name="ignore_app_id"/>
      <arg type="s" direction="out" name="class"/>
    </method>
  </interface>
</node>`;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const ROLE = 'oxidemx-indicator';

// ---------------------------------------------------------------------------
// PanelButton — GObject.registerClass + PanelMenu.Button subclass
//
// The return type of GObject.registerClass() carries additional GObject class
// identity metadata that TypeScript cannot express through the normal class
// hierarchy.  The field `_button` in the extension class is therefore typed
// `any` to avoid inference failures when accessing the registered-class
// instance. All PanelMenu.Button surface is invoked via `as any` casts with
// JSDoc comments explaining the contract at each call site.
// ---------------------------------------------------------------------------

const PanelButton = GObject.registerClass(
    class OxideMXIndicatorButton extends PanelMenu.Button {
        private _box!: St.BoxLayout;
        private _mouseIcon!: St.Icon;
        private _batteryIcon!: St.Icon;
        private _label!: St.Label;

        /** Most-recently applied settings snapshot — used by applySettings(). */
        private _lastSettings!: IndicatorSettings | null;

        _init(): void {
            // menuAlignment=0.5 centres the popup menu under the button.
            super._init(0.5, 'OxideMX Indicator', false);
            this._lastSettings = null;

            this._box = new St.BoxLayout({
                style_class: 'panel-status-indicators-box',
                y_align: Clutter.ActorAlign.CENTER,
            });
            this.add_child(this._box);

            // Mouse-device glyph (symbolic icon from the system theme or our icons/).
            this._mouseIcon = new St.Icon({
                icon_name: 'input-mouse-symbolic',
                style_class: 'system-status-icon',
            });

            // Battery state icon.
            this._batteryIcon = new St.Icon({
                icon_name: 'battery-good-symbolic',
                style_class: 'system-status-icon',
            });

            // Percentage label.
            this._label = new St.Label({
                text: '--',
                y_align: Clutter.ActorAlign.CENTER,
                style_class: 'oxidemx-indicator-label',
            });

            this._box.add_child(this._mouseIcon);
            this._box.add_child(this._batteryIcon);
            this._box.add_child(this._label);
        }

        /**
         * Rebuild visible children according to current settings.
         * Called on `settings.onChange` to avoid a full re-enable cycle.
         */
        applySettings(settings: IndicatorSettings): void {
            this._lastSettings = settings;

            const mode = settings.displayMode();
            this._mouseIcon.visible = settings.showMouseGlyph() && mode !== 'none';
            this._batteryIcon.visible = mode !== 'percent' && mode !== 'none';
            this._label.visible = mode !== 'icon' && mode !== 'none';
        }

        /**
         * Update the button's label, icon name, and colour based on the
         * latest DeviceState.  When `state` is null (daemon not yet
         * connected), show placeholder "--" text with no tinting.
         */
        setState(state: DeviceState | null): void {
            if (!state) {
                this._label.set_text('--');
                this._label.set_style(null);
                this._batteryIcon.icon_name = 'battery-missing-symbolic';
                return;
            }

            const pct = clampPct(state.battery);
            const charging = state.charging;

            // Update label text.
            this._label.set_text(formatPctLabel(pct, charging));

            // Update battery icon: approximate symbolic icon name from pct.
            const iconName = this._batteryIconName(pct, charging);
            this._batteryIcon.icon_name = iconName;

            // Apply band colour when settings allow it.
            if (this._lastSettings) {
                const band = bandFor(pct, charging, this._lastSettings.thresholds());
                const color = colorForBand(band, this._lastSettings.bandColors());
                if (this._lastSettings.applyColorToText()) {
                    this._label.set_style(`color: ${color};`);
                } else {
                    this._label.set_style(null);
                }
                if (this._lastSettings.tintMouseGlyph()) {
                    this._mouseIcon.set_style(`color: ${color};`);
                } else {
                    this._mouseIcon.set_style(null);
                }
            }
        }

        /**
         * Update the button's accessible tooltip to reflect stack health.
         * When the daemon is not running the tooltip hints at the problem.
         */
        setHealth(h: StackHealth): void {
            let tooltip: string;
            if (!h.daemonRunning) {
                tooltip = 'OxideMX daemon not running';
            } else if (!h.deviceLinked) {
                tooltip = 'No MX device connected';
            } else {
                tooltip = h.deviceName ? `${h.deviceName}` : 'OxideMX';
            }
            // St.Widget.set_accessible_name is the standard tooltip surface for
            // panel buttons in GNOME Shell extensions.
            this.set_accessible_name(tooltip);
        }

        // ---- private helpers ----

        private _batteryIconName(pct: number, charging: boolean): string {
            if (charging) return 'battery-good-charging-symbolic';
            if (pct <= 10) return 'battery-empty-symbolic';
            if (pct <= 25) return 'battery-caution-symbolic';
            if (pct <= 50) return 'battery-low-symbolic';
            if (pct <= 75) return 'battery-good-symbolic';
            return 'battery-full-symbolic';
        }
    },
);

// ---------------------------------------------------------------------------
// Extension class
// ---------------------------------------------------------------------------

export default class OxideMXIndicatorExtension extends Extension {
    private _settings: IndicatorSettings | null = null;
    private _placement: IndicatorPlacement | null = null;
    private _battery: BatteryClient | null = null;
    private _notifier: CriticalNotifier | null = null;
    private _supervisor: Supervisor | null = null;
    private _cancellable: Gio.Cancellable | null = null;
    private _unsubs: Array<() => void> = [];

    // Cursor Helper D-Bus registration state
    private _dbusId: number | null = null;
    private _registrationId: number | null = null;
    private _connection: Gio.DBusConnection | null = null;

    // window-demands-attention / window-marked-urgent listeners —
    // the focus-stealing-prevention safety net for the overlay's AI
    // chat window (the "stealmyfocus" pattern, scoped to our window).
    private _attentionSignalIds: number[] = [];

    override enable(): void {
        this._cancellable = new Gio.Cancellable();

        // Register the org.oxidemx.CursorHelper D-Bus interface.
        const nodeInfo: Gio.DBusNodeInfo = Gio.DBusNodeInfo.new_for_xml(DBUS_IFACE);
        this._dbusId = Gio.bus_own_name(
            Gio.BusType.SESSION,
            'org.oxidemx.CursorHelper',
            Gio.BusNameOwnerFlags.NONE,
            (connection: Gio.DBusConnection) => {
                this._connection = connection;
                this._registrationId = connection.register_object(
                    '/org/oxidemx/CursorHelper',
                    nodeInfo.interfaces[0],
                    (
                        _conn: Gio.DBusConnection,
                        _sender: string,
                        _path: string,
                        _iface: string,
                        method: string,
                        params: GLib.Variant,
                        invocation: Gio.DBusMethodInvocation,
                    ): void => {
                        try {
                            this._dispatchCursorHelper(method, params, invocation);
                        } catch (e) {
                            log(`[oxidemx-indicator] ${method} failed: ${e}`);
                            invocation.return_error_literal(
                                // @ts-expect-error -- Gio.DBusError is the registered error domain quark at GJS runtime.
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
        // this.getSettings() returns Gio.Settings from the gnome-shell package's nested
        // gio-2.0 transitive dep, which is structurally identical but a different TS
        // declaration path from our top-level gi://Gio. Cast via `any` at this boundary.
        this._settings = new IndicatorSettings(this.getSettings() as any);

        // Instantiate the BatteryClient and supporting services.
        this._battery = new BatteryClient(
            this._settings.refreshInterval(),
            this._cancellable,
        );
        this._notifier = new CriticalNotifier(this._settings);
        this._supervisor = new Supervisor(
            this._settings,
            this._battery,
            this._cancellable,
        );

        // Instantiate the IndicatorPlacement manager.
        this._placement = new IndicatorPlacement(
            ROLE,
            () => new (PanelButton as any)(),
            (indicator) => this._configureIndicator(indicator),
            () => {
                return {
                    panelTarget: this._settings!.panelTarget(),
                    displayMode: this._settings!.displayMode(),
                    position: this._settings!.position(),
                    positionIndex: this._settings!.positionIndex(),
                };
            }
        );

        // Place the indicators initially.
        this._placement.place(
            this._settings.panelTarget(),
            this._settings.displayMode(),
            this._settings.position(),
            this._settings.positionIndex()
        );

        // Subscribe: battery state → button display + critical notifier.
        this._unsubs.push(
            this._battery.onStateChange((state: DeviceState) => {
                for (const ind of this._placement?.indicators ?? []) {
                    ind.setState(state);
                }
                this._notifier!.observe(state);
            }),
        );

        // Subscribe: health → button health display.
        this._unsubs.push(
            this._supervisor.onHealthChange((h: StackHealth) => {
                for (const ind of this._placement?.indicators ?? []) {
                    ind.setHealth(h);
                    this._rebuildContextMenu(ind);
                }
            }),
        );

        // Subscribe: settings change → reapply display properties.
        this._unsubs.push(
            this._settings.onChange((key: string) => {
                if (this._settings && this._placement) {
                    if (['panel-target', 'display-mode', 'position', 'position-index'].includes(key)) {
                        this._placement.place(
                            this._settings.panelTarget(),
                            this._settings.displayMode(),
                            this._settings.position(),
                            this._settings.positionIndex()
                        );
                        // Make sure we apply current state/health to the newly placed buttons
                        const state = this._battery?.latest() ?? null;
                        const health = this._supervisor?.latestHealth() ?? null;
                        for (const ind of this._placement.indicators) {
                            ind.setState(state);
                            if (health) {
                                ind.setHealth(health);
                                this._rebuildContextMenu(ind);
                            }
                        }
                    } else {
                        for (const ind of this._placement.indicators) {
                            ind.applySettings(this._settings);
                        }
                    }
                }
            }),
        );

        // Start data services — they begin emitting state asynchronously.
        this._battery.start();
        this._supervisor.start();

        // Safety net: if Mutter ever demotes an overlay activation to
        // "window is ready" (demands-attention) instead of focusing,
        // re-activate immediately — but ONLY for our own overlay
        // window, so normal apps keep standard focus-stealing
        // prevention.
        const display: any = (global as any).display;
        if (display?.connect) {
            for (const signal of ['window-demands-attention', 'window-marked-urgent']) {
                this._attentionSignalIds.push(
                    display.connect(signal, (_d: unknown, win: any) => {
                        if (win && windowMatchesAppId(win, 'org.oxidemx.overlay')) {
                            log(`[oxidemx-indicator] overlay ${signal} — re-activating`);
                            Main.activateWindow(win);
                        }
                    }),
                );
            }
        }
    }

    override disable(): void {
        // Tear down subscriptions.
        for (const unsub of this._unsubs) {
            try { unsub(); } catch { /* ignore */ }
        }
        this._unsubs = [];

        // Disconnect the demands-attention safety net.
        const display: any = (global as any).display;
        for (const id of this._attentionSignalIds) {
            try { display?.disconnect?.(id); } catch { /* ignore */ }
        }
        this._attentionSignalIds = [];

        // Stop data services.
        this._battery?.stop();
        this._supervisor?.stop();

        // Destroy the placement manager and all indicators.
        if (this._placement !== null) {
            this._placement.destroy();
            this._placement = null;
        }

        // Cancel any in-flight async work.
        this._cancellable?.cancel();

        // Null all fields so GC can collect.
        this._settings = null;
        this._battery = null;
        this._notifier = null;
        this._supervisor = null;
        this._cancellable = null;

        // Unregister D-Bus interface.
        if (this._connection !== null && this._registrationId !== null) {
            try {
                this._connection.unregister_object(this._registrationId);
            } catch (e) {
                log(`[oxidemx-indicator] failed to unregister cursor helper: ${e}`);
            }
            this._registrationId = null;
            this._connection = null;
        }
        if (this._dbusId !== null) {
            Gio.bus_unown_name(this._dbusId);
            this._dbusId = null;
        }
    }

    // ---- click dispatch ----

    private _configureIndicator(indicator: any): void {
        if (!this._settings || !this._battery || !this._supervisor) return;

        indicator.applySettings(this._settings);
        indicator.setState(this._battery.latest() ?? null);

        const health = this._supervisor.latestHealth();
        if (health) {
            indicator.setHealth(health);
        }

        // Initialize the custom settings popup and add it to the main menu.
        // Pass the whole BatteryClient — its D-Bus proxy is created
        // asynchronously and would still be null here.
        const popup = new OxideMXPopup(this._battery, this._settings);
        indicator.menu.addMenuItem(popup);
        indicator._juhPopup = popup;

        // Create secondary context menu for right-clicks
        const contextMenu = new PopupMenu.PopupMenu(indicator, 0.5, St.Side.TOP);
        indicator._contextMenu = contextMenu;
        this._rebuildContextMenu(indicator);

        // Bind events
        indicator.connect('captured-event', (_actor: object, event: Clutter.Event) => {
            return this._onIndicatorCapturedEvent(indicator, event);
        });

        // On open: close a lingering context menu and re-query the daemon
        // so the popup always shows live state (covers keyboard/other open
        // paths, not just the left-click dispatch below).
        indicator.menu.connect('open-state-changed', (_menu: object, open: boolean) => {
            if (open && indicator._contextMenu.isOpen) {
                indicator._contextMenu.close(BoxPointer.PopupAnimation.FADE);
            }
            if (open && indicator._juhPopup) {
                indicator._juhPopup.refreshState();
            }
        });

        // Destroy context menu when indicator is destroyed
        indicator.connect('destroy', () => {
            if (indicator._contextMenu) {
                indicator._contextMenu.destroy();
            }
        });
    }

    private _onIndicatorCapturedEvent(indicator: any, event: Clutter.Event): boolean {
        if (event.type() !== Clutter.EventType.BUTTON_PRESS) {
            return Clutter.EVENT_PROPAGATE;
        }

        const button = event.get_button();

        if (button === Clutter.BUTTON_SECONDARY) {
            // Right-click: toggle the secondary context menu!
            indicator.menu.close(BoxPointer.PopupAnimation.FADE);
            if (indicator._contextMenu.isOpen) {
                indicator._contextMenu.close(BoxPointer.PopupAnimation.FADE);
            } else {
                this._rebuildContextMenu(indicator);
                indicator._contextMenu.open(BoxPointer.PopupAnimation.FULL);
            }
            return Clutter.EVENT_STOP;
        }

        if (button !== Clutter.BUTTON_PRIMARY) {
            return Clutter.EVENT_PROPAGATE;
        }

        // Left-click
        if (!this._settings) return Clutter.EVENT_STOP;

        const health = this._supervisor?.latestHealth();

        // Fallback auto-fix check: if the daemon is down, open the context menu instead.
        if (!health || !health.daemonRunning) {
            this._rebuildContextMenu(indicator);
            indicator._contextMenu.open(BoxPointer.PopupAnimation.FULL);
            return Clutter.EVENT_STOP;
        }

        const behavior = this._settings.clickBehavior();

        switch (behavior) {
            case 'popup':
                // Let the primary click propagate to open indicator.menu;
                // the open-state-changed handler refreshes the popup state.
                return Clutter.EVENT_PROPAGATE;

            case 'settings':
                this._spawnSettings();
                return Clutter.EVENT_STOP;

            case 'none':
            default:
                return Clutter.EVENT_STOP;
        }
    }

    private _spawnSettings(): void {
        try {
            Gio.Subprocess.new(
                ['oxidemx-settings'],
                Gio.SubprocessFlags.NONE,
            );
        } catch (e) {
            log(`[oxidemx-indicator] spawn oxidemx-settings failed: ${e}`);
        }
    }

    // ---- right-click context menu ----

    private _rebuildContextMenu(indicator: any): void {
        const menu = indicator._contextMenu;
        if (!menu) return;

        menu.removeAll();

        const settingsItem = new PopupMenu.PopupMenuItem('Open Settings');
        settingsItem.connect('activate', () => this._spawnSettings());
        menu.addMenuItem(settingsItem);

        const prefsItem = new PopupMenu.PopupMenuItem('Open Extension Preferences');
        prefsItem.connect('activate', () => this.openPreferences());
        menu.addMenuItem(prefsItem);

        const health = this._supervisor?.latestHealth();
        if (health && !health.daemonRunning) {
            menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            const startItem = new PopupMenu.PopupMenuItem('Start Daemon (Auto-fix)');
            startItem.connect('activate', () => {
                this._supervisor?.startDaemon().catch((e: unknown) => {
                    logError(e as object, '[oxidemx-indicator] startDaemon failed');
                });
            });
            menu.addMenuItem(startItem);
        }

        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        const aboutItem = new PopupMenu.PopupMenuItem('OxideMX Indicator v0.3.2', {
            reactive: false,
        });
        menu.addMenuItem(aboutItem);
    }

    private _dispatchCursorHelper(
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
                const [appId]: [string] = params.deep_unpack() as [string];
                const win: MetaWindow | null = findWindowByAppId(appId);
                if (win) {
                    // Inside an idle D-Bus handler there's no current
                    // event, so get_current_time() can return 0 — the
                    // roundtrip variant asks the server for a real
                    // timestamp, which matters for focus transfer.
                    const display: any = global.display;
                    const ts: number = global.get_current_time() ||
                        (display?.get_current_time_roundtrip?.() ?? 0);
                    if (win.minimized) {
                        win.unminimize();
                    }
                    if (typeof win.set_skip_taskbar === 'function') {
                        win.set_skip_taskbar(true);
                    }
                    if (typeof win.hide_from_window_list === 'function') {
                        win.hide_from_window_list();
                    }
                    // Main.activateWindow handles raise + unminimize +
                    // cross-workspace focus + hiding the overview;
                    // extension-initiated activation runs as a trusted
                    // PAGER source so focus-stealing prevention doesn't
                    // demote it.
                    Main.activateWindow(win as any, ts);
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
                invocation.return_value(new GLib.Variant('(a(iiiii))', [out]));
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

        const actors: MetaWindowActor[] = global.get_window_actors();
        for (let i = actors.length - 1; i >= 0; i--) {
            const win: MetaWindow | null | undefined = actors[i].get_meta_window?.();
            if (!win) continue;
            if (isIgnored(win)) continue;
            const type: number | undefined = win.get_window_type?.();
            if (type !== Meta.WindowType.NORMAL && type !== Meta.WindowType.DIALOG) {
                continue;
            }
            const cls: string | null = tryClass(win);
            if (cls) return cls;
        }
        return null;
    }

    private _moveOverlay(
        appId: string,
        x: number,
        y: number,
        monitor: number,
    ): boolean {
        const win: MetaWindow | null = findWindowByAppId(appId);
        if (!win) return false;

        if (typeof win.set_skip_taskbar === 'function') {
            win.set_skip_taskbar(true);
        }
        if (typeof win.hide_from_window_list === 'function') {
            win.hide_from_window_list();
        }

        let absX: number = x;
        let absY: number = y;
        if (monitor >= 0) {
            const g: MonitorGeometry | null = monitorGeometry(monitor);
            if (g) {
                absX = g.x + x;
                absY = g.y + y;
            }
        }

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

        win.move_frame(false, absX, absY);
        win.raise();
        return true;
    }
}
