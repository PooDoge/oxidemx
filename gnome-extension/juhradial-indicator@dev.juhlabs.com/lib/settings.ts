/**
 * Typed wrapper around Gio.Settings for the juhradial-indicator extension.
 *
 * Every GSettings key is exposed as a typed getter/setter pair so callers
 * never have to deal with raw string keys or untyped GVariant values.
 * The `onChange` method provides a self-unsubscribing subscription to the
 * `changed` signal, returning a teardown closure that fits cleanly into the
 * extension's `_unsubs` array.
 */

import Gio from 'gi://Gio';

import type { Thresholds, BandColors } from './format.js';

// ---------------------------------------------------------------------------
// String-enum types that mirror the GSettings choices declarations
// ---------------------------------------------------------------------------

export type DisplayMode = 'percent' | 'icon' | 'both';
export type PanelTarget = 'auto' | 'topbar' | 'dtp' | 'both';
export type Position = 'left' | 'center' | 'right';
export type ClickBehavior = 'popup' | 'settings' | 'none';

// ---------------------------------------------------------------------------
// All key names — used by reset() to iterate and by onChange to be explicit.
// ---------------------------------------------------------------------------

const ALL_KEYS: readonly string[] = [
    'display-mode',
    'show-mouse-glyph',
    'tint-mouse-glyph',
    'threshold-critical',
    'threshold-low',
    'color-critical',
    'color-low',
    'color-healthy',
    'color-charging',
    'apply-color-to-text',
    'panel-target',
    'position',
    'position-index',
    'click-behavior',
    'refresh-interval',
] as const;

// ---------------------------------------------------------------------------
// IndicatorSettings
// ---------------------------------------------------------------------------

export class IndicatorSettings {
    private readonly _s: Gio.Settings;

    constructor(gioSettings: Gio.Settings) {
        this._s = gioSettings;
    }

    // ---- display ----

    displayMode(): DisplayMode {
        return this._s.get_string('display-mode') as DisplayMode;
    }
    setDisplayMode(v: DisplayMode): void {
        this._s.set_string('display-mode', v);
    }

    showMouseGlyph(): boolean {
        return this._s.get_boolean('show-mouse-glyph');
    }
    setShowMouseGlyph(v: boolean): void {
        this._s.set_boolean('show-mouse-glyph', v);
    }

    tintMouseGlyph(): boolean {
        return this._s.get_boolean('tint-mouse-glyph');
    }
    setTintMouseGlyph(v: boolean): void {
        this._s.set_boolean('tint-mouse-glyph', v);
    }

    // ---- thresholds ----

    thresholdCritical(): number {
        return this._s.get_int('threshold-critical');
    }
    setThresholdCritical(v: number): void {
        this._s.set_int('threshold-critical', v);
    }

    thresholdLow(): number {
        return this._s.get_int('threshold-low');
    }
    setThresholdLow(v: number): void {
        this._s.set_int('threshold-low', v);
    }

    // ---- colours ----

    colorCritical(): string {
        return this._s.get_string('color-critical');
    }
    setColorCritical(v: string): void {
        this._s.set_string('color-critical', v);
    }

    colorLow(): string {
        return this._s.get_string('color-low');
    }
    setColorLow(v: string): void {
        this._s.set_string('color-low', v);
    }

    colorHealthy(): string {
        return this._s.get_string('color-healthy');
    }
    setColorHealthy(v: string): void {
        this._s.set_string('color-healthy', v);
    }

    colorCharging(): string {
        return this._s.get_string('color-charging');
    }
    setColorCharging(v: string): void {
        this._s.set_string('color-charging', v);
    }

    applyColorToText(): boolean {
        return this._s.get_boolean('apply-color-to-text');
    }
    setApplyColorToText(v: boolean): void {
        this._s.set_boolean('apply-color-to-text', v);
    }

    // ---- placement ----

    panelTarget(): PanelTarget {
        return this._s.get_string('panel-target') as PanelTarget;
    }
    setPanelTarget(v: PanelTarget): void {
        this._s.set_string('panel-target', v);
    }

    position(): Position {
        return this._s.get_string('position') as Position;
    }
    setPosition(v: Position): void {
        this._s.set_string('position', v);
    }

    positionIndex(): number {
        return this._s.get_int('position-index');
    }
    setPositionIndex(v: number): void {
        this._s.set_int('position-index', v);
    }

    // ---- behavior ----

    clickBehavior(): ClickBehavior {
        return this._s.get_string('click-behavior') as ClickBehavior;
    }
    setClickBehavior(v: ClickBehavior): void {
        this._s.set_string('click-behavior', v);
    }

    refreshInterval(): number {
        return this._s.get_int('refresh-interval');
    }
    setRefreshInterval(v: number): void {
        this._s.set_int('refresh-interval', v);
    }

    // ---- aggregate convenience getters ----

    /** Returns the threshold pair as a Thresholds object for use with format.ts. */
    thresholds(): Thresholds {
        return {
            critical: this.thresholdCritical(),
            low: this.thresholdLow(),
        };
    }

    /** Returns the colour set as a BandColors object for use with format.ts. */
    bandColors(): BandColors {
        return {
            critical: this.colorCritical(),
            low: this.colorLow(),
            healthy: this.colorHealthy(),
            charging: this.colorCharging(),
        };
    }

    // ---- subscription ----

    /**
     * Subscribe to any settings change.
     *
     * @param cb - Called with the changed key name whenever any key changes.
     * @returns A teardown function; call it from disable() to disconnect.
     */
    onChange(cb: (key: string) => void): () => void {
        const id = this._s.connect('changed', (_s: Gio.Settings, key: string) => cb(key));
        return () => this._s.disconnect(id);
    }

    // ---- reset ----

    /**
     * Reset every key to its schema default.
     * Backs the "Reset to defaults" button in prefs.ts (Phase 1C).
     */
    reset(): void {
        for (const key of ALL_KEYS) {
            this._s.reset(key);
        }
    }
}
