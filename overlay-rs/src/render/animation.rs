//! Frame-tick driver + easing curves + transform helpers.
//!
//! `glib::timeout_add_local(Duration::from_millis(16), ...)` for the
//! ~60 Hz frame tick. Easing curves match the legacy Python overlay so
//! the visual feel is preserved.

use iced::widget::canvas::Frame;
use iced::{Point, Radians, Vector};
use juhradial_shared::{Axis, ComposedTransform};

#[allow(dead_code)]
pub fn ease_out_back(t: f64, overshoot: f64) -> f64 {
    let t = t - 1.0;
    t * t * ((overshoot + 1.0) * t + overshoot) + 1.0
}

#[allow(dead_code)]
pub fn ease_out_quad(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(2)
}

/// Apply a [`ComposedTransform`]'s translate / rotate / flip to the
/// canvas frame in place. Identity transforms are a no-op so it's
/// cheap to call unconditionally on every element. Scale (uniform)
/// is *not* applied here — callers keep their existing
/// "scale-baked-into-radii" path so stroke widths and text sizes
/// stay pixel-perfect on the preset path.
///
/// Pivot order:
///   1. translate by (dx, dy) — outermost: shifts the element on
///      screen.
///   2. translate to centre — pivot to element centre.
///   3. rotate.
///   4. flip-scale on the user-picked axis.
///   5. translate back from centre.
///
/// Caller should wrap the affected drawing in `frame.with_save(|f|
/// { apply_composed_transform(f, centre, &t); ... })` if it wants
/// the transform to scope to a sub-region; otherwise the transform
/// persists for the rest of the frame.
pub fn apply_composed_transform(
    frame: &mut Frame,
    center: Point,
    t: &ComposedTransform,
) {
    if t.translate_x_px != 0.0 || t.translate_y_px != 0.0 {
        frame.translate(Vector::new(t.translate_x_px, t.translate_y_px));
    }
    let needs_pivot =
        t.rotate_rad != 0.0 || (t.flip_scale - 1.0).abs() > 1e-4;
    if !needs_pivot {
        return;
    }
    frame.translate(Vector::new(center.x, center.y));
    if t.rotate_rad != 0.0 {
        frame.rotate(Radians(t.rotate_rad));
    }
    if (t.flip_scale - 1.0).abs() > 1e-4 {
        let (sx, sy) = match t.flip_axis {
            Axis::X => (1.0, t.flip_scale),
            Axis::Y => (t.flip_scale, 1.0),
        };
        frame.scale_nonuniform(Vector::new(sx, sy));
    }
    frame.translate(Vector::new(-center.x, -center.y));
}

// TODO: a single `AnimationDriver` that owns the frame timer and a list
// of in-flight tweens (slice highlight progress, submenu pop-out
// progress, centre pulse, flash on click).

/// 32-byte uniform block that menu-tracking shaders embed when
/// they want to follow the menu's custom-track translate / rotate
/// / flip. Stored verbatim in WGSL as:
///
/// ```wgsl
/// struct MenuXform {
///     translate_norm: vec2<f32>,
///     _pad0: vec2<f32>,
///     rotate_rad: f32,
///     flip_scale: f32,
///     flip_axis: u32,    // 0 = X, 1 = Y
///     _pad1: f32,
/// };
/// ```
///
/// Scale is **deliberately not included**. The radial menu's
/// existing scale path already shrinks shader-fed radii (see
/// `app.rs::view` for the SDF, and the per-effect intensity *
/// menu_alpha pattern for the others). Adding scale here would
/// double-apply.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MenuXformRaw {
    pub translate_norm: [f32; 2],
    pub _pad0: [f32; 2],
    pub rotate_rad: f32,
    pub flip_scale: f32,
    pub flip_axis: u32,
    pub _pad1: f32,
}

impl MenuXformRaw {
    /// Reserved for shaders that want a no-op fallback when not
    /// passed a real transform. Currently every caller goes
    /// through `from_composed`, but keeping the constant means
    /// new shaders can opt in without recomputing the zeros.
    #[allow(dead_code)]
    pub const IDENTITY: MenuXformRaw = MenuXformRaw {
        translate_norm: [0.0; 2],
        _pad0: [0.0; 2],
        rotate_rad: 0.0,
        flip_scale: 1.0,
        flip_axis: 1, // Y
        _pad1: 0.0,
    };

    /// Build a uniform-ready transform from a logical-pixel-space
    /// `ComposedTransform`. `half_extent` is the menu window's
    /// half-size (typically `WINDOW_SIZE / 2.0` = 242 px) so the
    /// translate gets normalised into the same space the shader
    /// reads UVs in (clip-space `[-1, 1]²`, half-extent units).
    pub fn from_composed(
        transform: &juhradial_shared::ComposedTransform,
        half_extent: f32,
    ) -> Self {
        let flip_axis = match transform.flip_axis {
            juhradial_shared::Axis::X => 0u32,
            juhradial_shared::Axis::Y => 1u32,
        };
        MenuXformRaw {
            translate_norm: [
                transform.translate_x_px / half_extent,
                transform.translate_y_px / half_extent,
            ],
            _pad0: [0.0; 2],
            rotate_rad: transform.rotate_rad,
            flip_scale: transform.flip_scale,
            flip_axis,
            _pad1: 0.0,
        }
    }
}
