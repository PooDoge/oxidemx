// Aurora backdrop — renders behind the radial-menu canvas. A
// single full-screen triangle drives a fragment shader that
// produces a slowly-rotating conic gradient between three
// palette accents, faded radially so it doesn't bleed past the
// menu's halo or paint over the centre puck.

struct Uniforms {
    time: f32,
    intensity: f32,
    _pad0: vec2<f32>,
    accent: vec4<f32>,
    accent2: vec4<f32>,
    accent_dim: vec4<f32>,
    // Menu-element transform — see render/animation.rs::MenuXformRaw.
    // Translates / rotates / flips the aurora to track the canvas
    // when the user adds custom motion tracks to the menu element.
    // Identity (0/0/0/1/1/Y) when no tracks are set.
    menu_xform_translate: vec2<f32>,
    _pad_xform0: vec2<f32>,
    menu_xform_rotate: f32,
    menu_xform_flip_scale: f32,
    menu_xform_flip_axis: u32, // 0 = X, 1 = Y
    _pad_xform1: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

/// Apply the inverse of the menu transform to a UV coordinate so
/// the SHADER coordinate space follows the canvas's transform.
/// Without this, custom translate/rotate/flip tracks on the menu
/// element move the canvas wedges/icons but the aurora stays
/// pinned to the screen — visibly inconsistent.
///
/// Order matches `render::animation::apply_composed_transform`'s
/// inverse: un-translate first, then inverse-rotate, then inverse
/// flip-scale.
fn apply_menu_xform_inv(p: vec2<f32>) -> vec2<f32> {
    var q = p - u.menu_xform_translate;
    let c = cos(-u.menu_xform_rotate);
    let s = sin(-u.menu_xform_rotate);
    q = vec2<f32>(q.x * c - q.y * s, q.x * s + q.y * c);
    let denom = max(abs(u.menu_xform_flip_scale), 0.001);
    if u.menu_xform_flip_axis == 1u {
        // Y-axis flip → X scales, so undo by dividing X.
        q.x = q.x / denom;
    } else {
        q.y = q.y / denom;
    }
    return q;
}

// Single-triangle full-screen pass. Index 0/1/2 yields three
// vertices that form one triangle whose visible portion is
// exactly the viewport's [-1, 1]² rect:
//   i=0 → (-1, -1)
//   i=1 → ( 3, -1)
//   i=2 → (-1,  3)
// We also emit a `uv` varying that interpolates linearly with
// the clip-space position. Across the visible viewport, `uv`
// ranges from -1 to +1 on each axis — independent of HiDPI
// scale factor or where the widget sits in the framebuffer.
// (Doing this in the vertex stage is the only way to get
// viewport-relative coordinates without knowing the bounds; the
// fragment-stage `@builtin(position)` is in framebuffer pixels
// and would shift on any non-1× display.)
struct VOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32((i >> 1u) & 1u) * 4 - 1);
    var out: VOut;
    out.clip_pos = vec4<f32>(x, y, 0.0, 1.0);
    // Note: clip-space +Y is up; iced's canvas convention is
    // +Y down. The aurora is radially symmetric around the
    // centre, so the sign flip doesn't visually matter — and
    // the menu is also centred so a y-axis flip leaves the
    // result identical. We pass the raw clip xy through.
    out.uv = vec2<f32>(x, y);
    return out;
}

// Conic + radial blend. Three accent colours fade in/out at 120°
// intervals around the centre, with a slow rotation driven by
// `time`. Inner fade keeps the centre puck clean; outer fade
// matches the menu's halo so the aurora doesn't paint past the
// visible disc.
@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // `in.uv` is in [-1, 1]² across the visible viewport, so the
    // menu's centre is always at uv=(0,0) regardless of HiDPI
    // scale, window position, or framebuffer dimensions.
    //
    // Convert to canvas convention (+Y down) so the menu_xform
    // uniform — built from `translate_x_px / translate_y_px`
    // values that increase down/right in canvas space — can be
    // subtracted directly. Aurora is radially symmetric, so
    // flipping Y here doesn't change the output's look (it would
    // for direction-sensitive shaders).
    let p_canvas = vec2<f32>(in.uv.x, -in.uv.y);
    let p = apply_menu_xform_inv(p_canvas);

    let r = length(p);
    // Discard pixels well outside the menu disc — keeps the
    // aurora confined to the area the user actually sees + saves
    // a few ALU ops on edge fragments. 0.95 leaves a hair of
    // bleed for the halo.
    if r > 1.0 {
        discard;
    }

    let theta = atan2(p.y, p.x);
    // Slow clockwise rotation. 0.18 rad/s ≈ one full sweep every
    // ~35 s — visible motion without being distracting.
    let phase = theta + u.time * 0.18;

    // Three weights at 120° intervals → smooth conic blend.
    let w1 = 0.5 + 0.5 * cos(phase);
    let w2 = 0.5 + 0.5 * cos(phase + 2.0944);  // +120°
    let w3 = 0.5 + 0.5 * cos(phase + 4.18879); // +240°
    let wsum = max(w1 + w2 + w3, 1e-3);

    var blended =
        u.accent.rgb * (w1 / wsum)
        + u.accent2.rgb * (w2 / wsum)
        + u.accent_dim.rgb * (w3 / wsum);

    // Saturation boost: pull each pixel's colour away from its
    // own grey-equivalent so the swirl reads as vivid even
    // through the wedge fills layered on top of it. 0.4 is
    // tasteful — pure-saturated would be neon. Independent of
    // intensity so dialing intensity up doesn't over-saturate.
    let luma = dot(blended, vec3<f32>(0.299, 0.587, 0.114));
    blended = mix(vec3<f32>(luma), blended, 1.4);

    // Two-octave breathing: a slow ~6 s pulse on top of a
    // ~2 s shimmer. Together they keep the aurora alive without
    // ever quite repeating, which makes it feel less like a
    // pre-baked animation.
    let breathe =
        0.85 + 0.10 * sin(u.time * 1.05) + 0.05 * sin(u.time * 3.1);

    // Outer fade: smoothly reduce alpha as we approach the disc
    // edge so the aurora softens into the desktop / window edge
    // instead of cutting hard.
    let edge = 1.0 - smoothstep(0.55, 0.98, r);

    // Inner fade: don't overpaint the centre puck. The puck sits
    // at ~45 px in a 484 px window → ~0.19 of the half-extent;
    // we feather between 0.18 and 0.32 so the transition is
    // invisible to the user.
    let centre_fade = smoothstep(0.18, 0.32, r);

    // Final alpha — caller-provided intensity drives the master
    // multiplier. Cap at 1.0 so a user dialing the slider to
    // 100 % gets a bold glow; 60 % default still reads as
    // ambient. Old code clamped at 0.5 which was too timid.
    let alpha = u.intensity * edge * centre_fade * breathe;

    // Pre-multiplied alpha output to match iced's compositor.
    return vec4<f32>(blended * alpha, alpha);
}
