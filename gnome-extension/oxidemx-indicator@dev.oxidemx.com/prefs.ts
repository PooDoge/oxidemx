/**
 * OxideMX Indicator — libadwaita preferences dialog.
 *
 * Phase 1C.  Single Adw.PreferencesPage with seven groups:
 *   1. Preview       — live pill mirroring current settings (mock 42%).
 *   2. Display       — show-as radio + show/tint mouse-glyph switches.
 *   3. Battery level colors — band bar + colour pickers + thresholds.
 *   4. Placement     — panel-target combo + position radio + index spin.
 *   5. Behavior      — click-behavior combo + refresh-interval spinrow.
 *   6. About         — version / GNOME compat / license + buttons.
 *   7. Reset         — destructive-action footer button.
 *
 * SPDX-License-Identifier: GPL-3.0
 */

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';
import Gdk from 'gi://Gdk';
import Gio from 'gi://Gio';
// Cairo uses GJS's bare-module identifier ('cairo') rather than a
// gi:// URI. This resolves because @girs/gjs declares a module
// "cairo" in its ambient typing; if you switch to gi://cairo (a
// non-existent module URI) tsc will fail.
import Cairo from 'cairo';

// ExtensionPreferences is imported from the resource:// URL mapped in tsconfig.json
import { ExtensionPreferences } from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

import {
    IndicatorSettings,
    DisplayMode,
    PanelTarget,
    ClickBehavior,
} from './lib/settings.js';
import { bandFor, colorForBand } from './lib/format.js';

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VERSION = '0.0.1';
const ISSUE_URL = 'https://github.com/PooDoge/oxidemx/issues';
const MOCK_BATTERY = 42;

// Map panel-target enum values → display labels (and back by index).
const PANEL_TARGET_VALUES: PanelTarget[] = ['auto', 'topbar', 'dtp', 'both'];
const PANEL_TARGET_LABELS = [
    'Auto-detect',
    'Top bar only',
    'Dash to Panel only',
    'Both panels (currently top-bar only)',
];

// Map click-behavior enum values → display labels.
const CLICK_BEHAVIOR_VALUES: ClickBehavior[] = ['popup', 'settings', 'none'];
const CLICK_BEHAVIOR_LABELS = [
    'Open OxideMX popup',
    'Open OxideMX Settings',
    'Do nothing',
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Parse a #RRGGBB hex string into a Gdk.RGBA.
 * Returns an opaque black on parse failure.
 */
function hexToRgba(hex: string): Gdk.RGBA {
    const rgba = new Gdk.RGBA();
    const ok = rgba.parse(hex);
    if (!ok) rgba.parse('#000000');
    return rgba;
}

/**
 * Convert a Gdk.RGBA to a #RRGGBB hex string (ignores alpha).
 * Gdk.RGBA.to_string() emits "rgb(r,g,b)" — we convert manually.
 */
function rgbaToHex(rgba: Gdk.RGBA): string {
    const r = Math.round(rgba.red * 255);
    const g = Math.round(rgba.green * 255);
    const b = Math.round(rgba.blue * 255);
    return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`;
}

/**
 * Tint (or clear) a Gtk.Image symbolic icon by attaching a per-widget
 * Gtk.CssProvider that overrides the `color` property.
 *
 * Uses a module-level WeakMap so repeated calls replace the previous provider
 * rather than stacking indefinitely.
 *
 * @param img   - The image widget to tint.
 * @param color - #RRGGBB hex string, or null to clear the tint.
 */
const _imageProviders = new WeakMap<Gtk.Image, Gtk.CssProvider>();

function _applyMouseImgTint(img: Gtk.Image, color: string | null): void {
    // Remove the old provider if present.
    const old = _imageProviders.get(img);
    if (old !== undefined) {
        img.get_style_context().remove_provider(old);
        _imageProviders.delete(img);
    }
    if (color === null) return;

    const provider = new Gtk.CssProvider();
    provider.load_from_string(`image { color: ${color}; }`);
    img.get_style_context().add_provider(provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION);
    _imageProviders.set(img, provider);
}

/**
 * Create a color-button widget compatible with GNOME 45–51.
 *
 * Gtk.ColorDialogButton (GTK 4.10+, GNOME 46+) is the modern API.
 * Gtk.ColorButton is the deprecated but widely-supported fallback for 45.
 * We detect at runtime which is available and return either.
 *
 * @param initialHex - The initial #RRGGBB color.
 * @param onChanged  - Called with the new #RRGGBB string whenever color changes.
 * @returns A Gtk.Widget (either ColorDialogButton or ColorButton).
 */
function makeColorButton(
    initialHex: string,
    onChanged: (hex: string) => void,
): Gtk.Widget {
    // Prefer ColorDialogButton if available (GNOME 46+ / GTK 4.10+).
    // TypeScript cast: the @girs typing declares it, but an older runtime may
    // lack it; guard with a property-existence check.
    const CDB = (Gtk as any).ColorDialogButton as typeof Gtk.ColorDialogButton | undefined;
    if (CDB !== undefined) {
        const dialog = new Gtk.ColorDialog({ title: 'Choose color', with_alpha: false });
        const btn = new CDB({ dialog });
        btn.rgba = hexToRgba(initialHex);
        btn.connect('notify::rgba', () => {
            onChanged(rgbaToHex(btn.rgba));
        });
        // Compact sizing for the band grid.
        btn.valign = Gtk.Align.CENTER;
        return btn;
    }

    // Fallback: Gtk.ColorButton (deprecated since GTK 4.10 / GNOME 46).
    // Keep working on GNOME 45. The @girs typing exists; cast as any to avoid
    // noisy deprecation errors from tsc (the @girs adw-1 typings don't suppress
    // them via @deprecated in all builds).
    const btn = new (Gtk.ColorButton as any)({
        title: 'Choose color',
        use_alpha: false,
    }) as Gtk.ColorButton;
    (btn as any).set_rgba(hexToRgba(initialHex));
    btn.connect('color-set', () => {
        onChanged(rgbaToHex((btn as any).get_rgba()));
    });
    btn.valign = Gtk.Align.CENTER;
    return btn as unknown as Gtk.Widget;
}

// ---------------------------------------------------------------------------
// Main export
// ---------------------------------------------------------------------------

export default class OxideMXIndicatorPrefs extends ExtensionPreferences {
    // fillPreferencesWindow(window) — GNOME 45+ prefs entry point.
    //
    // The `window` parameter is typed `any` rather than `Adw.PreferencesWindow`
    // because the @girs/gnome-shell package vendors its own nested copy of
    // @girs/adw-1. The two copies are structurally very close but not identical
    // (e.g. GNOME 50+ adds `get_accessible_id`), so TypeScript reports TS2416
    // when the signature references the top-level package's Adw type.  Typing the
    // argument as `any` and immediately casting below is the idiomatic workaround
    // recommended for this class of @girs cross-package type mismatch.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    fillPreferencesWindow(window: any): Promise<void> {
        const _window = window as Adw.PreferencesWindow;
        // this.getSettings() returns a Gio.Settings from a slightly different
        // declaration path (gnome-shell's nested dep); cast via `any`.
        const gioSettings = this.getSettings() as any as Gio.Settings;
        const settings = new IndicatorSettings(gioSettings);

        const page = new Adw.PreferencesPage();
        _window.add(page);

        // Collect teardown closures — unsubscribed when the window closes.
        const unsubs: Array<() => void> = [];

        // ---- groups ----
        const { group: previewGroup, refresh: refreshPreview } =
            this._buildPreviewGroup(settings);
        page.add(previewGroup);

        const display = this._buildDisplayGroup(settings, refreshPreview);
        page.add(display.group);
        unsubs.push(...display.unsubs);

        const colors = this._buildColorsGroup(settings, refreshPreview);
        page.add(colors.group);
        unsubs.push(...colors.unsubs);

        const placement = this._buildPlacementGroup(settings);
        page.add(placement.group);
        unsubs.push(...placement.unsubs);

        const behavior = this._buildBehaviorGroup(settings);
        page.add(behavior.group);
        unsubs.push(...behavior.unsubs);

        page.add(this._buildAboutGroup());
        page.add(this._buildResetGroup(settings));

        // Subscribe to any settings change → repaint preview pill.
        unsubs.push(settings.onChange(() => refreshPreview()));

        // Tear down subscriptions when the window closes.
        _window.connect('close-request', () => {
            for (const unsub of unsubs) {
                try { unsub(); } catch { /* ignore */ }
            }
            return false; // do not prevent close
        });

        return Promise.resolve();
    }

    // =========================================================================
    // 1. Preview
    // =========================================================================

    private _buildPreviewGroup(
        settings: IndicatorSettings,
    ): { group: Adw.PreferencesGroup; refresh: () => void } {
        const group = new Adw.PreferencesGroup({
            title: 'Preview',
        });

        const row = new Adw.ActionRow({
            title: 'Preview',
            subtitle: 'Reflects the current settings live; battery shown is a mock value of 42%.',
        });
        group.add(row);

        // ---- pill container ----
        const pill = new Gtk.Box({
            orientation: Gtk.Orientation.HORIZONTAL,
            spacing: 4,
            valign: Gtk.Align.CENTER,
            margin_start: 8,
            margin_end: 8,
        });
        row.add_suffix(pill);

        // Mouse-glyph image.
        const mouseImg = new Gtk.Image({
            icon_name: 'input-mouse-symbolic',
            pixel_size: 14,
            valign: Gtk.Align.CENTER,
        });
        pill.append(mouseImg);

        // Battery glyph — a small filled rectangle representing charge level.
        const battArea = new Gtk.DrawingArea();
        battArea.set_content_width(24);
        battArea.set_content_height(14);
        battArea.valign = Gtk.Align.CENTER;
        pill.append(battArea);

        // Percentage label.
        const pctLabel = new Gtk.Label({
            label: `${MOCK_BATTERY}%`,
            valign: Gtk.Align.CENTER,
        });
        pill.append(pctLabel);

        // ---- refresh function — called on every settings change ----
        const refresh = () => {
            const mode = settings.displayMode();
            const showMouse = settings.showMouseGlyph();
            const tint = settings.tintMouseGlyph();

            const band = bandFor(MOCK_BATTERY, false, settings.thresholds());
            const color = colorForBand(band, settings.bandColors());

            // Mouse glyph visibility + tint.
            mouseImg.visible = showMouse && mode !== 'none';
            if (showMouse && tint && mode !== 'none') {
                // Tint the symbolic icon by injecting a per-widget CSS provider.
                // Gtk.CssProvider → StyleContext.add_provider is the GTK4 way;
                // St.Widget.set_style (GNOME Shell compositor) is not available here.
                _applyMouseImgTint(mouseImg, color);
            } else {
                _applyMouseImgTint(mouseImg, null);
            }

            // Battery glyph.
            battArea.visible = (mode === 'icon' || mode === 'both');

            // Percentage label.
            pctLabel.visible = (mode === 'percent' || mode === 'both');
            if (settings.applyColorToText()) {
                pctLabel.set_markup(`<span foreground="${color}">${MOCK_BATTERY}%</span>`);
            } else {
                pctLabel.label = `${MOCK_BATTERY}%`;
            }

            // Repaint battery drawing area.
            battArea.queue_draw();
        };

        // Battery glyph draw function.
        battArea.set_draw_func((_area: Gtk.DrawingArea, cr: Cairo.Context, w: number, h: number) => {
            const band = bandFor(MOCK_BATTERY, false, settings.thresholds());
            const color = colorForBand(band, settings.bandColors());
            const rgba = hexToRgba(color);

            // Outline.
            cr.setSourceRGBA(1, 1, 1, 0.6);
            cr.rectangle(0, 0, w - 3, h);
            cr.stroke();

            // Fill proportional to MOCK_BATTERY%.
            const fillW = Math.round(((w - 3) * MOCK_BATTERY) / 100);
            cr.setSourceRGB(rgba.red, rgba.green, rgba.blue);
            cr.rectangle(1, 1, fillW - 2, h - 2);
            cr.fill();

            // Nub.
            cr.setSourceRGBA(1, 1, 1, 0.6);
            cr.rectangle(w - 3, Math.round(h * 0.25), 3, Math.round(h * 0.5));
            cr.fill();
        });

        // Initial render.
        refresh();

        return { group, refresh };
    }

    // =========================================================================
    // 2. Display
    // =========================================================================

    private _buildDisplayGroup(
        settings: IndicatorSettings,
        refreshPreview: () => void,
    ): { group: Adw.PreferencesGroup; unsubs: Array<() => void> } {
        const group = new Adw.PreferencesGroup({ title: 'Display' });
        const unsubs: Array<() => void> = [];

        // ---- "Show as" row with toggle buttons ----
        const showAsRow = new Adw.ActionRow({
            title: 'Show as',
            subtitle: 'Pick what OxideMX draws in the panel.',
        });
        group.add(showAsRow);

        const modeBox = new Gtk.Box({
            orientation: Gtk.Orientation.HORIZONTAL,
            spacing: 0,
            valign: Gtk.Align.CENTER,
        });
        modeBox.add_css_class('linked');
        showAsRow.add_suffix(modeBox);

        const modeValues: DisplayMode[] = ['percent', 'icon', 'both', 'none'];
        const modeLabels = ['Percent', 'Icon', 'Both', 'Hidden'];
        const modeButtons: Gtk.ToggleButton[] = [];

        let _suppressModeSignal = false;

        const updateModeButtons = (active: DisplayMode) => {
            _suppressModeSignal = true;
            for (let i = 0; i < modeButtons.length; i++) {
                modeButtons[i].active = (modeValues[i] === active);
            }
            _suppressModeSignal = false;
        };

        for (let i = 0; i < modeValues.length; i++) {
            const btn = new Gtk.ToggleButton({ label: modeLabels[i] });
            if (i === 0) {
                // First button is the group leader (no group arg needed).
            } else {
                btn.set_group(modeButtons[0]);
            }
            btn.active = (modeValues[i] === settings.displayMode());
            const capturedValue = modeValues[i];
            btn.connect('toggled', () => {
                if (_suppressModeSignal) return;
                if (btn.active) {
                    settings.setDisplayMode(capturedValue);
                    refreshPreview();
                }
            });
            modeBox.append(btn);
            modeButtons.push(btn);
        }

        // Keep buttons in sync when settings change externally.
        unsubs.push(settings.onChange((key) => {
            if (key === 'display-mode') updateModeButtons(settings.displayMode());
        }));

        // ---- "Show symbolic mouse glyph" switch row ----
        const showGlyphRow = new Adw.SwitchRow({
            title: 'Show symbolic mouse glyph',
            subtitle: 'Draws a small mouse icon next to the battery indicator.',
        });
        showGlyphRow.active = settings.showMouseGlyph();
        showGlyphRow.connect('notify::active', () => {
            settings.setShowMouseGlyph(showGlyphRow.active);
            tintGlyphRow.sensitive = showGlyphRow.active;
            refreshPreview();
        });
        group.add(showGlyphRow);

        // ---- "Tint glyph by battery level" switch row ----
        const tintGlyphRow = new Adw.SwitchRow({
            title: 'Tint glyph by battery level',
            subtitle: 'Color the mouse glyph using the active battery-level color instead of the foreground.',
        });
        tintGlyphRow.active = settings.tintMouseGlyph();
        tintGlyphRow.sensitive = settings.showMouseGlyph();
        tintGlyphRow.connect('notify::active', () => {
            settings.setTintMouseGlyph(tintGlyphRow.active);
            refreshPreview();
        });
        group.add(tintGlyphRow);

        // Sync on external changes.
        unsubs.push(settings.onChange((key) => {
            if (key === 'show-mouse-glyph') {
                showGlyphRow.active = settings.showMouseGlyph();
                tintGlyphRow.sensitive = settings.showMouseGlyph();
            }
            if (key === 'tint-mouse-glyph') {
                tintGlyphRow.active = settings.tintMouseGlyph();
            }
        }));

        return { group, unsubs };
    }

    // =========================================================================
    // 3. Battery level colors
    // =========================================================================

    private _buildColorsGroup(
        settings: IndicatorSettings,
        refreshPreview: () => void,
    ): { group: Adw.PreferencesGroup; unsubs: Array<() => void> } {
        const group = new Adw.PreferencesGroup({ title: 'Battery level colors' });
        const unsubs: Array<() => void> = [];

        // Wrap everything in a vertical Gtk.Box and add it once to the group.
        const vbox = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 8,
            margin_top: 4,
            margin_bottom: 4,
            margin_start: 4,
            margin_end: 4,
        });
        group.add(vbox);

        // ---- Threshold bar ----
        const barArea = new Gtk.DrawingArea();
        barArea.set_content_height(22);
        barArea.set_hexpand(true);
        vbox.append(barArea);

        const drawBar = (_area: Gtk.DrawingArea, cr: Cairo.Context, w: number, h: number) => {
            const tc = settings.thresholdCritical();
            const tl = settings.thresholdLow();
            const colors = settings.bandColors();

            // Segments: critical, low, healthy.
            const bands: Array<{ hex: string; startPct: number; endPct: number }> = [
                { hex: colors.critical, startPct: 0,   endPct: tc  },
                { hex: colors.low,      startPct: tc,  endPct: tl  },
                { hex: colors.healthy,  startPct: tl,  endPct: 100 },
            ];

            for (const { hex, startPct, endPct } of bands) {
                const x = Math.round((startPct / 100) * w);
                const bandW = Math.round(((endPct - startPct) / 100) * w);
                const rgba = hexToRgba(hex);
                cr.setSourceRGB(rgba.red, rgba.green, rgba.blue);
                cr.rectangle(x, 0, bandW, h);
                cr.fill();
            }

            // Divider lines at the two threshold points.
            const thresholds = [tc, tl];
            for (const pct of thresholds) {
                const x = Math.round((pct / 100) * w);
                cr.setLineWidth(2);
                cr.setSourceRGBA(0, 0, 0, 0.6);
                cr.moveTo(x, 0);
                cr.lineTo(x, h);
                cr.stroke();
            }

            // Percent labels for thresholds.
            cr.setSourceRGBA(1, 1, 1, 0.85);
            // Draw label text at each threshold point using simple positioning.
            // (Pango layout would require gi://Pango — out of scope for this file;
            // the phase-1D CSS pass can embellish. Labels at the dividers.)
            const labels = [`${tc}%`, `${tl}%`];
            for (let i = 0; i < thresholds.length; i++) {
                const pct = thresholds[i];
                const x = Math.round((pct / 100) * w);
                // moveTo a few px right of the divider, vertically centred.
                cr.moveTo(x + 3, h - 4);
                cr.showText(labels[i]);
            }
        };

        barArea.set_draw_func(drawBar);

        // ---- 3-column band grid ----
        const bandGrid = new Gtk.Grid({
            column_spacing: 8,
            row_spacing: 4,
            margin_top: 4,
        });
        vbox.append(bandGrid);

        type BandSpec = {
            key: 'critical' | 'low' | 'healthy';
            label: string;
            rangeGetter: () => string;
            getHex: () => string;
            setHex: (v: string) => void;
        };

        const bandSpecs: BandSpec[] = [
            {
                key: 'critical',
                label: 'Critical',
                rangeGetter: () => `0–${settings.thresholdCritical()}%`,
                getHex: () => settings.colorCritical(),
                setHex: (v) => settings.setColorCritical(v),
            },
            {
                key: 'low',
                label: 'Low',
                rangeGetter: () => `${settings.thresholdCritical() + 1}–${settings.thresholdLow()}%`,
                getHex: () => settings.colorLow(),
                setHex: (v) => settings.setColorLow(v),
            },
            {
                key: 'healthy',
                label: 'Healthy',
                rangeGetter: () => `${settings.thresholdLow() + 1}–100%`,
                getHex: () => settings.colorHealthy(),
                setHex: (v) => settings.setColorHealthy(v),
            },
        ];

        const rangeLabels: Gtk.Label[] = [];

        for (let col = 0; col < bandSpecs.length; col++) {
            const spec = bandSpecs[col];

            // Outer cell box.
            const cell = new Gtk.Box({
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 4,
            });

            // Top row: swatch + name.
            const topRow = new Gtk.Box({
                orientation: Gtk.Orientation.HORIZONTAL,
                spacing: 6,
                valign: Gtk.Align.CENTER,
            });
            cell.append(topRow);

            // 16px color swatch.
            const swatch = new Gtk.DrawingArea();
            swatch.set_content_width(16);
            swatch.set_content_height(16);
            swatch.valign = Gtk.Align.CENTER;
            const capturedSpec = spec;
            swatch.set_draw_func((_a: Gtk.DrawingArea, cr: Cairo.Context, sw: number, sh: number) => {
                const rgba = hexToRgba(capturedSpec.getHex());
                cr.setSourceRGB(rgba.red, rgba.green, rgba.blue);
                cr.rectangle(0, 0, sw, sh);
                cr.fill();
            });
            topRow.append(swatch);

            // Bold label — 12px via markup.
            const nameLabel = new Gtk.Label({
                label: spec.label,
                valign: Gtk.Align.CENTER,
                xalign: 0,
            });
            nameLabel.set_markup(`<b><small>${spec.label}</small></b>`);
            topRow.append(nameLabel);

            // Range sub-label.
            const rangeLabel = new Gtk.Label({
                label: spec.rangeGetter(),
                valign: Gtk.Align.START,
                xalign: 0,
            });
            rangeLabel.set_markup(`<small>${spec.rangeGetter()}</small>`);
            rangeLabel.add_css_class('dim-label');
            cell.append(rangeLabel);
            rangeLabels.push(rangeLabel);

            // Color picker button.
            const colorBtn = makeColorButton(spec.getHex(), (hex) => {
                spec.setHex(hex);
                swatch.queue_draw();
                barArea.queue_draw();
                refreshPreview();
            });
            cell.append(colorBtn);

            // Sync swatch on external changes.
            unsubs.push(settings.onChange((key) => {
                const colorKeys: Record<string, boolean> = {
                    'color-critical': true, 'color-low': true, 'color-healthy': true,
                };
                if (colorKeys[key]) swatch.queue_draw();
            }));

            bandGrid.attach(cell, col, 0, 1, 1);
        }

        // ---- Threshold spinrows ----
        // Adw.SpinRow.new_with_range(min, max, step) → the cleanest constructor.
        const critAdj = new Gtk.Adjustment({
            lower: 1, upper: 99, step_increment: 1, value: settings.thresholdCritical(),
        });
        const critRow = new Adw.SpinRow({
            title: 'Critical threshold',
            subtitle: 'Must be less than Low.',
            adjustment: critAdj,
            numeric: true,
        });
        vbox.append(critRow);

        const lowAdj = new Gtk.Adjustment({
            lower: 1, upper: 99, step_increment: 1, value: settings.thresholdLow(),
        });
        const lowRow = new Adw.SpinRow({
            title: 'Low threshold',
            subtitle: 'Must be greater than Critical.',
            adjustment: lowAdj,
            numeric: true,
        });
        vbox.append(lowRow);

        // Threshold validation + write.
        critRow.connect('notify::value', () => {
            let v = Math.round(critRow.value);
            const tl = Math.round(lowRow.value);
            if (v >= tl) {
                v = tl - 1;
                log('[oxidemx-indicator] threshold clamped: critical must be < low');
                critRow.value = v;
            }
            settings.setThresholdCritical(v);
            _updateRangeLabels();
            barArea.queue_draw();
            refreshPreview();
        });

        lowRow.connect('notify::value', () => {
            let v = Math.round(lowRow.value);
            const tc = Math.round(critRow.value);
            if (v <= tc) {
                v = tc + 1;
                log('[oxidemx-indicator] threshold clamped: low must be > critical');
                lowRow.value = v;
            }
            settings.setThresholdLow(v);
            _updateRangeLabels();
            barArea.queue_draw();
            refreshPreview();
        });

        // Sync spinrows when settings change externally (e.g., reset).
        unsubs.push(settings.onChange((key) => {
            if (key === 'threshold-critical') critRow.value = settings.thresholdCritical();
            if (key === 'threshold-low') lowRow.value = settings.thresholdLow();
            if (key === 'threshold-critical' || key === 'threshold-low') {
                _updateRangeLabels();
                barArea.queue_draw();
            }
        }));

        const _updateRangeLabels = () => {
            const specs = bandSpecs;
            for (let i = 0; i < rangeLabels.length; i++) {
                rangeLabels[i].set_markup(`<small>${specs[i].rangeGetter()}</small>`);
            }
        };

        // ---- Apply colors to percentage text switch ----
        const applyColorRow = new Adw.SwitchRow({
            title: 'Apply colors to percentage text',
            subtitle: 'When off, only the icon picks up the band color.',
        });
        applyColorRow.active = settings.applyColorToText();
        applyColorRow.connect('notify::active', () => {
            settings.setApplyColorToText(applyColorRow.active);
            refreshPreview();
        });
        unsubs.push(settings.onChange((key) => {
            if (key === 'apply-color-to-text') applyColorRow.active = settings.applyColorToText();
        }));
        vbox.append(applyColorRow);

        return { group, unsubs };
    }

    // =========================================================================
    // 4. Placement
    // =========================================================================

    private _buildPlacementGroup(settings: IndicatorSettings): { group: Adw.PreferencesGroup; unsubs: Array<() => void> } {
        const group = new Adw.PreferencesGroup({ title: 'Placement' });
        const unsubs: Array<() => void> = [];

        // ---- Panel combo ----
        const panelModel = new Gtk.StringList();
        for (const label of PANEL_TARGET_LABELS) panelModel.append(label);

        const panelRow = new Adw.ComboRow({
            title: 'Panel',
            subtitle: 'Auto-detect renders on the Dash to Panel tray when installed; otherwise the GNOME top bar.',
            model: panelModel,
        });
        // Set initial selection by index.
        const initialPanelIdx = PANEL_TARGET_VALUES.indexOf(settings.panelTarget());
        panelRow.selected = initialPanelIdx >= 0 ? initialPanelIdx : 0;

        let _suppressPanelSignal = false;
        panelRow.connect('notify::selected', () => {
            if (_suppressPanelSignal) return;
            const idx = panelRow.selected;
            if (idx < PANEL_TARGET_VALUES.length) {
                settings.setPanelTarget(PANEL_TARGET_VALUES[idx]);
            }
        });
        unsubs.push(settings.onChange((key) => {
            if (key === 'panel-target') {
                _suppressPanelSignal = true;
                const idx = PANEL_TARGET_VALUES.indexOf(settings.panelTarget());
                panelRow.selected = idx >= 0 ? idx : 0;
                _suppressPanelSignal = false;
            }
        }));
        group.add(panelRow);

        // ---- Position row ----
        const posRow = new Adw.ActionRow({
            title: 'Position',
            subtitle: 'Where in the panel to place the indicator.',
        });
        group.add(posRow);

        const posBox = new Gtk.Box({
            orientation: Gtk.Orientation.HORIZONTAL,
            spacing: 6,
            valign: Gtk.Align.CENTER,
        });
        posRow.add_suffix(posBox);

        // Left / Center / Right toggle buttons.
        type Position = 'left' | 'center' | 'right';
        const posValues: Position[] = ['left', 'center', 'right'];
        const posLabels = ['Left', 'Center', 'Right'];
        const posButtons: Gtk.ToggleButton[] = [];
        const posToggleBox = new Gtk.Box({
            orientation: Gtk.Orientation.HORIZONTAL,
            spacing: 0,
        });
        posToggleBox.add_css_class('linked');

        let _suppressPosSignal = false;
        const updatePosButtons = (pos: Position) => {
            _suppressPosSignal = true;
            for (let i = 0; i < posButtons.length; i++) {
                posButtons[i].active = (posValues[i] === pos);
            }
            _suppressPosSignal = false;
        };

        for (let i = 0; i < posValues.length; i++) {
            const btn = new Gtk.ToggleButton({ label: posLabels[i] });
            if (i > 0) btn.set_group(posButtons[0]);
            btn.active = (posValues[i] === settings.position());
            const capturedPos = posValues[i];
            btn.connect('toggled', () => {
                if (_suppressPosSignal) return;
                if (btn.active) settings.setPosition(capturedPos);
            });
            posToggleBox.append(btn);
            posButtons.push(btn);
        }
        posBox.append(posToggleBox);

        unsubs.push(settings.onChange((key) => {
            if (key === 'position') updatePosButtons(settings.position() as Position);
        }));

        // Compact position-index SpinButton (not embeddable as SpinRow here).
        const idxAdj = new Gtk.Adjustment({
            lower: 0,
            upper: 99,
            step_increment: 1,
            value: settings.positionIndex(),
        });
        const idxSpin = new Gtk.SpinButton({
            adjustment: idxAdj,
            numeric: true,
            valign: Gtk.Align.CENTER,
            tooltip_text: 'Position index within section',
        });
        idxSpin.set_size_request(60, -1);
        idxSpin.connect('value-changed', () => {
            settings.setPositionIndex(Math.round(idxSpin.value));
        });
        unsubs.push(settings.onChange((key) => {
            if (key === 'position-index') idxSpin.value = settings.positionIndex();
        }));
        posBox.append(idxSpin);

        return { group, unsubs };
    }

    // =========================================================================
    // 5. Behavior
    // =========================================================================

    private _buildBehaviorGroup(settings: IndicatorSettings): { group: Adw.PreferencesGroup; unsubs: Array<() => void> } {
        const group = new Adw.PreferencesGroup({ title: 'Behavior' });
        const unsubs: Array<() => void> = [];

        // ---- On-click combo ----
        const clickModel = new Gtk.StringList();
        for (const label of CLICK_BEHAVIOR_LABELS) clickModel.append(label);

        const clickRow = new Adw.ComboRow({
            title: 'On click',
            subtitle: 'What happens when the user activates the indicator.',
            model: clickModel,
        });
        const initialClickIdx = CLICK_BEHAVIOR_VALUES.indexOf(settings.clickBehavior());
        clickRow.selected = initialClickIdx >= 0 ? initialClickIdx : 0;

        let _suppressClickSignal = false;
        clickRow.connect('notify::selected', () => {
            if (_suppressClickSignal) return;
            const idx = clickRow.selected;
            if (idx < CLICK_BEHAVIOR_VALUES.length) {
                settings.setClickBehavior(CLICK_BEHAVIOR_VALUES[idx]);
            }
        });
        unsubs.push(settings.onChange((key) => {
            if (key === 'click-behavior') {
                _suppressClickSignal = true;
                const idx = CLICK_BEHAVIOR_VALUES.indexOf(settings.clickBehavior());
                clickRow.selected = idx >= 0 ? idx : 0;
                _suppressClickSignal = false;
            }
        }));
        group.add(clickRow);

        // ---- Poll interval SpinRow + "s" suffix label ----
        const intervalAdj = new Gtk.Adjustment({
            lower: 5,
            upper: 120,
            step_increment: 5,
            value: settings.refreshInterval(),
        });
        const intervalRow = new Adw.SpinRow({
            title: 'Poll interval',
            subtitle: 'How often to read battery + connection state from the daemon. Lower = snappier, higher = lighter.',
            adjustment: intervalAdj,
            numeric: true,
        });
        // Unit suffix label in the row's suffix slot.
        const unitLabel = new Gtk.Label({
            label: 's',
            valign: Gtk.Align.CENTER,
        });
        unitLabel.add_css_class('dim-label');
        intervalRow.add_suffix(unitLabel);

        intervalRow.connect('notify::value', () => {
            settings.setRefreshInterval(Math.round(intervalRow.value));
        });
        unsubs.push(settings.onChange((key) => {
            if (key === 'refresh-interval') intervalRow.value = settings.refreshInterval();
        }));
        group.add(intervalRow);

        return { group, unsubs };
    }

    // =========================================================================
    // 6. About
    // =========================================================================

    private _buildAboutGroup(): Adw.PreferencesGroup {
        const group = new Adw.PreferencesGroup({ title: 'About' });

        const row = new Adw.ActionRow({
            title: 'OxideMX Indicator',
            subtitle: `v${VERSION} · GNOME 45+ · GPL-3.0`,
        });
        group.add(row);

        // "Open daemon" button — no-op for now.
        const daemonBtn = new Gtk.Button({
            label: 'Open daemon',
            valign: Gtk.Align.CENTER,
        });
        daemonBtn.connect('clicked', () => {
            // TODO: launch oxidemx-settings (daemon management page).
            log('[oxidemx-indicator] Open daemon button clicked — not yet wired.');
        });
        row.add_suffix(daemonBtn);

        // "Report issue" button.
        const issueBtn = new Gtk.Button({
            label: 'Report issue',
            valign: Gtk.Align.CENTER,
        });
        issueBtn.connect('clicked', () => {
            // Prefer Gtk.UriLauncher (GTK 4.10+, GNOME 46+); fall back to Gio.
            const UL = (Gtk as any).UriLauncher as typeof Gtk.UriLauncher | undefined;
            if (UL !== undefined) {
                const launcher = new UL({ uri: ISSUE_URL });
                // launch() is async but we don't need the result here.
                launcher.launch(null, null, null);
            } else {
                try {
                    Gio.app_info_launch_default_for_uri(ISSUE_URL, null);
                } catch (e) {
                    log(`[oxidemx-indicator] Failed to open issue URL: ${e}`);
                }
            }
        });
        row.add_suffix(issueBtn);

        return group;
    }

    // =========================================================================
    // 7. Reset to defaults
    // =========================================================================

    private _buildResetGroup(settings: IndicatorSettings): Adw.PreferencesGroup {
        // A bare group with no title — used as a footer container for the button.
        const group = new Adw.PreferencesGroup();

        const resetBtn = new Gtk.Button({
            label: 'Reset to defaults',
            halign: Gtk.Align.END,
            margin_top: 8,
        });
        resetBtn.add_css_class('destructive-action');
        resetBtn.connect('clicked', () => {
            settings.reset();
        });

        // Add the button directly to the group (Adw.PreferencesGroup.add() accepts any Gtk.Widget).
        group.add(resetBtn);

        return group;
    }
}
