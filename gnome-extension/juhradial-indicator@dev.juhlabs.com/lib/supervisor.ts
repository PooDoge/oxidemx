/**
 * Stack health supervisor for the JuhRadial indicator.
 *
 * Probes five conditions on each refresh-interval tick and calls subscribers
 * with a StackHealth snapshot. Also provides remediation helpers:
 *   - startDaemon()   — `systemctl --user start juhradialmx-daemon.service`.
 *   - ensureOverlay() — calls org.juhradial.Daemon.EnsureOverlayRunning().
 *
 * Design constraint: no long-lived processes are ever spawned from this code.
 * Only Gio.Subprocess one-shots (systemctl --user start) and D-Bus method
 * calls are used — both complete and exit quickly.
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import type { IndicatorSettings } from './settings.js';
import type { BatteryClient } from './battery.js';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface StackHealth {
    /** org.juhradial.Daemon is on the session bus AND systemd unit is active. */
    daemonRunning: boolean;
    /** The active device has a connection other than 'off'. */
    deviceLinked: boolean;
    /** org.juhradial.overlay is on the session bus. */
    overlayRunning: boolean;
    /** juhradial-cursor@dev.juhlabs.com is loaded and ENABLED. */
    cursorExtension: boolean;
    /** Device name from the last BatteryClient state, or '' when unknown. */
    deviceName: string;
}

// ---------------------------------------------------------------------------
// D-Bus name-list helper
// ---------------------------------------------------------------------------

/**
 * List all names currently owned on the session bus.
 * Returns an empty array on failure — callers treat missing names as not-running.
 */
async function sessionBusNames(): Promise<string[]> {
    return new Promise((resolve) => {
        Gio.DBus.session.call(
            'org.freedesktop.DBus',
            '/org/freedesktop/DBus',
            'org.freedesktop.DBus',
            'ListNames',
            null,
            new GLib.VariantType('(as)'),
            Gio.DBusCallFlags.NONE,
            5000, // 5-second timeout — health probes must be fast
            null,
            // AsyncReadyCallback: first param is source_object (DBusConnection | null)
            (_conn: Gio.DBusConnection | null, result: Gio.AsyncResult) => {
                try {
                    const reply = Gio.DBus.session.call_finish(result);
                    // reply is a GLib.Variant of type (as); deep_unpack → [[name, ...]]
                    const unpacked = reply.deep_unpack() as [string[]];
                    resolve(unpacked[0] ?? []);
                } catch (e) {
                    log(`[juhradial-indicator] supervisor: ListNames failed: ${e}`);
                    resolve([]);
                }
            },
        );
    });
}

/**
 * Run `systemctl --user is-active <unit>` and resolve to true when the exit
 * code is zero (unit active).  The process is intentionally short-lived.
 */
async function isSystemdUnitActive(unit: string, cancellable: Gio.Cancellable): Promise<boolean> {
    return new Promise((resolve) => {
        try {
            const proc = Gio.Subprocess.new(
                ['systemctl', '--user', 'is-active', unit],
                Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE,
            );
            // AsyncReadyCallback: first param is source_object (Subprocess | null)
            proc.wait_check_async(cancellable, (_proc: Gio.Subprocess | null, result: Gio.AsyncResult) => {
                try {
                    // Use the known-non-null `proc` reference rather than the possibly-null
                    // callback source_object, to avoid a null-check at the call site.
                    const ok = proc.wait_check_finish(result);
                    resolve(ok);
                } catch {
                    resolve(false);
                }
            });
        } catch (e) {
            log(`[juhradial-indicator] supervisor: is-active check failed: ${e}`);
            resolve(false);
        }
    });
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

export class Supervisor {
    private readonly _settings: IndicatorSettings;
    private readonly _battery: BatteryClient;
    private readonly _cancellable: Gio.Cancellable;

    private _timerId: number | null = null;
    private _lastHealth: StackHealth | null = null;
    private readonly _subscribers: Array<(h: StackHealth) => void> = [];

    constructor(
        settings: IndicatorSettings,
        batteryClient: BatteryClient,
        cancellable: Gio.Cancellable,
    ) {
        this._settings = settings;
        this._battery = batteryClient;
        this._cancellable = cancellable;
    }

    /**
     * Subscribe to health-change events.
     * Returns a teardown closure.
     */
    onHealthChange(cb: (h: StackHealth) => void): () => void {
        this._subscribers.push(cb);
        return () => {
            const idx = this._subscribers.indexOf(cb);
            if (idx !== -1) this._subscribers.splice(idx, 1);
        };
    }

    /** Run initial probe, then schedule periodic probes. */
    start(): void {
        // Kick off first probe asynchronously.
        this.poll().then((h) => this._notifyIfChanged(h)).catch((e: unknown) => {
            logError(e as object, '[juhradial-indicator] supervisor initial poll failed');
        });

        const intervalSec = this._settings.refreshInterval();
        this._timerId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT,
            intervalSec,
            () => {
                this.poll().then((h) => this._notifyIfChanged(h)).catch((e: unknown) => {
                    logError(e as object, '[juhradial-indicator] supervisor poll failed');
                });
                return GLib.SOURCE_CONTINUE;
            },
        );
    }

    /** Cancel timer and clear subscribers. */
    stop(): void {
        if (this._timerId !== null) {
            GLib.source_remove(this._timerId);
            this._timerId = null;
        }
        this._subscribers.length = 0;
    }

    /**
     * Run a single health probe and return the result.
     * Used both by the periodic timer and by extension.ts on-demand.
     */
    async poll(): Promise<StackHealth> {
        const [names, daemonUnit] = await Promise.all([
            sessionBusNames(),
            isSystemdUnitActive('juhradialmx-daemon.service', this._cancellable),
        ]);

        const nameSet = new Set(names);
        const latest = this._battery.latest();

        const health: StackHealth = {
            daemonRunning: nameSet.has('org.juhradial.Daemon') && daemonUnit,
            deviceLinked: latest !== null && latest.connection !== 'off',
            overlayRunning: nameSet.has('org.juhradial.overlay'),
            cursorExtension: this._probeCursorExtension(),
            deviceName: latest?.deviceName ?? '',
        };

        return health;
    }

    /**
     * Start the daemon via `systemctl --user start juhradialmx-daemon.service`.
     * Returns when the systemctl process exits (does not wait for daemon ready).
     */
    async startDaemon(): Promise<void> {
        return new Promise((resolve, reject) => {
            try {
                const proc = Gio.Subprocess.new(
                    ['systemctl', '--user', 'start', 'juhradialmx-daemon.service'],
                    Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE,
                );
                // AsyncReadyCallback: first param is source_object (Subprocess | null).
                // wait_check_async / wait_check_finish throws on non-zero exit so a
                // failed systemctl (unit unknown, masked, etc.) surfaces as a rejected
                // promise — matching the pattern used in isSystemdUnitActive above.
                proc.wait_check_async(this._cancellable, (_proc: Gio.Subprocess | null, result: Gio.AsyncResult) => {
                    try {
                        proc.wait_check_finish(result);
                        resolve();
                    } catch (e) {
                        reject(e);
                    }
                });
            } catch (e) {
                reject(e);
            }
        });
    }

    /**
     * Ask the daemon to ensure its overlay process is running by calling
     * org.juhradial.Daemon.EnsureOverlayRunning() over D-Bus.
     */
    async ensureOverlay(): Promise<void> {
        return new Promise((resolve, reject) => {
            Gio.DBus.session.call(
                'org.juhradial.Daemon',
                '/org/juhradial/Daemon',
                'org.juhradial.Daemon',
                'EnsureOverlayRunning',
                null,
                null,
                Gio.DBusCallFlags.NONE,
                10000,
                this._cancellable,
                // AsyncReadyCallback: first param is source_object (DBusConnection | null)
                (_conn: Gio.DBusConnection | null, result: Gio.AsyncResult) => {
                    try {
                        Gio.DBus.session.call_finish(result);
                        resolve();
                    } catch (e) {
                        reject(e);
                    }
                },
            );
        });
    }

    // ---- internal ----

    private _probeCursorExtension(): boolean {
        const mgr = Main.extensionManager;
        if (!mgr) return false;
        const ext = mgr.lookup('juhradial-cursor@dev.juhlabs.com');
        return ext !== undefined && ext.state === 1; // ExtensionState.ENABLED
    }

    private _notifyIfChanged(health: StackHealth): void {
        // Always notify on first probe; afterwards only when something changed.
        const prev = this._lastHealth;
        const changed =
            prev === null ||
            prev.daemonRunning !== health.daemonRunning ||
            prev.deviceLinked !== health.deviceLinked ||
            prev.overlayRunning !== health.overlayRunning ||
            prev.cursorExtension !== health.cursorExtension ||
            prev.deviceName !== health.deviceName;

        this._lastHealth = health;

        if (changed) {
            for (const cb of this._subscribers) {
                try {
                    cb(health);
                } catch (e) {
                    logError(e as object, '[juhradial-indicator] supervisor subscriber threw');
                }
            }
        }
    }
}
