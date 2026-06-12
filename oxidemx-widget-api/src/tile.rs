//! The `tile()` scene builder: a high-level template that reproduces the
//! built-in wedge typography from `overlay-rs/src/render/slices/widgets.rs`
//! `draw_widget_wedge`:
//!
//! - 18pt bold value (big number / main reading) — y=-14 from icon anchor
//! - optional sparkline below the value
//! - 8.5pt regular sublabel (condition / details)
//! - 8pt semibold UPPERCASE label (widget name)
//!
//! Colors default to `Color::Palette("accent")` unless `.color()` is given.

use oxidemx_widget_proto::{Color, Prim, Scene, TextAlign, TextWeight, WedgeGeom};

/// Start building a tile scene. Call `.into_scene(geom)` to finish.
pub fn tile() -> TileBuilder {
    TileBuilder {
        value: None,
        sublabel: None,
        label: None,
        sparkline: None,
        color: None,
    }
}

pub struct TileBuilder {
    value: Option<String>,
    sublabel: Option<String>,
    label: Option<String>,
    sparkline: Option<Vec<f32>>,
    color: Option<String>,
}

impl TileBuilder {
    /// The main (large) value string — e.g. "14°" or "72%".
    pub fn value(mut self, s: impl Into<String>) -> Self {
        self.value = Some(s.into());
        self
    }

    /// Small line below the value / sparkline — e.g. "Partly cloudy".
    pub fn sublabel(mut self, s: impl Into<String>) -> Self {
        self.sublabel = Some(s.into());
        self
    }

    /// Uppercase widget label at the bottom of the stack — e.g. "WEATHER".
    /// `.into_scene()` calls `.to_uppercase()` automatically.
    pub fn label(mut self, s: impl Into<String>) -> Self {
        self.label = Some(s.into());
        self
    }

    /// Optional sparkline data (normalised 0.0..=1.0 samples).
    pub fn sparkline(mut self, pts: &[f32]) -> Self {
        self.sparkline = Some(pts.to_vec());
        self
    }

    /// Override the palette color key (default: `"accent"`).
    pub fn color(mut self, key: impl Into<String>) -> Self {
        self.color = Some(key.into());
        self
    }

    /// Assemble the `Scene`. Prim order:
    /// `[value Text, Sparkline?, sublabel Text?, label Text?]`.
    ///
    /// Layout mirrors `draw_widget_wedge` (icon_pos.y as origin at y=0):
    ///
    /// ```text
    /// y = -14  → value (18pt Bold)
    /// y_cursor → -14 + 13 = -1  (after value row)
    ///   if sparkline: Sparkline at y=-1, h=10 → y_cursor = -1+10+3 = 12
    ///   sublabel at y_cursor + 4
    ///   y_cursor += 11
    ///   label at y_cursor + 6
    /// ```
    pub fn into_scene(self, _geom: WedgeGeom) -> Scene {
        let palette_key = self.color.unwrap_or_else(|| "accent".into());
        let accent = Color::Palette(palette_key);
        let mut prims = Vec::new();

        // --- value (18pt Bold, centered) ------------------------------------
        if let Some(val) = self.value {
            prims.push(Prim::Text {
                x: 0.0,
                y: -14.0,
                content: val,
                size: 18.0,
                color: accent.clone(),
                weight: TextWeight::Bold,
                align: TextAlign::Center,
            });
        }

        // y_cursor starts after the value row
        let mut y_cursor: f32 = -1.0; // -14 + 13

        // --- sparkline (optional) -------------------------------------------
        if let Some(points) = self.sparkline {
            prims.push(Prim::Sparkline {
                x: -20.0,
                y: y_cursor,
                w: 40.0,
                h: 10.0,
                points,
                color: accent.clone(),
            });
            y_cursor += 10.0 + 3.0; // h + gap
        }

        // --- sublabel (8.5pt Regular) ----------------------------------------
        if let Some(sub) = self.sublabel {
            prims.push(Prim::Text {
                x: 0.0,
                y: y_cursor + 4.0,
                content: sub,
                size: 8.5,
                color: accent.clone(),
                weight: TextWeight::Regular,
                align: TextAlign::Center,
            });
            y_cursor += 11.0;
        }

        // --- label (8pt Semibold, uppercased) --------------------------------
        if let Some(lbl) = self.label {
            prims.push(Prim::Text {
                x: 0.0,
                y: y_cursor + 6.0,
                content: lbl.to_uppercase(),
                size: 8.0,
                color: accent,
                weight: TextWeight::Semibold,
                align: TextAlign::Center,
            });
        }

        Scene { prims }
    }
}
