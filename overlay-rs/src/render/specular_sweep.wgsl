// Specular sweep — animated narrow band of light that rotates
// slowly around the disc's outer rim. Adds dynamic motion to
// the static 3D framing; reads as a polished surface catching
// ambient light as it turns. Works alongside disc_bevel (which
// owns the static rim light) by adding motion on top.
//
// The sweep peaks on the lit-side hemisphere — i.e. its
// brightness is gated by the dot product with `light_angle` —
// so when it crosses to the shadow side it fades out cleanly.
// This avoids a "Christmas-tree spinning light" effect on the
// dark side and keeps the lighting story coherent.

struct Uniforms {
    // Inner ring radius, normalised to half-extent.
    inner_r: f32,
    // Outer ring radius.
    outer_r: f32,
    // 0..=1 user knob.
    intensity: f32,
    // Seconds since the menu opened. Drives the sweep angle.
    time: f32,
    // Period in seconds for one full revolution of the sweep
    // around the disc.
    period_s: f32,
    // Angular half-width of the sweep band in radians. Smaller
    // = tighter, more like a polished gleam; larger = broader
    // light wash.
    half_width_rad: f32,
    // Same global light direction as disc_bevel etc. The sweep's
    // brightness peaks when the sweep angle aligns with the
    // light direction, fades to zero on the shadow side.
    light_angle: f32,
    _pad0: f32,
    // Sweep colour (typically near-white).
    sweep_color: vec4<f32>,
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

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let p = vec2<f32>(in.uv.x, -in.uv.y);
    let r = length(p);

    // Bail outside the wedge band.
    if r < u.inner_r * 0.95 || r > u.outer_r * 1.04 {
        discard;
    }

    let intensity = clamp(u.intensity, 0.0, 1.0);
    let pi = 3.14159265359;

    // Current sweep angle: rotates one full revolution per
    // `period_s`. Avoid period_s == 0 division by clamping.
    let period = max(u.period_s, 0.5);
    let sweep_theta = (u.time / period) * 2.0 * pi;

    // Angle of this fragment.
    let theta = atan2(p.y, p.x);

    // Angular distance from this fragment to the sweep centre,
    // wrapped to [-π, π].
    var dtheta = theta - sweep_theta;
    if dtheta > pi {
        dtheta = dtheta - 2.0 * pi;
    } else if dtheta < -pi {
        dtheta = dtheta + 2.0 * pi;
    }

    // Sweep envelope: cosine falloff over `half_width_rad`. Out-
    // side that window the sweep is fully off; inside, it
    // smoothly peaks at dtheta == 0.
    let abs_dtheta = abs(dtheta);
    if abs_dtheta > u.half_width_rad {
        discard;
    }
    let cos_phase = cos((abs_dtheta / max(u.half_width_rad, 0.0001)) * (pi * 0.5));
    let sweep_strength = cos_phase * cos_phase; // squared cosine for a softer peak

    // Lit-side gate: dot product of sweep direction with light
    // direction. When the sweep is on the lit hemisphere, the
    // result is positive; on the shadow side it's negative.
    let sweep_dx = cos(sweep_theta);
    let sweep_dy = sin(sweep_theta);
    let light_dx = cos(u.light_angle);
    let light_dy = sin(u.light_angle);
    let lit_align = sweep_dx * light_dx + sweep_dy * light_dy;
    // Map to [0, 1] with a slight bias toward the lit side —
    // we want the sweep visible across most of the lit half but
    // mostly invisible on the shadow side.
    let lit_gate = clamp(lit_align * 0.6 + 0.4, 0.0, 1.0);

    // Radial mask: peak at the outer rim, fall off inward so
    // the sweep stays close to the edge. This makes it feel
    // like reflected light catching a polished bevel rather
    // than flooding the entire wedge.
    let r_norm = clamp((r - u.inner_r) / max(u.outer_r - u.inner_r, 0.0001), 0.0, 1.0);
    // Peak at r_norm ~ 0.85 (just inside the outer edge) and
    // fall off both ways.
    let radial_peak = exp(-pow(r_norm - 0.85, 2.0) * 18.0);

    // Outside the disc edge fade-out (matches disc_bevel's
    // outer falloff so the two reads as one optical effect).
    let outside_fade = 1.0 - smoothstep(u.outer_r, u.outer_r * 1.04, r);

    let alpha =
        sweep_strength * lit_gate * radial_peak * outside_fade * u.sweep_color.a;

    return vec4<f32>(u.sweep_color.rgb * alpha * intensity, alpha * intensity);
}
