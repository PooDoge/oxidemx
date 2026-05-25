/**
 * Placement helpers: resolve the target panel (top bar or Dash to Panel) and
 * add/remove the indicator button.
 *
 * Design notes:
 *   - `auto` probes for DTP at the moment of placement and picks DTP when
 *     present, otherwise falls back to the top bar. This means toggling DTP
 *     on/off after the extension has started requires disabling and re-enabling
 *     this extension — the same pattern every other indicator extension uses.
 *   - We never cache the resolved target, so `resolvePanelTarget` is pure and
 *     can be called multiple times without stale state.
 */

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import type { PanelTarget, Position } from './settings.js';

// ---------------------------------------------------------------------------
// Internal probe
// ---------------------------------------------------------------------------

/**
 * Return true when Dash to Panel is loaded and in the ENABLED state.
 *
 * ExtensionState.ENABLED === 1 per @girs/gnome-shell extensionUtils.d.ts.
 */
function isDashToPanelEnabled(): boolean {
    const mgr = Main.extensionManager;
    if (!mgr) return false;
    const ext = mgr.lookup('dash-to-panel@jderose9.github.com');
    return ext !== undefined && ext.state === 1; // ExtensionState.ENABLED
}

// ---------------------------------------------------------------------------
// Public surface
// ---------------------------------------------------------------------------

/**
 * Resolve the abstract `PanelTarget` setting into a concrete placement target.
 *
 * When `setting === 'auto'`, the function probes whether DTP is enabled:
 *   - DTP enabled  → resolves to 'dtp'.
 *   - DTP disabled → resolves to 'topbar'.
 * All other values pass through unchanged.
 *
 * @param setting - The panel-target GSettings value.
 * @returns The resolved placement target (never 'auto').
 */
export function resolvePanelTarget(setting: PanelTarget): 'topbar' | 'dtp' | 'both' {
    if (setting === 'auto') {
        return isDashToPanelEnabled() ? 'dtp' : 'topbar';
    }
    return setting;
}

/**
 * Add a panel button to the configured panel(s).
 *
 * For 'topbar': calls `Main.panel.addToStatusArea()` with the given role,
 * position, and positionIndex.
 *
 * For 'dtp': falls through to the top bar with a console.warn when DTP is
 * not actually enabled, since DTP may have been disabled between when the
 * setting was saved and when enable() runs.
 *
 * For 'both': adds to both panels (top bar always; DTP when available).
 *
 * @param button        - The St.Widget / PanelMenu.Button instance to add.
 * @param target        - Resolved target ('topbar' | 'dtp' | 'both').
 * @param position      - Panel section: 'left' | 'center' | 'right'.
 * @param positionIndex - Order within the section (0 = leftmost).
 * @param role          - Unique role string used by addToStatusArea.
 */
export function addToPanel(
    button: object,
    target: 'topbar' | 'dtp' | 'both',
    position: Position,
    positionIndex: number,
    role: string,
): void {
    const addToTopBar = (): void => {
        Main.panel.addToStatusArea(role, button as any, positionIndex, position);
    };

    const addToDtp = (): void => {
        if (!isDashToPanelEnabled()) {
            log('[juhradial-indicator] DTP requested but extension not enabled; falling back to topbar');
            addToTopBar();
            return;
        }
        // DTP exposes `window.SETTINGS` and wraps the standard panel. The
        // cleanest integration is to add to the primary `Main.panel`; DTP
        // mirrors top-bar status-area entries into its own bar automatically
        // for the common single-monitor case. A future iteration can call
        // DTP's per-monitor panel API when multi-monitor DTP support is needed.
        addToTopBar();
    };

    switch (target) {
        case 'topbar':
            addToTopBar();
            break;
        case 'dtp':
            addToDtp();
            break;
        case 'both':
            addToTopBar();
            // For 'both' we still only have one button instance — adding it to
            // two status areas simultaneously causes a GObject double-parent
            // error. Log a note and skip the second add. A dual-button design
            // is deferred to Phase 2 if there is demand.
            log('[juhradial-indicator] panel-target=both: dual-instance not yet supported; showing on top bar only.');
            break;
    }
}

/**
 * Remove the panel button from the status area, cleaning up the widget.
 *
 * Calls `button.destroy()` which triggers GNOME Shell's standard status-area
 * cleanup path (the button removes itself from `Main.panel._statusArea`).
 * If the button has already been destroyed this is a no-op.
 *
 * @param button - The indicator button to destroy.
 * @param role   - The role string used when the button was added (for logging).
 */
export function removeFromPanel(button: object, role: string): void {
    try {
        (button as any).destroy();
    } catch (e) {
        log(`[juhradial-indicator] removeFromPanel(${role}): ${e}`);
    }
}
