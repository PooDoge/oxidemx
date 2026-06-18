//! Design tokens for the AI chat (from the P0 visual-system spec).
//! Spacing, radii, elevation, and the type ramp as named constants so
//! every view consumes the same scale. Colors are NOT here — they stay
//! theme-resolved through `Kit`.

#![allow(dead_code)]

use iced::{Color, Shadow, Vector};

// ── Spacing (4px base, +2/+6 half-steps) ──────────────────────────────
pub const S0_5: f32 = 2.0;
pub const S1: f32 = 4.0;
pub const S1_5: f32 = 6.0;
pub const S2: f32 = 8.0;
pub const S2_5: f32 = 10.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S5: f32 = 20.0;
pub const S6: f32 = 24.0;

// ── Corner radii (by role) ────────────────────────────────────────────
pub const R_CONTROL: f32 = 6.0; // input inner, code block
pub const R_BUTTON: f32 = 9.0; // icon buttons
pub const R_CARD: f32 = 12.0; // agent / tool cards
pub const R_PANEL: f32 = 16.0; // popovers, dialogs
pub const R_PILL: f32 = 999.0; // chips, status dots

// ── Type ramp (text().size()) ─────────────────────────────────────────
pub const T_DISPLAY: f32 = 18.0; // 600
pub const T_HEADING: f32 = 16.0; // 600
pub const T_TITLE: f32 = 14.5; // 600
pub const T_BODY: f32 = 13.0;
pub const T_BODY_SM: f32 = 12.0;
pub const T_LABEL: f32 = 11.5; // 500
pub const T_META: f32 = 11.0;
pub const T_CAPTION: f32 = 10.5;
pub const T_MICRO: f32 = 10.0; // mono

// ── Elevation (single iced::Shadow per tier; no spread/inset) ─────────
/// e1 — cards (agent + tool).
pub fn e1() -> Shadow {
    Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
        offset: Vector::new(0.0, 2.0),
        blur_radius: 8.0,
    }
}

/// e2 — overlays (palettes, popovers, dock).
pub fn e2() -> Shadow {
    Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
        offset: Vector::new(0.0, 12.0),
        blur_radius: 32.0,
    }
}

/// e3 — modals (lightbox, dialogs).
pub fn e3() -> Shadow {
    Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, 0.6),
        offset: Vector::new(0.0, 24.0),
        blur_radius: 60.0,
    }
}
