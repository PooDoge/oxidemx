/**
 * Placement manager: handles adding/removing the indicator button dynamically
 * to/from the correct panel(s) (standard top bar and/or Dash to Panel).
 *
 * Design notes:
 *   - Follows Dash to Panel enable/disable and panel recreation dynamically.
 *   - Supports multi-instance configurations (e.g. rendering on multiple monitor panels).
 *   - Allows hiding the visual indicator ('none' display-mode) while keeping the extension active.
 */

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import type { PanelTarget, DisplayMode, Position } from './settings.js';

declare const global: any;

const D2P_UUID = 'dash-to-panel@jderose9.github.com';

function getDashToPanel(): any {
    return (global as any).dashToPanel ?? null;
}

function isDashToPanelEnabled(): boolean {
    const mgr = Main.extensionManager;
    if (!mgr) return false;
    const ext = mgr.lookup(D2P_UUID);
    return ext !== undefined && ext.state === 1; // ExtensionState.ENABLED
}

export class IndicatorPlacement {
    private _uuid: string;
    private _createIndicator: () => any;
    private _configure: (indicator: any) => void;
    private _getParams: () => { panelTarget: PanelTarget; displayMode: DisplayMode; position: Position; positionIndex: number };

    private _indicators: any[] = [];
    private _destroyIds: Map<any, number> = new Map();
    private _dashToPanel: any = null;
    private _panelsSignalId = 0;
    private _extManagerSignalId = 0;

    constructor(
        uuid: string,
        createIndicator: () => any,
        configure: (indicator: any) => void,
        getParams: () => { panelTarget: PanelTarget; displayMode: DisplayMode; position: Position; positionIndex: number }
    ) {
        this._uuid = uuid;
        this._createIndicator = createIndicator;
        this._configure = configure;
        this._getParams = getParams;

        // Watch extensionManager state to follow D2P enablement.
        this._extManagerSignalId = (Main.extensionManager as any).connect(
            'extension-state-changed', (_mgr: unknown, ext: any) => {
                if (ext?.uuid === D2P_UUID) {
                    const p = this._getParams();
                    this.place(p.panelTarget, p.displayMode, p.position, p.positionIndex);
                }
            });
    }

    get indicators(): any[] {
        return this._indicators;
    }

    place(
        panelTarget: PanelTarget,
        displayMode: DisplayMode,
        position: Position,
        positionIndex: number
    ): void {
        this._removeIndicators();

        if (displayMode === 'none') {
            this._trackDashToPanel(getDashToPanel());
            return;
        }

        const dashToPanel = getDashToPanel();
        const d2pActive = isDashToPanelEnabled() && dashToPanel;
        const panels = dashToPanel?.panels;

        const useTopBar = panelTarget === 'topbar' || panelTarget === 'both' || (panelTarget === 'auto' && !d2pActive);
        const useDtp = d2pActive && (panelTarget === 'dtp' || panelTarget === 'both' || panelTarget === 'auto');

        if (useTopBar) {
            this._addIndicator(Main.panel, this._uuid, position, positionIndex);
        }

        if (useDtp && Array.isArray(panels) && panels.length > 0) {
            panels.forEach((d2pPanel: any, index: number) => {
                this._addIndicator(d2pPanel?.panel, `${this._uuid}-${index}`, position, positionIndex);
            });
        }

        this._trackDashToPanel(dashToPanel);
    }

    destroy(): void {
        if (this._extManagerSignalId) {
            (Main.extensionManager as any).disconnect(this._extManagerSignalId);
            this._extManagerSignalId = 0;
        }
        this._untrackDashToPanel();
        this._removeIndicators();
    }

    private _addIndicator(panel: any, role: string, position: Position, positionIndex: number): void {
        if (!panel || typeof panel.addToStatusArea !== 'function')
            return;

        const indicator = this._createIndicator();
        panel.addToStatusArea(role, indicator, positionIndex, position);
        this._configure(indicator);

        const destroyId = (indicator as any).connect('destroy', () => {
            this._indicators = this._indicators.filter(i => i !== indicator);
            this._destroyIds.delete(indicator);
        });
        this._destroyIds.set(indicator, destroyId);

        this._indicators.push(indicator);
    }

    private _removeIndicators(): void {
        for (const indicator of this._indicators) {
            const destroyId = this._destroyIds.get(indicator);
            if (destroyId !== undefined) {
                try {
                    (indicator as any).disconnect(destroyId);
                } catch (_e) {}
            }
            try {
                indicator.destroy();
            } catch (_e) {}
        }
        this._indicators = [];
        this._destroyIds.clear();
    }

    private _trackDashToPanel(dashToPanel: any): void {
        if (dashToPanel === this._dashToPanel)
            return;
        this._untrackDashToPanel();
        if (dashToPanel) {
            this._dashToPanel = dashToPanel;
            this._panelsSignalId = dashToPanel.connect('panels-created', () => {
                const p = this._getParams();
                this.place(p.panelTarget, p.displayMode, p.position, p.positionIndex);
            });
        }
    }

    private _untrackDashToPanel(): void {
        if (this._panelsSignalId && this._dashToPanel) {
            try {
                this._dashToPanel.disconnect(this._panelsSignalId);
            } catch (_e) {}
        }
        this._panelsSignalId = 0;
        this._dashToPanel = null;
    }
}
