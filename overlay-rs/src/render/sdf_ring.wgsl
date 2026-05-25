// SDF wedge ring: draws the radial menu's annular wedges using a
// signed-distance-field analytic shader. Replaces the canvas's
// tessellated wedge fills + strokes + hover wash with one
// full-screen pass.
//
// Composition (back-to-front, OVER blend):
//   1. Wedge fill: `surface0` slot colour, masked by the
//      smoothstep'd SDF interior.
//   2. Hover wash: accent colour at `hover_wash_peak * h` alpha,
//      same fill mask. Tints the hovered wedge toward accent
//      without fully washing the surface0 substrate out.
//   3. Stroke band: `surface2 → accent` interpolated by `h`,
//      width `stroke_half_px * fwidth(d)` so it stays a stable
//      number of pixels at any HiDPI / fractional scale. Alpha
//      ramps `60/255 → 150/255` matching the canvas formula.
//
// All three layers respect `intensity` (the user's slider) and
// `base_alpha` (the existing menu_background_opacity knob).
//
// Out of scope still: icon glow rings, sub-item arc, centre puck.
// Those stay on the iced canvas and paint on top of this layer.
//
// See feedback memory entries #2 (Y flip), #10 (vec4 packing for
// std140 alignment), #12 (hybrid SDF + canvas) for context.

struct Uniforms {
    inner_r: f32,
    outer_r: f32,
    gap_rad: f32,
    intensity: f32,
    slot_count: u32,
    base_alpha: f32,
    stroke_half_px: f32,
    hover_wash_peak: f32,
    // Distance from menu centre to each icon's centre, normalised.
    icon_r: f32,
    // Icon background disc radius, normalised.
    icon_bg_radius: f32,
    _pad0: vec2<f32>,
    // Per-slot RGBA. Alpha is reserved for future per-slot
    // visibility tricks but currently unused (canvas paints
    // every wedge regardless of `visible_if`).
    colors: array<vec4<f32>, 8>,
    // 8 highlight scalars packed into 2 vec4s — slot i lives at
    // `highlights_packed[i / 4][i % 4]`. See the same packing
    // in render/sdf_ring.rs::SdfRingProgram::draw.
    highlights_packed: array<vec4<f32>, 2>,
    stroke_color: vec4<f32>,
    accent_color: vec4<f32>,
    // Icon background colours: `surface1_color` is the resting
    // tone, `surface2_color` is the hover target. Lerped by `h`.
    surface1_color: vec4<f32>,
    surface2_color: vec4<f32>,
    // Menu-element transform; see render/animation.rs::MenuXformRaw.
    menu_xform_translate: vec2<f32>,
    _pad_xform0: vec2<f32>,
    menu_xform_rotate: f32,
    menu_xform_flip_scale: f32,
    menu_xform_flip_axis: u32, // 0 = X, 1 = Y
    _pad_xform1: f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

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
    out.uv = vec2<f32>(x, y);
    return out;
}

/// See aurora.wgsl for the symmetric helper. Inverse-transform
/// of the menu's translate / rotate / flip — keeps the SDF
/// wedges visually locked to the canvas wedges when the user
/// adds custom motion tracks to the menu element.
fn apply_menu_xform_inv(p: vec2<f32>) -> vec2<f32> {
    var q = p - u.menu_xform_translate;
    let c = cos(-u.menu_xform_rotate);
    let s = sin(-u.menu_xform_rotate);
    q = vec2<f32>(q.x * c - q.y * s, q.x * s + q.y * c);
    let denom = max(abs(u.menu_xform_flip_scale), 0.001);
    if u.menu_xform_flip_axis == 1u {
        q.x = q.x / denom;
    } else {
        q.y = q.y / denom;
    }
    return q;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Canvas convention (+Y down); same flip as hover_glow.wgsl.
    let p_canvas = vec2<f32>(in.uv.x, -in.uv.y);
    let p = apply_menu_xform_inv(p_canvas);
    let r = length(p);
    let aa_w = max(fwidth(r), 0.001);

    let ann_d = max(r - u.outer_r, u.inner_r - r);

    // Per-slot angular partition.
    let n = f32(u.slot_count);
    let slot_size = 6.2831853 / n;
    let theta = atan2(p.y, p.x);
    let theta_shifted = theta + 1.5707963 + slot_size * 0.5;
    let two_pi = 6.2831853;
    let theta_wrapped =
        theta_shifted - floor(theta_shifted / two_pi) * two_pi;
    let slot_f = floor(theta_wrapped / slot_size);
    let slot_i = clamp(i32(slot_f), 0, i32(u.slot_count) - 1);
    let theta_local =
        theta_wrapped - slot_f * slot_size - slot_size * 0.5;

    let half_sweep = slot_size * 0.5 - u.gap_rad * 0.5;
    let ang_offset_rad = abs(theta_local) - half_sweep;
    let ang_d = ang_offset_rad * r;

    let d = max(ann_d, ang_d);
    let aa_d = max(fwidth(d), 0.001);
    let stroke_half = u.stroke_half_px * aa_d;

    // Early-out for fragments far from both fill and stroke.
    if d > stroke_half + aa_d * 2.0 {
        discard;
    }

    let fill_mask = 1.0 - smoothstep(-aa_d, aa_d, d);
    let stroke_mask =
        1.0 - smoothstep(stroke_half - aa_d, stroke_half + aa_d, abs(d));

    // Highlight progress for the current slot.
    let h_block = u.highlights_packed[slot_i >> 2u];
    var h = 0.0;
    let h_idx = slot_i & 3;
    if h_idx == 0 { h = h_block.x; }
    else if h_idx == 1 { h = h_block.y; }
    else if h_idx == 2 { h = h_block.z; }
    else { h = h_block.w; }

    let slot_color = u.colors[slot_i];
    let stroke_col = mix(u.stroke_color.rgb, u.accent_color.rgb, h);

    // Layer 1: wedge fill (surface0).
    var color = slot_color.rgb;
    var alpha = u.base_alpha * fill_mask;

    // Layer 2: hover wash (accent over fill).
    let wash_a = u.hover_wash_peak * h * fill_mask;
    let after_wash = wash_a + alpha * (1.0 - wash_a);
    color =
        (u.accent_color.rgb * wash_a + color * alpha * (1.0 - wash_a))
        / max(after_wash, 0.001);
    alpha = after_wash;

    // Layer 3: stroke band on top of fill.
    // Canvas formula: alpha = (60 + 90*h) / 255 = 0.235 + 0.353*h.
    let stroke_a_factor = 0.235 + 0.353 * h;
    let stroke_a = stroke_a_factor * stroke_mask;
    let after_stroke = stroke_a + alpha * (1.0 - stroke_a);
    color =
        (stroke_col * stroke_a + color * alpha * (1.0 - stroke_a))
        / max(after_stroke, 0.001);
    alpha = after_stroke;

    // -- Per-slot icon background + hover glow ring --
    //
    // Each slot has an icon centred on its bisector at radius
    // `icon_r`. The canvas paints (a) a soft accent halo ring
    // 2 px outside the icon disc when hovered and (b) a filled
    // surface1→surface2 disc behind the icon glyph. The SDF
    // adds the same two passes so that, at intensity = 1, the
    // canvas can mute everything except the icon glyph itself.
    let bisector =
        f32(slot_i) * slot_size - 1.5707963;
    let icon_centre =
        vec2<f32>(cos(bisector), sin(bisector)) * u.icon_r;
    let icon_d = length(p - icon_centre) - u.icon_bg_radius;
    let icon_aa = max(fwidth(icon_d), 0.001);

    // Layer 4: hover glow ring — accent halo 2 px outside the
    // icon disc, 3 px wide, alpha (90/255)*h matching canvas.
    let glow_centre_d = 2.0 * icon_aa;
    let glow_half = 1.5 * icon_aa;
    let glow_dist = abs(icon_d - glow_centre_d);
    let glow_mask =
        1.0 - smoothstep(glow_half - icon_aa, glow_half + icon_aa, glow_dist);
    let glow_a = (90.0 / 255.0) * h * glow_mask;
    let after_glow = glow_a + alpha * (1.0 - glow_a);
    color =
        (u.accent_color.rgb * glow_a + color * alpha * (1.0 - glow_a))
        / max(after_glow, 0.001);
    alpha = after_glow;

    // Layer 5: icon background disc — surface1 → surface2 lerp,
    // alpha (230 + 25*h)/255 matching canvas.
    let bg_mask = 1.0 - smoothstep(-icon_aa, icon_aa, icon_d);
    let bg_color =
        mix(u.surface1_color.rgb, u.surface2_color.rgb, h);
    let bg_a_factor = (230.0 + 25.0 * h) / 255.0;
    let bg_a = bg_a_factor * bg_mask;
    let after_bg = bg_a + alpha * (1.0 - bg_a);
    color =
        (bg_color * bg_a + color * alpha * (1.0 - bg_a))
        / max(after_bg, 0.001);
    alpha = after_bg;

    // User intensity multiplier on the whole composite.
    alpha = alpha * u.intensity;
    if alpha <= 0.0 {
        discard;
    }

    // Pre-multiplied alpha output (matches iced's compositor).
    return vec4<f32>(color * alpha, alpha);
}
