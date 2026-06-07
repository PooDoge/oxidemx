/**
 * OxideMX Native GJS Settings Popup.
 *
 * Implements a settings popover inside GNOME Shell using Clutter/St.
 * Connects directly to the OxideMX daemon via D-Bus proxy for real-time
 * querying and mutations, eliminating the standalone popup-rs window.
 *
 * SPDX-License-Identifier: GPL-3.0
 */

import GObject from 'gi://GObject';
import St from 'gi://St';
import Clutter from 'gi://Clutter';
import Cairo from 'cairo';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import * as Slider from 'resource:///org/gnome/shell/ui/slider.js';
import * as BoxPointer from 'resource:///org/gnome/shell/ui/boxpointer.js';

import { DaemonProxy } from './battery.js';
import { IndicatorSettings } from './settings.js';

declare const TextDecoder: any;

const POPUP_CONTENT_HEIGHT = 460;
const POPUP_WIDTH = 360;

const QUICK_TOGGLE_CATALOG: Record<string, { label: string, icon: string, desc: string }> = {
    'gaming': { label: 'Gaming Mode', icon: 'applications-games-symbolic', desc: 'Hides the radial, bumps DPI' },
    'haptics': { label: 'Haptic Feedback', icon: 'audio-volume-high-symbolic', desc: 'Click-and-hold ticks' },
    'radial': { label: 'Radial Overlay', icon: 'applications-graphics-symbolic', desc: 'Toggle the radial menu' },
    'flow': { label: 'Flow', icon: 'view-grid-symbolic', desc: 'Cross-device scroll & paste' },
    'smart': { label: 'SmartShift', icon: 'system-switch-user-symbolic', desc: 'Free-spin scroll wheel' },
    'highlight': { label: 'Cursor highlight', icon: 'preferences-desktop-cursors-symbolic', desc: 'Pulse ring on shake' }
};

const QUICK_SLIDER_CATALOG: Record<string, { label: string, icon: string, desc: string }> = {
    'dpi': { label: 'Pointer DPI', icon: 'preferences-desktop-cursors-symbolic', desc: '200 – 6,400 dpi' },
    'scroll': { label: 'Scroll sensitivity', icon: 'system-switch-user-symbolic', desc: '1 – 10' },
    'haptic_i': { label: 'Haptic intensity', icon: 'audio-volume-high-symbolic', desc: 'Off – Strong' },
    'accel': { label: 'Pointer acceleration', icon: 'view-grid-symbolic', desc: '-1.0 – 1.0' }
};

export const OxideMXPopup = GObject.registerClass(
    class OxideMXPopup extends PopupMenu.PopupBaseMenuItem {
        private _container!: St.BoxLayout;
        private _scroll!: St.ScrollView;
        private _content!: St.BoxLayout;

        private _proxy: DaemonProxy | null = null;
        private _settings: IndicatorSettings | null = null;

        // Header widgets
        private _deviceLabel!: St.Label;
        private _connectionLabel!: St.Label;

        // Hero Battery widgets
        private _batteryRingArea!: St.DrawingArea;
        private _batteryPercentLabel!: St.Label;
        private _batteryTimeLabel!: St.Label;

        // Dynamic State
        private _batteryPct: number = 0;
        private _charging: boolean = false;
        private _deviceName: string = 'MX Master';
        private _connectionType: string = '–';
        private _gamingMode: boolean = false;
        private _hapticsEnabled: boolean = true;
        private _smartShiftEnabled: boolean = false;
        private _smartShiftThreshold: number = 30;
        private _dpi: number = 1600;
        private _easySwitchCurrent: number = 0;
        private _easySwitchCount: number = 0;
        private _hostNames: string[] = [];

        // UI section containers
        private _hostSection!: St.BoxLayout;
        private _hostBox!: St.BoxLayout;
        private _hostButtons: St.Button[] = [];
        private _togglesSection!: St.BoxLayout;
        private _slidersSection!: St.BoxLayout;

        // @ts-expect-error - GObject subclass overrides signature
        _init(proxy: DaemonProxy | null, settings: IndicatorSettings | null) {
            super._init({
                reactive: false,
                can_focus: false,
                style_class: 'oxidemx-popup-item',
            });

            this._proxy = proxy;
            this._settings = settings;

            this._container = new St.BoxLayout({
                orientation: Clutter.Orientation.VERTICAL,
                style_class: 'oxidemx-popup',
                x_expand: true,
            });
            this.add_child(this._container);

            this._container.set_style(`min-width: ${POPUP_WIDTH}px; max-width: ${POPUP_WIDTH}px;`);

            this._scroll = new St.ScrollView({
                style_class: 'oxidemx-scroll',
                overlay_scrollbars: true,
                x_expand: true,
                hscrollbar_policy: St.PolicyType.NEVER,
                vscrollbar_policy: St.PolicyType.AUTOMATIC,
            });
            this._scroll.set_style(`height: ${POPUP_CONTENT_HEIGHT}px;`);

            this._content = new St.BoxLayout({
                orientation: Clutter.Orientation.VERTICAL,
                style_class: 'oxidemx-content',
                x_expand: true,
            });
            this._scroll.add_child(this._content);
            this._container.add_child(this._scroll);

            // Rebuild elements on click / open
            this._buildUI();
        }

        private _buildUI() {
            this._content.destroy_all_children();

            this._buildHeader();
            this._content.add_child(this._createSeparator());
            this._buildHeroBattery();
            
            // Re-fetch config to construct toggle/slider layouts dynamically
            const config = this._loadConfig();
            const popupCfg = config?.popup || {
                mode: 'simple',
                show_host_buttons: true,
                simple_toggles: ['gaming', 'haptics', 'radial'],
                power_toggles: ['gaming', 'haptics', 'radial', 'flow'],
                power_sliders: ['dpi', 'scroll']
            };

            if (popupCfg.show_host_buttons) {
                this._content.add_child(this._createSeparator());
                this._buildEasySwitch();
            }

            this._content.add_child(this._createSeparator());
            this._buildQuickToggles(popupCfg);

            // Sliders only appear in power user mode
            if (popupCfg.mode === 'power' && popupCfg.power_sliders && popupCfg.power_sliders.length > 0) {
                this._content.add_child(this._createSeparator());
                this._buildQuickSliders(popupCfg);
            }

            this._content.add_child(this._createSeparator());
            this._buildFooter();
        }

        private _loadConfig(): any {
            try {
                const path = GLib.build_filenamev([GLib.get_home_dir(), '.config', 'oxidemx', 'config.json']);
                const file = Gio.File.new_for_path(path);
                const [success, contents] = file.load_contents(null);
                if (success) {
                    const text = new TextDecoder('utf-8').decode(contents);
                    return JSON.parse(text);
                }
            } catch (e) {
                log(`[oxidemx-popup] Failed to load config.json: ${e}`);
            }
            return null;
        }

        private _createSeparator(): St.Widget {
            return new St.Widget({
                style_class: 'oxidemx-popup-separator',
                x_expand: true,
            });
        }

        private _buildHeader() {
            const row = new St.BoxLayout({
                orientation: Clutter.Orientation.HORIZONTAL,
                style_class: 'oxidemx-header',
                x_expand: true,
            });

            this._deviceLabel = new St.Label({
                text: this._deviceName || '',
                style_class: 'oxidemx-device-chip',
                y_align: Clutter.ActorAlign.CENTER,
            });

            this._connectionLabel = new St.Label({
                text: (this._connectionType ? `● ${this._connectionType}` : '○ –') || '',
                style_class: 'oxidemx-connection-status',
                x_expand: true,
                x_align: Clutter.ActorAlign.END,
                y_align: Clutter.ActorAlign.CENTER,
            });

            row.add_child(this._deviceLabel);
            row.add_child(this._connectionLabel);
            this._content.add_child(row);
        }

        private _buildHeroBattery() {
            const row = new St.BoxLayout({
                orientation: Clutter.Orientation.HORIZONTAL,
                style_class: 'oxidemx-hero',
                x_expand: true,
            });

            // DrawingArea for battery circle progress using Cairo
            this._batteryRingArea = new St.DrawingArea({
                style_class: 'oxidemx-battery-ring',
                width: 88,
                height: 88,
                x_expand: false,
                y_expand: false,
            });

            this._batteryRingArea.connect('repaint', (area) => {
                const cr = area.get_context();
                const [w, h] = area.get_surface_size();
                const cx = w / 2;
                const cy = h / 2;
                const strokeWidth = 7;
                const r = (Math.min(w, h) - strokeWidth) / 2;

                // Draw background circle (dimmed color)
                cr.setSourceRGBA(0.18, 0.18, 0.20, 1.0);
                cr.setLineWidth(strokeWidth);
                cr.arc(cx, cy, r, 0, 2 * Math.PI);
                cr.stroke();

                // Draw foreground battery arc
                const pct = this._batteryPct / 100;
                if (pct > 0.01) {
                    const color = this._getBatteryColor(this._batteryPct, this._charging);
                    cr.setSourceRGB(color.r, color.g, color.b);
                    cr.setLineWidth(strokeWidth);
                    cr.arc(cx, cy, r, -Math.PI / 2, -Math.PI / 2 + 2 * Math.PI * pct);
                    cr.stroke();
                }
            });

            // Center the percentage text inside the battery ring using a Clutter.BinLayout
            const ringStack = new St.Widget({
                layout_manager: new Clutter.BinLayout() as any,
                width: 88,
                height: 88,
            });
            
            this._batteryPercentLabel = new St.Label({
                text: `${this._batteryPct}%`,
                style_class: 'oxidemx-battery-percent-label',
                x_align: Clutter.ActorAlign.CENTER,
                y_align: Clutter.ActorAlign.CENTER,
            });

            ringStack.add_child(this._batteryRingArea);
            ringStack.add_child(this._batteryPercentLabel);

            // Right column: Device Name and Battery status
            const rightCol = new St.BoxLayout({
                orientation: Clutter.Orientation.VERTICAL,
                style_class: 'oxidemx-hero-meta',
                x_expand: true,
                y_align: Clutter.ActorAlign.CENTER,
            });

            const title = new St.Label({
                text: this._deviceName || '',
                style_class: 'oxidemx-hero-title',
            });

            this._batteryTimeLabel = new St.Label({
                text: this._estimateRemaining(this._batteryPct, this._charging) || '',
                style_class: 'oxidemx-hero-subtitle',
            });

            rightCol.add_child(title);
            rightCol.add_child(this._batteryTimeLabel);

            row.add_child(ringStack);
            row.add_child(rightCol);
            this._content.add_child(row);
        }

        private _getBatteryColor(pct: number, charging: boolean) {
            let hex = '#ffffff';
            if (this._settings) {
                if (charging) {
                    hex = this._settings.colorCharging();
                } else if (pct <= this._settings.thresholdCritical()) {
                    hex = this._settings.colorCritical();
                } else if (pct <= this._settings.thresholdLow()) {
                    hex = this._settings.colorLow();
                } else {
                    hex = this._settings.colorHealthy();
                }
            }
            
            const s = hex.replace('#', '');
            if (s.length === 6) {
                const r = parseInt(s.substring(0, 2), 16) / 255;
                const g = parseInt(s.substring(2, 4), 16) / 255;
                const b = parseInt(s.substring(4, 6), 16) / 255;
                return { r, g, b };
            }
            return { r: 1.0, g: 1.0, b: 1.0 };
        }

        private _estimateRemaining(pct: number, charging: boolean): string {
            if (charging) return 'Charging…';
            let crit = 15;
            let low = 30;
            if (this._settings) {
                crit = this._settings.thresholdCritical();
                low = this._settings.thresholdLow();
            }
            if (pct <= crit) return 'About 4 hours left';
            if (pct <= low) return 'About 1 day left';
            return 'About 3–5 days left';
        }

        private _buildEasySwitch() {
            this._hostSection = new St.BoxLayout({
                orientation: Clutter.Orientation.VERTICAL,
                style_class: 'oxidemx-section',
                x_expand: true,
            });

            const label = new St.Label({
                text: 'Easy-Switch',
                style_class: 'oxidemx-section-title',
            });
            this._hostSection.add_child(label);

            this._hostBox = new St.BoxLayout({
                orientation: Clutter.Orientation.HORIZONTAL,
                style_class: 'oxidemx-segmented',
                x_expand: true,
            });
            this._hostSection.add_child(this._hostBox);

            this._rebuildHostButtons();
            this._content.add_child(this._hostSection);
        }

        private _rebuildHostButtons() {
            if (!this._hostBox) return;
            this._hostBox.destroy_all_children();
            this._hostButtons = [];

            const hostCount = Math.max(3, this._easySwitchCount);

            for (let i = 0; i < hostCount; i++) {
                let name = this._hostNames[i] || `CH ${i + 1}`;
                
                const btn = new St.Button({
                    label: name,
                    style_class: i === this._easySwitchCurrent ? 'oxidemx-segment oxidemx-segment-active' : 'oxidemx-segment',
                    x_expand: true,
                    can_focus: true,
                });

                btn.connect('clicked', () => {
                    if (this._proxy) {
                        this._proxy.SetHostAsync(i, (res, err) => {
                            if (err) log(`[oxidemx-popup] SetHost failed: ${err}`);
                            this.refreshState();
                        });
                    }
                });

                this._hostBox.add_child(btn);
                this._hostButtons.push(btn);
            }
        }

        private _buildQuickToggles(popupCfg: any) {
            this._togglesSection = new St.BoxLayout({
                orientation: Clutter.Orientation.VERTICAL,
                style_class: 'oxidemx-section',
                x_expand: true,
            });

            const label = new St.Label({
                text: 'Quick toggles',
                style_class: 'oxidemx-section-title',
            });
            this._togglesSection.add_child(label);

            const list = popupCfg.mode === 'power' ? popupCfg.power_toggles : popupCfg.simple_toggles;
            
            for (const id of list) {
                const entry = Object.prototype.hasOwnProperty.call(QUICK_TOGGLE_CATALOG, id) ? QUICK_TOGGLE_CATALOG[id] : null;
                if (!entry || !entry.label) continue;

                let is_on = false;
                if (id === 'gaming') is_on = this._gamingMode;
                else if (id === 'haptics') is_on = this._hapticsEnabled;
                else if (id === 'smart') is_on = this._smartShiftEnabled;

                const row = new St.Button({
                    style_class: 'oxidemx-optrow-header',
                    x_expand: true,
                    can_focus: true,
                });

                const box = new St.BoxLayout({
                    orientation: Clutter.Orientation.HORIZONTAL,
                    x_expand: true,
                });

                const icon = new St.Icon({
                    icon_name: entry.icon,
                    icon_size: 14,
                    style_class: 'oxidemx-row-icon',
                    y_align: Clutter.ActorAlign.CENTER,
                });
                
                const title = new St.Label({
                    text: entry.label || '',
                    style_class: 'oxidemx-row-title',
                    x_expand: true,
                    y_align: Clutter.ActorAlign.CENTER,
                });

                const valLabel = new St.Label({
                    text: is_on ? 'On' : 'Off',
                    style_class: is_on ? 'oxidemx-optrow-value oxidemx-optrow-value-on' : 'oxidemx-optrow-value',
                    y_align: Clutter.ActorAlign.CENTER,
                });

                box.add_child(icon);
                box.add_child(title);
                box.add_child(valLabel);
                row.set_child(box);

                row.connect('clicked', () => {
                    const nextValue = !is_on;
                    if (!this._proxy) return;

                    if (id === 'gaming') {
                        this._proxy.SetGamingModeAsync(nextValue, (res, err) => {
                            if (err) log(`[oxidemx-popup] SetGamingMode failed: ${err}`);
                            this.refreshState();
                        });
                    } else if (id === 'haptics') {
                        this._proxy.SetHapticsEnabledAsync(nextValue, (res, err) => {
                            if (err) log(`[oxidemx-popup] SetHapticsEnabled failed: ${err}`);
                            this.refreshState();
                        });
                    } else if (id === 'smart') {
                        this._proxy.SetSmartShiftAsync(nextValue, 30, (res, err) => {
                            if (err) log(`[oxidemx-popup] SetSmartShift failed: ${err}`);
                            this.refreshState();
                        });
                    } else {
                        // Warn placeholder to match popup-rs warning logs
                        log(`[oxidemx-popup] action not yet wired to daemon: ${id}`);
                    }
                });

                this._togglesSection.add_child(row);
            }

            this._content.add_child(this._togglesSection);
        }

        private _buildQuickSliders(popupCfg: any) {
            this._slidersSection = new St.BoxLayout({
                orientation: Clutter.Orientation.VERTICAL,
                style_class: 'oxidemx-section',
                x_expand: true,
            });

            const label = new St.Label({
                text: 'Quick sliders',
                style_class: 'oxidemx-section-title',
            });
            this._slidersSection.add_child(label);

            for (const id of popupCfg.power_sliders) {
                const entry = Object.prototype.hasOwnProperty.call(QUICK_SLIDER_CATALOG, id) ? QUICK_SLIDER_CATALOG[id] : null;
                if (!entry || !entry.label) continue;

                if (id === 'dpi') {
                    const sliderRow = new St.BoxLayout({
                        orientation: Clutter.Orientation.VERTICAL,
                        style_class: 'oxidemx-slider-row',
                        x_expand: true,
                    });

                    const labelBox = new St.BoxLayout({
                        orientation: Clutter.Orientation.HORIZONTAL,
                        x_expand: true,
                    });
                    
                    labelBox.add_child(new St.Label({
                        text: entry.label || '',
                        style_class: 'oxidemx-row-title',
                        x_expand: true,
                    }));

                    const valueLabel = new St.Label({
                        text: `${this._dpi} dpi`,
                        style_class: 'oxidemx-optrow-value',
                    });
                    labelBox.add_child(valueLabel);
                    sliderRow.add_child(labelBox);

                    // Map DPI: [200, 6400] -> [0.0, 1.0]
                    const initialVal = (this._dpi - 200) / (6400 - 200);
                    const slider = new Slider.Slider(initialVal);
                    slider.connect('value-changed', (s: any) => {
                        const raw = s.value * (6400 - 200) + 200;
                        const dpi = Math.round(raw / 100) * 100;
                        valueLabel.text = `${dpi} dpi`;
                    });

                    // Trigger DPI update on release
                    slider.connect('drag-end', (s: any) => {
                        const raw = s.value * (6400 - 200) + 200;
                        const dpi = Math.round(raw / 100) * 100;
                        if (this._proxy) {
                            this._proxy.SetDpiAsync(dpi, (res, err) => {
                                if (err) log(`[oxidemx-popup] SetDpi failed: ${err}`);
                            });
                        }
                    });

                    sliderRow.add_child(slider);
                    this._slidersSection.add_child(sliderRow);
                } else {
                    // Other sliders are placeholders (e.g. scroll sensitivity, haptic intensity, pointer acceleration)
                    const sliderRow = new St.BoxLayout({
                        orientation: Clutter.Orientation.VERTICAL,
                        style_class: 'oxidemx-slider-row',
                        x_expand: true,
                    });

                    const labelBox = new St.BoxLayout({
                        orientation: Clutter.Orientation.HORIZONTAL,
                        x_expand: true,
                    });

                    labelBox.add_child(new St.Label({
                        text: entry.label || '',
                        style_class: 'oxidemx-row-title',
                        x_expand: true,
                    }));

                    const valueLabel = new St.Label({
                        text: id === 'scroll' ? '5' : (id === 'haptic_i' ? 'Medium' : '0.00'),
                        style_class: 'oxidemx-optrow-value',
                    });
                    labelBox.add_child(valueLabel);
                    sliderRow.add_child(labelBox);

                    const slider = new Slider.Slider(0.5);
                    slider.connect('value-changed', (s: any) => {
                        if (id === 'scroll') {
                            const val = Math.round(s.value * 9 + 1);
                            valueLabel.text = `${val}`;
                        } else if (id === 'haptic_i') {
                            const val = Math.round(s.value * 5);
                            const labels = ['Off', 'Light', 'Soft', 'Medium', 'Firm', 'Strong'];
                            valueLabel.text = labels[val] || 'Medium';
                        } else if (id === 'accel') {
                            const val = (s.value * 2.0) - 1.0;
                            valueLabel.text = `${val.toFixed(2)}`;
                        }
                    });

                    slider.connect('drag-end', () => {
                        log(`[oxidemx-popup] slider not yet wired to daemon: ${id}`);
                    });

                    sliderRow.add_child(slider);
                    this._slidersSection.add_child(sliderRow);
                }
            }

            this._content.add_child(this._slidersSection);
        }

        private _buildFooter() {
            const footer = new St.BoxLayout({
                orientation: Clutter.Orientation.HORIZONTAL,
                style_class: 'oxidemx-footer',
                x_expand: true,
            });

            const settingsBtn = new St.Button({
                label: 'Settings',
                style_class: 'oxidemx-footer-btn',
            });

            settingsBtn.connect('clicked', () => {
                // Launch settings app and close menu
                try {
                    Gio.Subprocess.new(['oxidemx-settings'], Gio.SubprocessFlags.NONE);
                } catch (e) {
                    log(`[oxidemx-popup] Failed to spawn oxidemx-settings: ${e}`);
                }
                
                // Find parent PopupMenu and close it
                let parent: any = this.get_parent();
                while (parent && !(parent instanceof PopupMenu.PopupMenu)) {
                    parent = parent.get_parent();
                }
                if (parent) {
                    (parent as PopupMenu.PopupMenu).close(BoxPointer.PopupAnimation.FADE);
                }
            });

            const versionLabel = new St.Label({
                text: 'v0.3.2', // matches current package version
                style_class: 'oxidemx-footer-version',
                x_expand: true,
                x_align: Clutter.ActorAlign.END,
                y_align: Clutter.ActorAlign.CENTER,
            });

            footer.add_child(settingsBtn);
            footer.add_child(versionLabel);
            this._content.add_child(footer);
        }

        /**
         * Re-queries the daemon for all settings and refreshes the popup content.
         */
        refreshState() {
            if (!this._proxy) return;

            // 1. Get Battery & Connection State
            this._proxy.GetActiveDeviceStateAsync((res, err) => {
                if (err || !res) return;
                const [battery, charging, connection, device_name] = res;
                this._batteryPct = battery;
                this._charging = charging;
                this._deviceName = device_name || 'MX Master';
                this._connectionType = connection;

                // Update UI elements
                if (this._deviceLabel) this._deviceLabel.text = this._deviceName;
                if (this._connectionLabel) this._connectionLabel.text = connection ? `● ${connection}` : '○ Disconnected';
                if (this._batteryPercentLabel) this._batteryPercentLabel.text = `${this._batteryPct}%`;
                if (this._batteryTimeLabel) this._batteryTimeLabel.text = this._estimateRemaining(this._batteryPct, this._charging);
                if (this._batteryRingArea) this._batteryRingArea.queue_repaint();
            });

            // 2. Get Easy-Switch details
            this._proxy.GetEasySwitchInfoAsync((infoRes, infoErr) => {
                if (infoErr || !infoRes) return;
                const [current, count] = infoRes;
                this._easySwitchCurrent = current;
                this._easySwitchCount = count;

                this._proxy!.GetHostNamesAsync((namesRes, namesErr) => {
                    if (!namesErr && namesRes) {
                        this._hostNames = namesRes;
                    }
                    this._rebuildHostButtons();
                });
            });

            // 3. Get DPI
            this._proxy.GetDpiAsync((res, err) => {
                if (!err && res !== null) {
                    this._dpi = res;
                    this._buildUI(); // Full UI rebuild to update initial slider positions and toggles
                }
            });

            // 4. Get Gaming Mode
            this._proxy.GetGamingModeAsync((res, err) => {
                if (!err && res !== null) {
                    this._gamingMode = res;
                }
            });

            // 5. Get SmartShift
            this._proxy.GetSmartShiftAsync((res, err) => {
                if (!err && res) {
                    const [enabled, threshold] = res;
                    this._smartShiftEnabled = enabled;
                    this._smartShiftThreshold = threshold;
                }
            });
        }
    }
);
