/**
 * Pure logic helpers shared by extension.ts and prefs.ts.
 * No GJS imports — safe to unit-test under plain node if we ever wire jest in.
 *
 * Exports:
 *   Band         — union of the four battery state bands.
 *   Thresholds   — critical / low threshold percentages.
 *   BandColors   — hex colour per band.
 *   bandFor      — classify a battery reading into a Band.
 *   colorForBand — resolve a Band to its configured colour string.
 *   clampPct     — clamp an arbitrary number to [0, 100] integer.
 *   formatPctLabel — produce a display label like "30%" or "⚡30%".
 */

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** The four mutually exclusive battery-state bands. */
export type Band = 'critical' | 'low' | 'healthy' | 'charging';

/** Upper bounds (inclusive, as percentages 1–99) for the critical and low bands. */
export interface Thresholds {
    critical: number;
    low: number;
}

/** Per-band display colours as #RRGGBB hex strings. */
export interface BandColors {
    critical: string;
    low: string;
    healthy: string;
    charging: string;
}

// ---------------------------------------------------------------------------
// Pure functions
// ---------------------------------------------------------------------------

/**
 * Classify a battery reading into a Band.
 *
 * Order of precedence:
 *   1. charging → always 'charging', regardless of pct.
 *   2. pct <= thresholds.critical → 'critical'.
 *   3. pct <= thresholds.low     → 'low'.
 *   4. otherwise                 → 'healthy'.
 */
export function bandFor(pct: number, charging: boolean, thresholds: Thresholds): Band {
    if (charging) return 'charging';
    if (pct <= thresholds.critical) return 'critical';
    if (pct <= thresholds.low) return 'low';
    return 'healthy';
}

/**
 * Resolve a Band to its configured colour string.
 * Returns the hex value stored in `colors` for the given band — a straight
 * property lookup; no computation.
 */
export function colorForBand(band: Band, colors: BandColors): string {
    return colors[band];
}

/**
 * Clamp an arbitrary number to the [0, 100] integer range.
 * Input is rounded before clamping so fractional readings snap to the
 * nearest integer rather than truncating toward zero.
 */
export function clampPct(pct: number): number {
    return Math.max(0, Math.min(100, Math.round(pct)));
}

/**
 * Build a display label for the given battery percentage.
 *
 * Examples:
 *   formatPctLabel(30, false) → "30%"
 *   formatPctLabel(30, true)  → "⚡30%"
 *
 * The lightning-bolt prefix (U+26A1) follows the CSS/UI convention of a
 * single leading character so the indicator stays narrow.
 */
export function formatPctLabel(pct: number, charging: boolean): string {
    const clamped = clampPct(pct);
    return charging ? `⚡${clamped}%` : `${clamped}%`;
}
