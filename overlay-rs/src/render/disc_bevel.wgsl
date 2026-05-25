// Disc bevel — paints a beveled rim along the outer edge of the
// wedge ring + a carved inset shadow along the inner edge. The
// effect reads as a polished coin or carved metal disc, framing
// the radial menu in 3D regardless of hover state.
//
// Directional: the rim light brightens where the surface normal
// faces a virtual light source (default upper-left, configurable
// via `light_angle`), and dims on the opposite side. The inner
// shadow is the inverse — darkest where the lit-side rim glows.
//
// Layered ABOVE the canvas wedges. Inside the ring's wedge band
// it's transparent; only the rim/inset bands carry alpha, so
// canvas-painted slice colour shows through unaffected.

struct Uniforms {
    // Inner ring radius, normalised to half-extent ([0,1]).
    inner_r: f32,
    // Outer ring radius, normalised the same way.
    outer_r: f32,
    // 0..=1 user knob — fades the entire effect out.
    intensity: f32,
    // Half-width of the outer rim band in normalised units.
    // Larger = thicker rim. Default ~0.04.
    rim_width: f32,
    // Half-width of the inner inset band (the carved shadow at
    // the centre-zone boundary).
    inset_width: f32,
    // Direction of the virtual light source, in radians (canvas
    // convention: -π/2 = top, 0 = right). Default `-3π/4` =
    // upper-left for the conventional 3D look.
    light_angle: f32,
    // 0..=1, additional darkening on the lit-side opposite (the
    // shadow side of the rim). Stronger = more pronounced 3D.
    shadow_strength: f32,
    _pad0: f32,
    // Rim highlight colour (typically near-white).
    rim_color: vec4<f32>,
    // Inset shadow colour (typically near-black).
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
    // Canvas convention: +Y down, slice 0 at -π/2 (top).
    let p = vec2<f32>(in.uv.x, -in.uv.y);
    let r = length(p);

    // Bail well outside the disc.
    if r > u.outer_r + u.rim_width + 0.05 {
        discard;
    }
    // Bail well inside the inner ring.
    if r < u.inner_r - u.inset_width - 0.05 {
        discard;
    }

    let intensity = clamp(u.intensity, 0.0, 1.0);

    // ---------- OUTER RIM ----------
    // Distance from the outer ring boundary. Negative inside the
    // wedge band, positive outside.
    let d_outer = r - u.outer_r;
    // Rim mask: peaks at d_outer == 0 (right on the boundary),
    // falls off both inward (band of `rim_width`) and outward
    // (~half rim_width for a soft outer halo).
    let rim_inner_falloff = clamp(1.0 + d_outer / u.rim_width, 0.0, 1.0); // 0 deep inside, 1 at boundary
    let rim_outer_falloff = clamp(1.0 - d_outer / (u.rim_width * 0.5), 0.0, 1.0);
    let rim_mask = min(rim_inner_falloff, rim_outer_falloff);
    // Smoothstep for a softer roll-off than the hard linear.
    let rim_mask_soft = smoothstep(0.0, 1.0, rim_mask);

    // Directional weight: cos of the angle between the surface
    // normal (which is just the radial direction `p / r`) and
    // the light direction. cos in [-1, 1]; remap to [0, 1].
    let theta = atan2(p.y, p.x);
    let normal_x = cos(theta);
    let normal_y = sin(theta);
    let light_x = cos(u.light_angle);
    let light_y = sin(u.light_angle);
    let lambert = clamp(normal_x * light_x + normal_y * light_y, -1.0, 1.0);
    // Lit side: lambert > 0; shadow side: lambert < 0.
    let lit_amount = clamp(lambert * 0.5 + 0.5, 0.0, 1.0); // [0, 1]

    // Rim contribution: bright on lit side, dim on shadow side.
    // Use lit_amount^2 to bias toward the highlight peak, and
    // include a small base so the rim is visible (not pitch-
    // black) on the shadow side.
    let rim_strength = mix(0.15, 1.0, lit_amount * lit_amount);
    let rim_a = rim_mask_soft * rim_strength * u.rim_color.a;
    let rim_rgb = u.rim_color.rgb;

    // Shadow side darkening just outside the rim peak — gives
    // the rim an "edge that bevels into shadow" look.
    let shadow_side = clamp(-lambert, 0.0, 1.0); // 0..1 on shadow side
    let rim_shadow_a = rim_mask_soft * shadow_side * shadow_side * u.shadow_strength * 0.5;

    // ---------- INNER INSET ----------
    // Carved shadow at the inner ring boundary — appears as a
    // groove between the centre puck and the wedge ring.
    let d_inner = r - u.inner_r;
    let inset_inner_falloff = clamp(1.0 + d_inner / u.inset_width, 0.0, 1.0); // 0 well below inner_r, 1 at boundary
    let inset_outer_falloff = clamp(1.0 - d_inner / (u.inset_width * 0.6), 0.0, 1.0);
    let inset_mask = min(inset_inner_falloff, inset_outer_falloff);
    let inset_mask_soft = smoothstep(0.0, 1.0, inset_mask);

    // Inverse Lambert — the inner inset is darkest on the LIT
    // side (the rim catches light, the inset behind it falls
    // into shadow). Subtle: a thin inset highlight on the
    // shadow side gives the groove its own bevel.
    let inset_a = inset_mask_soft * (0.4 + 0.6 * lit_amount) * u.shadow_color.a;
    let inset_rgb = u.shadow_color.rgb;

    // Shadow-side inset highlight (very subtle).
    let inset_hi_a = inset_mask_soft * shadow_side * 0.18 * u.rim_color.a;

    // ---------- COMPOSITE ----------
    // Combine the two main bands. They never overlap (outer rim
    // sits at r ≈ outer_r; inner inset sits at r ≈ inner_r),
    // so straight additive composition is fine.
    let dark_a = rim_shadow_a + inset_a;
    let bright_a = rim_a + inset_hi_a;

    // OVER blending: bright (white) on top of dark (black).
    // Just sum alphas weighted, RGB by alpha-weighted average.
    let total_a = clamp(dark_a + bright_a, 0.0, 1.0);
    var rgb = vec3<f32>(0.0);
    if total_a > 0.0001 {
        rgb = (rim_rgb * bright_a + inset_rgb * dark_a) / max(bright_a + dark_a, 0.0001);
    }

    return vec4<f32>(rgb * intensity, total_a * intensity);
}
