// Drop shadow — paints a soft outer shadow OUTSIDE the disc,
// offset slightly away from the light source. Reads as the
// menu casting a real shadow onto whatever is behind it,
// turning the radial menu from "painted on the screen" into a
// physical floating object.
//
// Renders BEHIND everything else (first in the layer stack), so
// the disc's own contents paint over it cleanly.

struct Uniforms {
    // Outer ring radius, normalised to half-extent.
    outer_r: f32,
    // 0..=1 user knob.
    intensity: f32,
    // How far the shadow extends past the disc edge in
    // normalised units. Larger = bigger soft halo.
    spread: f32,
    // How sharp the shadow's inner edge is. 0..=1; higher =
    // sharper transition right at the disc boundary.
    falloff: f32,
    // Direction of the virtual light, in radians (canvas
    // convention). The shadow OFFSETS in the OPPOSITE direction.
    light_angle: f32,
    // Offset distance in normalised units (so the shadow is
    // displaced from directly-under-the-disc to behind-and-
    // away-from-the-light).
    offset_dist: f32,
    _pad0: vec2<f32>,
    // Shadow colour (typically near-black, low alpha).
    shadow_color: vec4<f32>,
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

    // Shadow centre is offset OPPOSITE the light direction.
    let dx = -cos(u.light_angle) * u.offset_dist;
    let dy = -sin(u.light_angle) * u.offset_dist;
    let centre = vec2<f32>(dx, dy);

    // Distance from the offset shadow centre.
    let d = distance(p, centre);

    // Bail well outside the shadow radius.
    let max_r = u.outer_r + u.spread + 0.05;
    if d > max_r {
        discard;
    }
    // Bail well inside (the disc itself paints over us anyway,
    // but discarding here saves ALU on internal pixels).
    let inner_cutoff = u.outer_r * 0.4;
    if d < inner_cutoff {
        discard;
    }

    let intensity = clamp(u.intensity, 0.0, 1.0);

    // Two-stage soft falloff:
    //   * Inner edge of the shadow ring sits at outer_r;
    //     falloff sharpness is `falloff` (smoothstep width).
    //   * Outer edge fades to 0 over `spread`.
    let inner_width = mix(u.outer_r * 0.4, u.outer_r * 0.05,
                          clamp(u.falloff, 0.0, 1.0));
    let inner_a = smoothstep(u.outer_r - inner_width, u.outer_r, d);
    let outer_a = 1.0 - smoothstep(u.outer_r, u.outer_r + u.spread, d);
    var alpha = inner_a * outer_a * u.shadow_color.a;

    // Directional bias: lit-side gets less shadow (the light
    // pushes the shadow to the dark side). Compute the angle
    // from the OFFSET centre and dim the lit-side hemisphere.
    let theta = atan2(p.y - centre.y, p.x - centre.x);
    let lambert = cos(theta - u.light_angle);
    // lambert in [-1, 1]; +1 = lit side, -1 = shadow side.
    // Scale alpha by (1 - lit_factor) so lit-side fades.
    let lit_dim = clamp((lambert + 1.0) * 0.5, 0.0, 1.0); // 0..1, 1=lit
    alpha = alpha * (1.0 - lit_dim * 0.55);

    return vec4<f32>(u.shadow_color.rgb * alpha * intensity, alpha * intensity);
}
