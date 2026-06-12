//! Retained display list returned by a widget's `render()`.
//! Coordinates are wedge-local: origin at the icon anchor, +y down.
//!
//! WIRE FORMAT: postcard encodes enum variants by declaration index —
//! `Prim`, `Color`, `PathOp`, `TextWeight`, `TextAlign` are APPEND-ONLY
//! and must never be reordered within an `API_VERSION` (see event.rs for
//! the full evolution rules).

use serde::{Deserialize, Serialize};

/// Geometry of the slot the widget is rendering into, plus its hover
/// progress (0.0 = idle, 1.0 = fully hovered). Sent with every render call.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WedgeGeom {
    pub width: f32,
    pub height: f32,
    pub inner_radius: f32,
    pub outer_radius: f32,
    pub angle_start: f32,
    pub angle_end: f32,
    pub hovered: f32,
}

/// Encoded-scene byte cap (spec §8): one strike if exceeded.
pub const MAX_SCENE_BYTES: usize = 64 * 1024;
/// Primitive-count cap, counted recursively through groups.
pub const MAX_SCENE_PRIMS: usize = 2048;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Color {
    /// Key into the user's active theme palette ("yellow", "teal", …).
    Palette(String),
    Rgba(u8, u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextWeight { Regular, Medium, Semibold, Bold }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign { Left, Center, Right }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Prim {
    Arc {
        cx: f32, cy: f32, radius: f32,
        start_angle: f32, end_angle: f32,
        stroke: Option<Stroke>, fill: Option<Color>,
    },
    Path {
        ops: Vec<PathOp>,
        stroke: Option<Stroke>, fill: Option<Color>,
    },
    Text {
        x: f32, y: f32, content: String, size: f32,
        color: Color, weight: TextWeight, align: TextAlign,
    },
    Sparkline {
        x: f32, y: f32, w: f32, h: f32,
        /// Normalised samples in 0.0..=1.0.
        points: Vec<f32>, color: Color,
    },
    /// Static image from the widget bundle's `assets/` dir; the host
    /// decodes and caches it.
    Image { x: f32, y: f32, w: f32, h: f32, asset: String },
    /// Translated/scaled subtree.
    Group { dx: f32, dy: f32, scale: f32, children: Vec<Prim> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Scene {
    pub prims: Vec<Prim>,
}

impl Scene {
    /// Recursive primitive count (groups count as 1 + children).
    pub fn prim_count(&self) -> usize {
        fn count(prims: &[Prim]) -> usize {
            prims.iter().map(|p| match p {
                Prim::Group { children, .. } => 1 + count(children),
                _ => 1,
            }).sum()
        }
        count(&self.prims)
    }

    /// Enforce the spec §8 caps. The host treats `Err` as a strike.
    pub fn validate(&self) -> Result<(), String> {
        let n = self.prim_count();
        if n > MAX_SCENE_PRIMS {
            return Err(format!("scene has {n} prims (max {MAX_SCENE_PRIMS})"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scene() -> Scene {
        Scene {
            prims: vec![
                Prim::Text {
                    x: 0.0, y: -6.0, content: "14°".into(), size: 18.0,
                    color: Color::Palette("yellow".into()),
                    weight: TextWeight::Bold, align: TextAlign::Center,
                },
                Prim::Sparkline {
                    x: -21.0, y: 4.0, w: 42.0, h: 11.0,
                    points: vec![0.3, 0.5, 0.4, 0.9],
                    color: Color::Rgba(255, 171, 107, 255),
                },
                Prim::Path {
                    ops: vec![PathOp::MoveTo(0.0, 0.0), PathOp::LineTo(4.0, 4.0), PathOp::Close],
                    stroke: Some(Stroke { color: Color::Palette("teal".into()), width: 1.5 }),
                    fill: None,
                },
                Prim::Group {
                    dx: 2.0, dy: 2.0, scale: 0.5,
                    children: vec![Prim::Image { x: 0.0, y: 0.0, w: 16.0, h: 16.0, asset: "assets/sun.png".into() }],
                },
            ],
        }
    }

    #[test]
    fn scene_postcard_round_trip() {
        let scene = sample_scene();
        let bytes = postcard::to_allocvec(&scene).unwrap();
        let back: Scene = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(scene, back);
    }

    #[test]
    fn scene_caps_are_enforced() {
        let mut scene = Scene { prims: vec![] };
        for _ in 0..(MAX_SCENE_PRIMS + 1) {
            scene.prims.push(Prim::Text {
                x: 0.0, y: 0.0, content: "x".into(), size: 8.0,
                color: Color::Rgba(0, 0, 0, 255),
                weight: TextWeight::Regular, align: TextAlign::Left,
            });
        }
        assert!(scene.validate().is_err());
    }
}
