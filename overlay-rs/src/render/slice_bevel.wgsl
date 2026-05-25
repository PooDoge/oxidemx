// Slice bevel — paints carved grooves along each wedge boundary
// (the radial dividers between slices), with directional rim
// lighting so each slice reads as a separate 3D button.
//
// For each fragment inside the wedge band, we compute the angular
// distance to the nearest wedge boundary and paint:
//   * A thin dark groove right on the boundary (where the bevel
//     meets, like the gap between two raised buttons).
//   * On the lit side of the groove (the side where the surface
//     normal faces the light), a soft white rim highlight.
//   * On the shadow side, a soft shadow that bleeds slightly
//     into the wedge.
//
// The "lit side" / "shadow side" determination uses the boundary
// normal — perpendicular to the boundary radial line — and the
// global light direction. Result: every slice appears to have its
// own 3D bevel that catches light consistently.

struct Uniforms {
    // Inner ring radius, normalised to half-extent.
    inner_r: f32,
    // Outer ring radius.
    outer_r: f32,
    // 0..=1 user knob.
    intensity: f32,
    // Number of slices (4..=8 typical). Each boundary sits at
    // `(2π / slot_count) * (k + 0.5)` for k = 0..slot_count-1
    // in canvas convention (slice 0 bisector at -π/2).
    slot_count: u32,
    // Half-width of the groove in radians × `outer_r`. Scales to
    // match the `rim_width` of disc_bevel for visual consistency.
    groove_width: f32,
    // Rim light direction in canvas convention.
    light_angle: f32,
    // 0..=1, how far the lit-side rim brightens.
    rim_brightness: f32,
    // 0..=1, how far the shadow-side darkens.
    shadow_amount: f32,
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
    // Canvas convention.
    let p = vec2<f32>(in.uv.x, -in.uv.y);
    let r = length(p);

    // Bail outside the wedge band.
    if r < u.inner_r * 0.95 || r > u.outer_r * 1.02 {
        discard;
    }

    let intensity = clamp(u.intensity, 0.0, 1.0);
    let pi = 3.14159265359;
    let two_pi = pi * 2.0;
    let n = max(f32(u.slot_count), 1.0);
    let slice_angle = two_pi / n;

    // Angle of this fragment, in canvas convention.
    let theta = atan2(p.y, p.x);
    // Slice boundaries sit at:
    //   bisector_k - half_sweep, bisector_k + half_sweep
    // where bisector_k = -π/2 + k * slice_angle and
    // half_sweep = slice_angle / 2.
    // So boundaries are at:
    //   -π/2 + k * slice_angle - slice_angle/2
    //   = -π/2 + (k - 0.5) * slice_angle
    //   = -π/2 - slice_angle/2 + k * slice_angle
    // The nearest boundary's angle has the form
    //   boundary = -π/2 - slice_angle/2 + round((theta - boundary0) / slice_angle) * slice_angle
    // Simpler: compute (theta + π/2 + slice_angle/2) mod slice_angle
    // → distance from start-of-wedge boundary.
    let boundary0 = -pi * 0.5 - slice_angle * 0.5;
    var rel = (theta - boundary0);
    // Normalise to a positive [0, slice_angle) range.
    rel = rel - floor(rel / slice_angle) * slice_angle;
    // Now rel is the angular distance from the previous boundary
    // (clockwise). The next boundary sits at slice_angle.
    // Distance to nearest boundary:
    let dist_to_prev = rel;
    let dist_to_next = slice_angle - rel;
    let nearest_dist_rad = min(dist_to_prev, dist_to_next);
    // Convert to a length-units distance using r so the groove
    // looks the same width near the inner ring as near the outer
    // ring (a constant angular width would look much narrower at
    // the inner boundary).
    let groove_dist = nearest_dist_rad * r;

    // Bail well away from any boundary.
    if groove_dist > u.groove_width * 2.5 {
        discard;
    }

    // Side of the boundary: -1 if we're on the previous-side,
    // +1 if we're on the next-side. (Used to pick lit vs shadow.)
    let side = select(-1.0, 1.0, dist_to_next < dist_to_prev);

    // Boundary-normal direction in 2D. The boundary is a radial
    // line at angle = (k * slice_angle + boundary0). The normal
    // is perpendicular to the radial direction = tangent
    // direction at that angle.
    // Boundary angle of the nearest boundary:
    let nearest_boundary_offset = select(0.0, slice_angle, dist_to_next < dist_to_prev);
    // Index of the nearest boundary in fragments where we're
    // close to "next" — we shifted by slice_angle. Doesn't
    // matter for the normal itself; only the angle does.
    let boundary_theta = boundary0 + (theta - boundary0) - rel + nearest_boundary_offset;

    // Boundary normal: perpendicular to the radial direction at
    // boundary_theta. In 2D: `n = (-sin(boundary_theta),
    // cos(boundary_theta))` flips 90° from radial. Sign carries
    // which side the normal points to — we multiply by `side`
    // so the normal always points AWAY from this fragment's
    // side of the boundary.
    let nx = -sin(boundary_theta) * side;
    let ny = cos(boundary_theta) * side;

    // Light direction.
    let lx = cos(u.light_angle);
    let ly = sin(u.light_angle);
    let n_dot_l = clamp(nx * lx + ny * ly, -1.0, 1.0);
    // Lit side: this side's normal faces toward the light → this
    // side gets the highlight. n_dot_l > 0 means lit.
    let lit_amount = clamp(n_dot_l * 0.5 + 0.5, 0.0, 1.0);

    // Groove mask: a thin dark line peaked at groove_dist == 0.
    let groove_falloff = exp(-groove_dist / max(u.groove_width * 0.4, 0.0001));
    let groove_a = groove_falloff * 0.55 * clamp(u.shadow_amount, 0.0, 1.0);
    let groove_rgb = vec3<f32>(0.0);

    // Rim mask: peaks just OFF the boundary (on the lit side),
    // falls off into the slice interior.
    let rim_offset = groove_dist - u.groove_width * 0.6;
    let rim_falloff = clamp(
        exp(-max(rim_offset, 0.0) / max(u.groove_width * 0.8, 0.0001)),
        0.0,
        1.0,
    );
    // Only paint rim on the lit side (n_dot_l > 0).
    let rim_lit_gate = max(n_dot_l, 0.0);
    let rim_a = rim_falloff * rim_lit_gate * clamp(u.rim_brightness, 0.0, 1.0) * 0.55;
    let rim_rgb = vec3<f32>(1.0);

    // Shadow side wash: same falloff but on the shadow side.
    let shadow_lit_gate = max(-n_dot_l, 0.0);
    let shadow_side_a = rim_falloff * shadow_lit_gate * clamp(u.shadow_amount, 0.0, 1.0) * 0.4;
    let shadow_side_rgb = vec3<f32>(0.0);

    // Fade out at the inner / outer ring boundaries so the bevel
    // doesn't end abruptly.
    let radial_fade = smoothstep(u.inner_r * 0.97, u.inner_r * 1.05, r)
        * (1.0 - smoothstep(u.outer_r * 0.95, u.outer_r * 1.02, r));

    let total_a_dark = (groove_a + shadow_side_a) * radial_fade;
    let total_a_light = rim_a * radial_fade;

    let total_a = clamp(total_a_dark + total_a_light, 0.0, 1.0);
    var rgb = vec3<f32>(0.0);
    if total_a > 0.0001 {
        rgb = (rim_rgb * total_a_light + groove_rgb * groove_a + shadow_side_rgb * shadow_side_a)
            / max(total_a_light + groove_a + shadow_side_a, 0.0001);
    }

    return vec4<f32>(rgb * intensity, total_a * intensity);
}
