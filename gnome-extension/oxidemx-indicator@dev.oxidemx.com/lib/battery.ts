/**
 * D-Bus client for the OxideMX daemon battery surface.
 *
 * BatteryClient opens a session-bus proxy to org.oxidemx.Daemon,
 * subscribes to the DeviceStateChanged signal for push updates, and
 * runs a poll-fallback timer for resilience when the signal stream stalls.
 *
 * CriticalNotifier watches the incoming state stream and fires a one-shot
 * Gio.Notification when the device first enters the critical-and-not-charging
 * band. It re-arms when the device exits the critical band or starts charging.
 *
 * Mock mode (OXIDEMX_INDICATOR_MOCK=1): BatteryClient skips the D-Bus
 * proxy entirely and emits synthetic state on each timer tick, cycling
 * battery from 100 down to 0 and back, for offline development.
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import { bandFor } from './format.js';
import type { IndicatorSettings } from './settings.js';

// ---------------------------------------------------------------------------
// D-Bus interface XML — only the methods + signal this client consumes.
// ---------------------------------------------------------------------------

const IFACE_XML = `
<node>
  <interface name="org.oxidemx.Daemon">
    <method name="GetActiveDeviceState">
      <arg type="y" direction="out" name="battery"/>
      <arg type="b" direction="out" name="charging"/>
      <arg type="s" direction="out" name="connection"/>
      <arg type="s" direction="out" name="device_name"/>
      <arg type="s" direction="out" name="device_id"/>
    </method>
    <method name="ShowPopup">
      <arg type="i" direction="in" name="panel_x"/>
      <arg type="i" direction="in" name="panel_y"/>
      <arg type="i" direction="in" name="panel_w"/>
      <arg type="i" direction="in" name="panel_h"/>
    </method>
    <method name="EnsureOverlayRunning"/>
    <method name="SetGamingMode">
      <arg type="b" direction="in" name="enabled"/>
    </method>
    <method name="SetHapticsEnabled">
      <arg type="b" direction="in" name="enabled"/>
    </method>
    <method name="SetSmartShift">
      <arg type="b" direction="in" name="enabled"/>
      <arg type="y" direction="in" name="threshold"/>
    </method>
    <method name="SetDpi">
      <arg type="q" direction="in" name="dpi"/>
    </method>
    <method name="SetHost">
      <arg type="y" direction="in" name="host_index"/>
      <arg type="b" direction="out" name="success"/>
    </method>
    <method name="GetDpi">
      <arg type="q" direction="out" name="dpi"/>
    </method>
    <method name="GetSmartShift">
      <arg type="b" direction="out" name="enabled"/>
      <arg type="y" direction="out" name="threshold"/>
    </method>
    <method name="GetEasySwitchInfo">
      <arg type="y" direction="out" name="count"/>
      <arg type="y" direction="out" name="current"/>
    </method>
    <method name="GetHostNames">
      <arg type="as" direction="out" name="names"/>
    </method>
    <method name="GetGamingMode">
      <arg type="b" direction="out" name="enabled"/>
    </method>
    <method name="GetThumbWheelStatus">
      <arg type="b" direction="out" name="supported"/>
      <arg type="b" direction="out" name="invert"/>
    </method>
    <method name="SetThumbWheelInvert">
      <arg type="b" direction="in" name="invert"/>
    </method>
    <method name="GetRadialEnabled">
      <arg type="b" direction="out" name="enabled"/>
    </method>
    <method name="SetRadialEnabled">
      <arg type="b" direction="in" name="enabled"/>
    </method>
    <property name="HapticsEnabled" type="b" access="read"/>
    <property name="GamingModeEnabled" type="b" access="read"/>
    <property name="DaemonVersion" type="s" access="read"/>
    <property name="DeviceName" type="s" access="read"/>
    <signal name="DeviceStateChanged">
      <arg type="y" name="battery"/>
      <arg type="b" name="charging"/>
      <arg type="s" name="connection"/>
      <arg type="s" name="device_name"/>
      <arg type="s" name="device_id"/>
    </signal>
  </interface>
</node>`;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface DeviceState {
    battery: number;
    charging: boolean;
    connection: string;
    deviceName: string;
    deviceId: string;
}

// ---------------------------------------------------------------------------
// Proxy wrapper type
// The makeProxyWrapper return type is hard to pin down without codegen, so we
// use a local structural interface for the parts we actually call.
// ---------------------------------------------------------------------------

export interface DaemonProxy {
    connectSignal(
        name: string,
        cb: (proxy: DaemonProxy, _sender: string, args: unknown[]) => void,
    ): number;
    disconnectSignal(id: number): void;
    GetActiveDeviceStateAsync(
        cb: (result: [number, boolean, string, string, string] | null, err: unknown | null) => void,
    ): void;
    ShowPopupAsync(
        panelX: number,
        panelY: number,
        panelW: number,
        panelH: number,
        cb: (result: unknown | null, err: unknown | null) => void,
    ): void;
    EnsureOverlayRunningAsync(
        cb: (result: unknown | null, err: unknown | null) => void,
    ): void;
    SetGamingModeAsync(
        enabled: boolean,
        cb: (result: unknown | null, err: unknown | null) => void,
    ): void;
    SetHapticsEnabledAsync(
        enabled: boolean,
        cb: (result: unknown | null, err: unknown | null) => void,
    ): void;
    SetSmartShiftAsync(
        enabled: boolean,
        threshold: number,
        cb: (result: unknown | null, err: unknown | null) => void,
    ): void;
    SetDpiAsync(
        dpi: number,
        cb: (result: unknown | null, err: unknown | null) => void,
    ): void;
    SetHostAsync(
        hostIndex: number,
        cb: (result: boolean | null, err: unknown | null) => void,
    ): void;
    // NOTE: every reply below is an ARRAY of out-args, even for
    // single-value methods — GetDpi resolves to [dpi], GetHostNames to
    // [names[]]. Unwrap with res[0] at the call site.
    GetDpiAsync(
        cb: (result: [number] | null, err: unknown | null) => void,
    ): void;
    GetSmartShiftAsync(
        cb: (result: [boolean, number] | null, err: unknown | null) => void,
    ): void;
    /** Returns (num_hosts, current_host) — count FIRST, matching the daemon. */
    GetEasySwitchInfoAsync(
        cb: (result: [number, number] | null, err: unknown | null) => void,
    ): void;
    GetHostNamesAsync(
        cb: (result: [string[]] | null, err: unknown | null) => void,
    ): void;
    GetGamingModeAsync(
        cb: (result: [boolean] | null, err: unknown | null) => void,
    ): void;
    GetThumbWheelStatusAsync(
        cb: (result: [boolean, boolean] | null, err: unknown | null) => void,
    ): void;
    SetThumbWheelInvertAsync(
        invert: boolean,
        cb: (result: unknown | null, err: unknown | null) => void,
    ): void;
    GetRadialEnabledAsync(
        cb: (result: [boolean] | null, err: unknown | null) => void,
    ): void;
    SetRadialEnabledAsync(
        enabled: boolean,
        cb: (result: unknown | null, err: unknown | null) => void,
    ): void;
    // Cached D-Bus properties (declared in IFACE_XML) — read directly,
    // no round-trip.
    readonly HapticsEnabled?: boolean;
    readonly GamingModeEnabled?: boolean;
    readonly DaemonVersion?: string;
    readonly DeviceName?: string;
}

// ---------------------------------------------------------------------------
// Mock mode flag — checked once at module evaluation time.
// ---------------------------------------------------------------------------

const MOCK_MODE = GLib.getenv('OXIDEMX_INDICATOR_MOCK') === '1';

// ---------------------------------------------------------------------------
// BatteryClient
// ---------------------------------------------------------------------------

export class BatteryClient {
    private readonly _refreshIntervalSec: number;
    private readonly _cancellable: Gio.Cancellable;

    private _proxy: DaemonProxy | null = null;
    private _signalId: number | null = null;
    private _timerId: number | null = null;
    private _latest: DeviceState | null = null;
    private _mockPct: number = 100;

    private readonly _subscribers: Array<(state: DeviceState) => void> = [];

    constructor(refreshIntervalSec: number, cancellable: Gio.Cancellable) {
        this._refreshIntervalSec = refreshIntervalSec;
        this._cancellable = cancellable;
    }

    /**
     * Subscribe to device-state changes.
     * Returns a teardown closure; call it from disable() / stop().
     */
    onStateChange(cb: (state: DeviceState) => void): () => void {
        this._subscribers.push(cb);
        return () => {
            const idx = this._subscribers.indexOf(cb);
            if (idx !== -1) this._subscribers.splice(idx, 1);
        };
    }

    /** Start the D-Bus proxy + timer (or mock timer in MOCK_MODE). */
    start(): void {
        if (MOCK_MODE) {
            this._startMock();
        } else {
            this._startDbus();
        }
    }

    /** Disconnect proxy, cancel timer, clear subscribers. */
    stop(): void {
        if (this._timerId !== null) {
            GLib.source_remove(this._timerId);
            this._timerId = null;
        }
        if (this._proxy !== null && this._signalId !== null) {
            this._proxy.disconnectSignal(this._signalId);
            this._signalId = null;
        }
        this._proxy = null;
        this._subscribers.length = 0;
    }

    /** Last-known device state, or null before the first poll/signal. */
    latest(): DeviceState | null {
        return this._latest;
    }

    /** D-Bus proxy for settings and actions. */
    get proxy(): DaemonProxy | null {
        return this._proxy;
    }

    // ---- internal ----

    private _notify(state: DeviceState): void {
        // Signal payload carries empty strings for deviceName/connection/id by
        // design — merge from last known state so the tooltip doesn't blank
        // between battery emit and the next poll.
        // See daemon/src/battery.rs Task 0.2b TODO.
        if (this._latest) {
            if (!state.deviceName) state = { ...state, deviceName: this._latest.deviceName };
            if (!state.connection) state = { ...state, connection: this._latest.connection };
            if (!state.deviceId)   state = { ...state, deviceId:   this._latest.deviceId   };
        }
        this._latest = state;
        for (const cb of this._subscribers) {
            try {
                cb(state);
            } catch (e) {
                logError(e as object, '[oxidemx-indicator] BatteryClient subscriber threw');
            }
        }
    }

    private _startDbus(): void {
        // makeProxyWrapper returns a factory function (not a constructor).
        // Signature: (bus, name, path, asyncCallback?, cancellable?, flags?) => T & DBusProxy
        const makeProxy = Gio.DBusProxy.makeProxyWrapper<DaemonProxy>(IFACE_XML);

        makeProxy(
            Gio.DBus.session,
            'org.oxidemx.Daemon',
            '/org/oxidemx/Daemon',
            (p: (DaemonProxy & Gio.DBusProxy) | null, err: unknown | null) => {
                if (err || !p) {
                    log(`[oxidemx-indicator] daemon proxy init failed: ${err}`);
                    return;
                }
                this._proxy = p;
                this._bindSignal();
                this._poll();
            },
            this._cancellable,
        );
    }

    private _bindSignal(): void {
        if (!this._proxy) return;
        this._signalId = this._proxy.connectSignal(
            'DeviceStateChanged',
            (_proxy: DaemonProxy, _sender: string, args: unknown[]) => {
                const state = this._parseArgs(args);
                if (state) this._notify(state);
            },
        );

        // Start the poll-fallback timer.
        this._timerId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT,
            this._refreshIntervalSec,
            () => {
                this._poll();
                return GLib.SOURCE_CONTINUE;
            },
        );
    }

    private _poll(): void {
        if (!this._proxy) return;
        this._proxy.GetActiveDeviceStateAsync(
            (result: [number, boolean, string, string, string] | null, err: unknown | null) => {
                if (err || !result) {
                    log(`[oxidemx-indicator] GetActiveDeviceState failed: ${err}`);
                    return;
                }
                const [battery, charging, connection, device_name, device_id] = result;
                const state: DeviceState = {
                    battery,
                    charging,
                    connection,
                    deviceName: device_name,
                    deviceId: device_id,
                };
                this._notify(state);
            },
        );
    }

    private _parseArgs(args: unknown[]): DeviceState | null {
        if (!Array.isArray(args) || args.length < 5) return null;
        const [battery, charging, connection, device_name, device_id] = args as [
            number,
            boolean,
            string,
            string,
            string,
        ];
        return {
            battery: Number(battery),
            charging: Boolean(charging),
            connection: String(connection),
            deviceName: String(device_name),
            deviceId: String(device_id),
        };
    }

    // ---- mock mode ----

    private _startMock(): void {
        this._mockPct = 100;
        this._timerId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT,
            this._refreshIntervalSec,
            () => {
                this._mockPct -= 1;
                if (this._mockPct < 0) this._mockPct = 100;
                const state: DeviceState = {
                    battery: this._mockPct,
                    charging: false,
                    connection: 'bluetooth',
                    deviceName: 'Mock MX Master 4',
                    deviceId: 'mock-1',
                };
                this._notify(state);
                return GLib.SOURCE_CONTINUE;
            },
        );
    }
}

// ---------------------------------------------------------------------------
// CriticalNotifier
// ---------------------------------------------------------------------------

export class CriticalNotifier {
    private readonly _settings: IndicatorSettings;
    /** True when the notifier is ready to fire (not yet triggered this cycle). */
    private _armed: boolean = true;

    constructor(settings: IndicatorSettings) {
        this._settings = settings;
    }

    /**
     * Observe a new DeviceState and fire a critical notification if warranted.
     *
     * The notifier is "armed" at construction and after the device exits the
     * critical band (charges above critical threshold or starts charging).
     * When armed and the device enters the critical-and-not-charging band, a
     * one-shot Gio.Notification is emitted and the notifier disarms until the
     * band exits.
     *
     * This one-shot pattern prevents notification spam when the battery stays
     * at 15% for half an hour — only the crossing fires, not every poll tick.
     */
    observe(state: DeviceState): void {
        const band = bandFor(
            state.battery,
            state.charging,
            this._settings.thresholds(),
        );

        // Re-arm when the device is charging or has exited the critical band.
        if (band !== 'critical') {
            this._armed = true;
            return;
        }

        // band === 'critical' and not charging (bandFor already handles charging → 'charging').
        if (!this._armed) return;

        // First crossing into critical while armed — fire the notification.
        this._armed = false;
        this._sendNotification(state);
    }

    private _sendNotification(state: DeviceState): void {
        const notification = new Gio.Notification();
        notification.set_title('MX device low');
        notification.set_body(
            `${state.deviceName} at ${state.battery}% — connect charging cable`,
        );
        notification.set_priority(Gio.NotificationPriority.URGENT);
        // Application-level notifications require a Gio.Application; inside a
        // GNOME Shell extension the conventional approach is to send via the
        // org.gnome.Shell application id which is always running.
        const app = Gio.Application.get_default();
        if (app) {
            app.send_notification('oxidemx-battery-critical', notification);
        } else {
            // Fallback: no app context, log to journal.
            log(`[oxidemx-indicator] critical battery: ${state.deviceName} at ${state.battery}%`);
        }
    }
}
