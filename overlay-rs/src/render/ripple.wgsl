// Haptic-ripple shader: a thin glowing ring that expands from
// the menu centre and fades out as `progress` goes 0 → 1.
//
// Uses the same vertex-stage UV trick as the aurora backdrop so
// the ring is centred and scaled correctly on any HiDPI factor.

struct Uniforms {
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

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let p = in.uv;
    let r = length(p);
    if r > 1.0 {
        discard;
    }

    // Ring radius eases out from 0.18 (just outside the centre
    // puck) to 0.95 (just inside the disc edge). Cubic ease-out
    // matches the menu's overall animation feel.
    let pr = u.progress;
    let eased = 1.0 - pow(1.0 - pr, 3.0);
    let ring_r = mix(0.18, 0.95, eased);

    // Ring thickness widens slightly as it expands so the ring
    // softens visually instead of staying a hairline at the
    // largest radii.
    let half_thickness = mix(0.04, 0.08, eased);

    // Distance from the current pixel to the ring → fwidth-style
    // soft edge so the ring is anti-aliased without MSAA.
    let d = abs(r - ring_r);
    let edge = 1.0 - smoothstep(half_thickness * 0.4, half_thickness, d);

    // Alpha decays linearly with progress — the ring is brightest
    // at trigger and fades to invisible by the time it reaches
    // the disc edge. Multiplied by user intensity for taste.
    let life = 1.0 - pr;
    let alpha = u.intensity * edge * life * 0.85;

    return vec4<f32>(u.color.rgb * alpha, alpha);
}
