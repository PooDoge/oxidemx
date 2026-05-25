// Parallax-tilt effect — paints inside the hovered wedge a
// directional specular highlight (near the cursor) plus a soft
// inset shadow on the side facing away. The visual reads as
// "this slice is tilted toward your finger", giving the menu a
// 3D-button feel without actually transforming geometry.
//
// Layered ABOVE hover_glow (which paints the edge-aura) so the
// in-wedge lighting wins where they overlap. Cheap: one wedge
// SDF + a couple of exp() falloffs.

struct Uniforms {
    // Wedge geometry (same convention as hover_glow.wgsl).
    bisector_rad: f32,
    half_sweep: f32,
    inner_r: f32,
    outer_r: f32,
    // Tween: 0 = no effect, 1 = full effect.
    progress: f32,
    // User-tunable strength (0..=1).
    intensity: f32,
    // 0..=1, how much the side facing AWAY from the cursor
    // darkens.
    shadow_amount: f32,
    // 0..=1, specular tightness; higher = smaller highlight.
    sharpness: f32,
    // Cursor position in clip-space UV (centre of menu = (0,0),
    // with +X right and +Y down to match canvas convention —
    // the canvas-side caller does the flip).
    cursor_uv: vec2<f32>,
    _pad0: vec2<f32>,
    // Highlight colour (typically white / soft white).
    highlight_color: vec4<f32>,
    // Slice-local accent — fades into the highlight on the lit
    // side and gets darkened on the shadow side. Usually the
    // slice's configured colour or the active palette accent.
    accent_color: vec4<f32>,
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

// Wedge SDF — same shape as hover_glow.wgsl. Negative inside.
fn wedge_sdf(p: vec2<f32>, bisector: f32, half_sweep: f32, inner_r: f32, outer_r: f32) -> f32 {
    let r = length(p);
    let band_dist = max(inner_r - r, r - outer_r);
    let theta = atan2(p.y, p.x);
    var dtheta = theta - bisector;
    let pi = 3.14159265359;
    if dtheta > pi {
        dtheta = dtheta - 2.0 * pi;
    } else if dtheta < -pi {
        dtheta = dtheta + 2.0 * pi;
    }
    let arc_dist = (abs(dtheta) - half_sweep) * r;
    return max(band_dist, arc_dist);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Y-flip per shader-gotchas memo #2 — bisector_rad uses
    // canvas convention (slice 0 at -π/2 = 12 o'clock with
    // Y-down). Cursor UV is also passed in canvas convention,
    // so they're consistent in this `p` space.
    let p = vec2<f32>(in.uv.x, -in.uv.y);
    let cursor = u.cursor_uv;

    // Bail outside the disc — saves ALU on offscreen pixels.
    let r = length(p);
    if r > 1.05 {
        discard;
    }

    // Inside-the-wedge mask. SDF is negative inside; we want a
    // smooth falloff so the lighting blends across the edge
    // rather than hard-clipping.
    let d = wedge_sdf(p, u.bisector_rad, u.half_sweep, u.inner_r, u.outer_r);
    if d > 0.04 {
        // Outside the wedge with margin — nothing to paint.
        discard;
    }
    // 0..1 over the inner wedge area, fading to 0 at the edge.
    let inside = clamp(-d * 25.0, 0.0, 1.0);

    // Distance from this fragment to the cursor in the same UV
    // space. Tighter sharpness → smaller highlight.
    let cursor_dist = distance(p, cursor);
    // sharpness in [0,1] maps to a falloff exponent. At 0.5 the
    // highlight has roughly the size of the wedge interior; at
    // 1.0 it's a tight specular dot.
    let falloff = mix(4.0, 16.0, clamp(u.sharpness, 0.0, 1.0));
    let specular = exp(-cursor_dist * falloff);

    // Direction from wedge centre toward cursor (in radial
    // coords). Used to compute "is this fragment on the lit side
    // or the shadow side". `wedge_centre` is the radial midpoint
    // of the wedge (between inner_r and outer_r).
    let mid_r = (u.inner_r + u.outer_r) * 0.5;
    let wedge_centre = vec2<f32>(cos(u.bisector_rad), sin(u.bisector_rad)) * mid_r;
    let to_cursor = normalize(cursor - wedge_centre);
    let to_frag = p - wedge_centre;
    // Dot product gives [-1, 1] — +1 = on the lit side, -1 =
    // on the shadow side. Half-add to map to [0,1].
    let lit = clamp(dot(normalize(to_frag), to_cursor) * 0.5 + 0.5, 0.0, 1.0);

    // Compose the lighting:
    //   * Specular: sharp white highlight where the cursor sits.
    //   * Side wash: gentle accent-coloured boost on the lit side
    //     and accent-coloured darkening on the shadow side.
    let shadow_strength = clamp(u.shadow_amount, 0.0, 1.0);
    let side_boost = (lit - 0.5) * 2.0; // [-1, 1] across the wedge

    // Highlight RGBA (specular): pure highlight_color modulated
    // by specular strength.
    let highlight_rgb = u.highlight_color.rgb * specular;
    let highlight_a = specular * u.highlight_color.a;

    // Side wash: tints the wedge with `accent_color` boost on
    // the lit side, darkens with negative wash on the shadow
    // side. The shadow side just darkens (alpha negative would
    // be wrong; use a slightly different composition).
    let wash_strength = max(side_boost, 0.0) * 0.45;
    let wash_rgb = u.accent_color.rgb * wash_strength;
    let wash_a = wash_strength * u.accent_color.a;

    // Shadow dim: where lit < 0.5, paint a translucent black
    // proportional to (0.5 - lit).
    let shadow_strength_local = max(-side_boost, 0.0) * shadow_strength;
    let shadow_rgb = vec3<f32>(0.0, 0.0, 0.0);
    let shadow_a = shadow_strength_local * 0.55;

    // Combine via additive-then-alpha-blend. We output a single
    // RGBA where:
    //   rgb = highlight_rgb + wash_rgb
    //   a = max of the two positive contributions, then blend
    //       the shadow on top by mixing toward black.
    let lit_rgb = highlight_rgb + wash_rgb;
    let lit_a = max(highlight_a, wash_a);

    // Shadow darkens by *adding* a black layer with its own alpha.
    let final_rgb = mix(lit_rgb, shadow_rgb, shadow_a);
    let final_a = lit_a + shadow_a - lit_a * shadow_a; // OVER op

    // Gate by inside-the-wedge mask, the user's intensity, and
    // the hover progress.
    let gate = inside * u.progress * clamp(u.intensity, 0.0, 1.0);
    return vec4<f32>(final_rgb * gate, final_a * gate);
}
