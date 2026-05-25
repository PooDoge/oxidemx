/**
 * D-Bus client for the JuhRadial daemon battery surface.
 *
 * BatteryClient opens a session-bus proxy to org.juhradial.Daemon,
 * subscribes to the DeviceStateChanged signal for push updates, and
 * runs a poll-fallback timer for resilience when the signal stream stalls.
 *
 * CriticalNotifier watches the incoming state stream and fires a one-shot
 * Gio.Notification when the device first enters the critical-and-not-charging
 * band. It re-arms when the device exits the critical band or starts charging.
 *
 * Mock mode (JUHRADIAL_INDICATOR_MOCK=1): BatteryClient skips the D-Bus
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
  <interface name="org.juhradial.Daemon">
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

interface DaemonProxy {
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
}

// ---------------------------------------------------------------------------
// Mock mode flag — checked once at module evaluation time.
// ---------------------------------------------------------------------------

const MOCK_MODE = GLib.getenv('JUHRADIAL_INDICATOR_MOCK') === '1';

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

    // ---- internal ----

    private _notify(state: DeviceState): void {
        this._latest = state;
        for (const cb of this._subscribers) {
            try {
                cb(state);
            } catch (e) {
                logError(e as object, '[juhradial-indicator] BatteryClient subscriber threw');
            }
        }
    }

    private _startDbus(): void {
        // makeProxyWrapper returns a factory function (not a constructor).
        // Signature: (bus, name, path, asyncCallback?, cancellable?, flags?) => T & DBusProxy
        const makeProxy = Gio.DBusProxy.makeProxyWrapper<DaemonProxy>(IFACE_XML);

        makeProxy(
            Gio.DBus.session,
            'org.juhradial.Daemon',
            '/org/juhradial/Daemon',
            (p: (DaemonProxy & Gio.DBusProxy) | null, err: unknown | null) => {
                if (err || !p) {
                    log(`[juhradial-indicator] daemon proxy init failed: ${err}`);
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
                    log(`[juhradial-indicator] GetActiveDeviceState failed: ${err}`);
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
            app.send_notification('juhradial-battery-critical', notification);
        } else {
            // Fallback: no app context, log to journal.
            log(`[juhradial-indicator] critical battery: ${state.deviceName} at ${state.battery}%`);
        }
    }
}
