/**
 * JuhRadial Indicator — GNOME Shell extension entry point.
 *
 * Composes a PanelMenu.Button that shows the connected MX device's battery
 * level in the top bar (and/or Dash to Panel). Dispatches left-click events
 * according to the click-behavior GSettings key, provides a right-click
 * context menu, and surfaces the JuhRadial stack-health state via tooltip
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

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

import { IndicatorSettings } from './lib/settings.js';
import { BatteryClient, CriticalNotifier, DeviceState } from './lib/battery.js';
import { Supervisor, StackHealth } from './lib/supervisor.js';
import { resolvePanelTarget, addToPanel, removeFromPanel } from './lib/placement.js';
import { bandFor, colorForBand, formatPctLabel, clampPct } from './lib/format.js';

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const ROLE = 'juhradial-indicator';

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
    class JuhRadialIndicatorButton extends PanelMenu.Button {
        private _box!: St.BoxLayout;
        private _mouseIcon!: St.Icon;
        private _batteryIcon!: St.Icon;
        private _label!: St.Label;

        /** Most-recently applied settings snapshot — used by applySettings(). */
        private _lastSettings: IndicatorSettings | null = null;

        _init(): void {
            // menuAlignment=0.5 centres the popup menu under the button.
            super._init(0.5, 'JuhRadial Indicator', false);

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
                style_class: 'juhradial-indicator-label',
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
            this._mouseIcon.visible = settings.showMouseGlyph();
            this._batteryIcon.visible = mode !== 'percent';
            this._label.visible = mode !== 'icon';
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
                tooltip = 'JuhRadial daemon not running';
            } else if (!h.deviceLinked) {
                tooltip = 'No MX device connected';
            } else {
                tooltip = h.deviceName ? `${h.deviceName}` : 'JuhRadial';
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

export default class JuhRadialIndicatorExtension extends Extension {
    private _settings: IndicatorSettings | null = null;
    // GObject.registerClass alters class identity — typed as `any` per spec.
    private _button: any = null;
    private _battery: BatteryClient | null = null;
    private _notifier: CriticalNotifier | null = null;
    private _supervisor: Supervisor | null = null;
    private _cancellable: Gio.Cancellable | null = null;
    private _unsubs: Array<() => void> = [];
    /** Last-known health snapshot; used by click dispatch for the Start Daemon menu item. */
    private _lastHealth: StackHealth | null = null;

    override enable(): void {
        this._cancellable = new Gio.Cancellable();
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

        // Build the panel button.
        // new PanelButton() — the registerClass'd constructor matches _init() above.
        this._button = new (PanelButton as any)();
        this._button.applySettings(this._settings);

        // Wire a right-click handler.
        // PanelMenu.Button.connect is the GObject signal API.
        this._button.connect('button-press-event', (_actor: object, event: Clutter.Event) => {
            return this._onButtonClick(event);
        });

        // Subscribe: battery state → button display + critical notifier.
        this._unsubs.push(
            this._battery.onStateChange((state: DeviceState) => {
                this._button.setState(state);
                this._notifier!.observe(state);
            }),
        );

        // Subscribe: health → button health display + local cache.
        this._unsubs.push(
            this._supervisor.onHealthChange((h: StackHealth) => {
                this._lastHealth = h;
                this._button.setHealth(h);
                this._rebuildHealthMenu(h);
            }),
        );

        // Subscribe: settings change → reapply display properties.
        this._unsubs.push(
            this._settings.onChange((_key: string) => {
                if (this._settings && this._button) {
                    this._button.applySettings(this._settings);
                }
            }),
        );

        // Add button to panel.
        const resolvedTarget = resolvePanelTarget(this._settings.panelTarget());
        addToPanel(
            this._button,
            resolvedTarget,
            this._settings.position(),
            this._settings.positionIndex(),
            ROLE,
        );

        // Start data services — they begin emitting state asynchronously.
        this._battery.start();
        this._supervisor.start();
    }

    override disable(): void {
        // Tear down subscriptions.
        for (const unsub of this._unsubs) {
            try { unsub(); } catch { /* ignore */ }
        }
        this._unsubs = [];

        // Stop data services.
        this._battery?.stop();
        this._supervisor?.stop();

        // Remove and destroy the button.
        if (this._button !== null) {
            removeFromPanel(this._button, ROLE);
            this._button = null;
        }

        // Cancel any in-flight async work.
        this._cancellable?.cancel();

        // Null all fields so GC can collect.
        this._settings = null;
        this._battery = null;
        this._notifier = null;
        this._supervisor = null;
        this._cancellable = null;
        this._lastHealth = null;
    }

    // ---- click dispatch ----

    /**
     * Handle button-press-event on the indicator button.
     *
     * Left click:
     *   popup   → dispatch ShowPopup over D-Bus with the button's panel rect.
     *   settings → spawn juhradial-settings via Gio.Subprocess.
     *   none    → propagate normally.
     *
     * Right click (button 3):
     *   Open the right-click context menu.
     */
    private _onButtonClick(event: Clutter.Event): boolean {
        const button = event.get_button();

        if (button === Clutter.BUTTON_SECONDARY) {
            // Right click — let PanelMenu.Button open its menu normally.
            return Clutter.EVENT_PROPAGATE;
        }

        if (button !== Clutter.BUTTON_PRIMARY) {
            return Clutter.EVENT_PROPAGATE;
        }

        // Left click — dispatch according to click-behavior setting.
        if (!this._settings) return Clutter.EVENT_PROPAGATE;

        const behavior = this._settings.clickBehavior();

        switch (behavior) {
            case 'popup':
                this._dispatchShowPopup();
                return Clutter.EVENT_STOP;

            case 'settings':
                this._spawnSettings();
                return Clutter.EVENT_STOP;

            case 'none':
            default:
                return Clutter.EVENT_PROPAGATE;
        }
    }

    private _dispatchShowPopup(): void {
        if (!this._button) return;

        // get_transformed_extents() returns a Graphene.Rect.
        // We access its fields via the .origin (Point) and .size (Size) structs.
        // The coords are stage-absolute logical pixels per Phase 0 Task 0.2.
        let x = 0, y = 0, w = 0, h = 0;
        try {
            // _button is typed `any` (GObject.registerClass identity); at runtime it is a
            // Clutter.Actor subclass that inherits get_transformed_extents() → Graphene.Rect.
            // The @girs Graphene.Rect struct exposes .origin.x/.y and .size.width/.height
            // as plain number fields — no getter methods needed.
            const rect = this._button.get_transformed_extents();
            x = Math.round(rect.origin.x);
            y = Math.round(rect.origin.y);
            w = Math.round(rect.size.width);
            h = Math.round(rect.size.height);
        } catch (e) {
            log(`[juhradial-indicator] get_transformed_extents failed: ${e}`);
        }

        Gio.DBus.session.call(
            'org.juhradial.Daemon',
            '/org/juhradial/Daemon',
            'org.juhradial.Daemon',
            'ShowPopup',
            new GLib.Variant('(iiii)', [x, y, w, h]),
            null,
            Gio.DBusCallFlags.NONE,
            5000,
            this._cancellable,
            // AsyncReadyCallback: first param is source_object (DBusConnection | null)
            (_conn: Gio.DBusConnection | null, result: Gio.AsyncResult) => {
                try {
                    Gio.DBus.session.call_finish(result);
                } catch (e) {
                    log(`[juhradial-indicator] ShowPopup failed: ${e}`);
                }
            },
        );
    }

    private _spawnSettings(): void {
        try {
            Gio.Subprocess.new(
                ['juhradial-settings'],
                Gio.SubprocessFlags.NONE,
            );
        } catch (e) {
            log(`[juhradial-indicator] spawn juhradial-settings failed: ${e}`);
        }
    }

    // ---- right-click context menu ----

    /**
     * Rebuild the right-click menu with current health state.
     * Called whenever StackHealth changes so the "Start Daemon" item tracks
     * the live daemon status without requiring a toggle.
     */
    private _rebuildHealthMenu(health: StackHealth): void {
        if (!this._button) return;

        const menu: PopupMenu.PopupMenu = this._button.menu;
        if (!menu) return;

        menu.removeAll();

        // "Open Settings" → spawn juhradial-settings.
        const settingsItem = new PopupMenu.PopupMenuItem('Open Settings');
        settingsItem.connect('activate', () => this._spawnSettings());
        menu.addMenuItem(settingsItem);

        // "Open Extension Preferences" → openPreferences() (inherited from Extension).
        const prefsItem = new PopupMenu.PopupMenuItem('Open Extension Preferences');
        prefsItem.connect('activate', () => this.openPreferences());
        menu.addMenuItem(prefsItem);

        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        // "Start Daemon" — only when daemon is not running.
        if (!health.daemonRunning) {
            const startItem = new PopupMenu.PopupMenuItem('Start Daemon');
            startItem.connect('activate', () => {
                this._supervisor?.startDaemon().catch((e: unknown) => {
                    logError(e as object, '[juhradial-indicator] startDaemon failed');
                });
            });
            menu.addMenuItem(startItem);
        }

        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        // "About" label.
        const aboutItem = new PopupMenu.PopupMenuItem('JuhRadial Indicator v0.0.1', {
            reactive: false,
        });
        menu.addMenuItem(aboutItem);
    }
}
