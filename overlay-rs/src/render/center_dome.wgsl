// Centre dome — turns the centre deadzone into a sphere with
// directional Lambert shading + a Phong-like specular highlight.
// Reads as a physical button at the centre of the radial menu.
//
// Direction-aware: light from upper-left by default. Highlight is
// a tight Phong spot; the lit hemisphere gets a soft brighten,
// the shadow hemisphere darkens. Uses the canvas's existing
// centre puck colour as the base; only the lighting overlay is
// painted by the shader.

struct Uniforms {
    // Outer radius of the centre puck, normalised to half-extent.
    radius: f32,
    // 0..=1 user knob.
    intensity: f32,
    // Direction to the virtual light source, in radians (canvas
    // convention — `-3π/4` is upper-left for the conventional
    // 3D look).
    light_angle: f32,
    // Phong shininess exponent (1..256). Higher = tighter
    // specular spot. Default ~32.
    shininess: f32,
    // 0..=1, how far the lit hemisphere brightens above neutral.
    rim_brightness: f32,
    // 0..=1, how far the shadow hemisphere darkens below neutral.
    shadow_amount: f32,
    _pad0: vec2<f32>,
    // Specular highlight colour (typically near-white).
    specular_color: vec4<f32>,
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
    // Canvas convention: +Y down. Sample in canvas coords.
    let p = vec2<f32>(in.uv.x, -in.uv.y);
    let r = length(p);

    // Bail outside the puck (with a 1-pixel buffer for AA).
    if r > u.radius + 0.005 {
        discard;
    }

    let intensity = clamp(u.intensity, 0.0, 1.0);

    // Sphere math: lift the 2D point onto a hemisphere of radius
    // `radius`. Z is the height of the dome above the disc plane.
    // For a perfect hemisphere: z = sqrt(radius^2 - r^2). We
    // soften the edges with smoothstep so the dome doesn't have
    // a hard silhouette.
    let r_norm = clamp(r / u.radius, 0.0, 1.0);
    // Alpha mask — fade the effect to 0 at the edge so it
    // doesn't form a hard ring.
    let edge_fade = 1.0 - smoothstep(0.85, 1.0, r_norm);

    // Surface normal of the dome at this fragment. For a
    // hemisphere parameterised by (x, y) in 2D:
    //   n = (x/R, y/R, sqrt(1 - (r/R)^2))
    let z = sqrt(max(0.0, 1.0 - r_norm * r_norm));
    let n = vec3<f32>(p.x / u.radius, p.y / u.radius, z);

    // Light direction: 3D unit vector. We treat the light as
    // coming from above (positive Z) tilted by `light_angle` in
    // the XY plane. A 30° elevation feels right — neither flat
    // top-down nor grazing-edge.
    let elev = 0.523599; // 30° in radians
    let lz = sin(elev);
    let lxy = cos(elev);
    let l = vec3<f32>(cos(u.light_angle) * lxy, sin(u.light_angle) * lxy, lz);

    let n_dot_l = clamp(dot(n, l), -1.0, 1.0);
    // Lambert diffuse, signed: positive = lit, negative = shadow.
    let lit_amount = clamp(n_dot_l * 0.5 + 0.5, 0.0, 1.0);

    // Phong specular: reflect the light around the normal, dot
    // with view direction (we approximate view as +Z, the
    // user looking straight at the disc). Then take the result
    // to the `shininess` power.
    // r_l = 2 (n·l) n - l
    let r_l = 2.0 * n_dot_l * n - l;
    let v = vec3<f32>(0.0, 0.0, 1.0);
    let r_dot_v = clamp(dot(r_l, v), 0.0, 1.0);
    let specular = pow(r_dot_v, max(u.shininess, 1.0));

    // Compose:
    //  * Bright wash on the lit hemisphere — adds white * rim_brightness.
    //  * Dark wash on the shadow hemisphere — adds black * shadow_amount.
    //  * Specular dot — adds white * specular regardless of side.
    // Sum all into one RGBA premultiplied.
    let lit_factor = max(0.0, n_dot_l) * clamp(u.rim_brightness, 0.0, 1.0);
    let shadow_factor = max(0.0, -n_dot_l) * clamp(u.shadow_amount, 0.0, 1.0);

    let highlight_a = lit_factor * 0.5 + specular * u.specular_color.a;
    let shadow_a = shadow_factor * 0.55;

    let highlight_rgb = u.specular_color.rgb;
    let shadow_rgb = vec3<f32>(0.0);

    let total_a = clamp(highlight_a + shadow_a, 0.0, 1.0);
    var rgb = vec3<f32>(0.0);
    if total_a > 0.0001 {
        rgb = (highlight_rgb * highlight_a + shadow_rgb * shadow_a)
            / max(highlight_a + shadow_a, 0.0001);
    }

    return vec4<f32>(rgb * intensity * edge_fade, total_a * intensity * edge_fade);
}
