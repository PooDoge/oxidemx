// SDF hover glow — paints a soft glowing aura along the wedge
// boundary of the hovered slice. Distance-field math gives
// resolution-independent, perfectly anti-aliased edges with no
// MSAA dependency.
//
// Same clip-space-uv vertex pattern as aurora + ripple. Fragment
// computes the SDF for the wedge, then `exp(-d / sigma)` falloff
// scaled by hover progress + user intensity.

struct Uniforms {
    bisector_rad: f32,
    half_sweep: f32,
    inner_r: f32,
    outer_r: f32,
    progress: f32,
    intensity: f32,
    _pad0: vec2<f32>,
    color: vec4<f32>,
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

// Signed distance from `p` to the wedge defined by:
//   * centred at origin
//   * angular sweep [bisector - half_sweep, bisector + half_sweep]
//   * radial range [inner_r, outer_r]
// Negative inside, positive outside. We compute it as the max of:
//   1) distance to the inner annulus edge
//   2) distance to the outer annulus edge
//   3) distance to each radial side (start_angle, end_angle)
fn wedge_sdf(p: vec2<f32>, bisector: f32, half_sweep: f32, inner_r: f32, outer_r: f32) -> f32 {
    let r = length(p);
    // Radial distance: how far we are from the [inner, outer] band.
    // 0 inside the band, positive outside.
    let band_dist = max(inner_r - r, r - outer_r);

    // Angular distance from the wedge sweep. Compute the angle
    // to `p` and take its delta from the bisector, clamped to
    // [-π, π] so wraparound at ±π doesn't break the comparison.
    let theta = atan2(p.y, p.x);
    var dtheta = theta - bisector;
    // Wrap to [-π, π].
    let pi = 3.14159265359;
    if dtheta > pi {
        dtheta = dtheta - 2.0 * pi;
    } else if dtheta < -pi {
        dtheta = dtheta + 2.0 * pi;
    }
    // Distance OUTSIDE the angular sweep, in radians. 0 if inside.
    // Convert to a distance-along-arc by multiplying by r so the
    // angular SDF is in the same units as the radial SDF.
    let arc_dist = (abs(dtheta) - half_sweep) * r;

    return max(band_dist, arc_dist);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Clip space puts +Y up but iced's canvas — and the
    // bisector_rad we receive — uses +Y down (slice 0 at
    // bisector = -π/2 means "12 o'clock" with Y-down). Flip y
    // here so atan2(p.y, p.x) lands in the same convention as
    // the bisector. Skipping this gives a 180° rotation —
    // hovering the top slice would glow the bottom one.
    let p = vec2<f32>(in.uv.x, -in.uv.y);
    let r = length(p);
    if r > 1.05 {
        discard;
    }

    let d = wedge_sdf(p, u.bisector_rad, u.half_sweep, u.inner_r, u.outer_r);

    // Two glow components composed:
    //   * Inner highlight: positive `d` close to the boundary
    //     glows brightly with a tight falloff (sharper edge ring)
    //   * Outer aura: same direction but slower falloff so the
    //     glow extends visibly past the wedge
    // `d < 0` is inside the wedge. We want the glow to live
    // around the boundary and extend OUTWARD, so use `max(d, 0)`
    // for the outer falloff and a small wash inside.

    let inner_wash = exp(-max(0.0, -d) * 12.0);
    let outer_aura = exp(-max(0.0, d) * 6.0);

    // Combine + scale by progress + intensity. Cap at ~0.7 so a
    // 100 % intensity glow doesn't completely flood the wedge.
    let glow = (inner_wash * 0.4 + outer_aura) * u.progress * u.intensity * 0.7;

    return vec4<f32>(u.color.rgb * glow, glow);
}
