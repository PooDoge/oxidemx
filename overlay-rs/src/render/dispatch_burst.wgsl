// Dispatch-burst shader: a celebratory flourish anchored at the
// activated slice's icon position. Three selectable styles
// share the same vertex stage and the same uniform layout —
// fragment branches on `style` and produces a different look
// for each:
//
//   * 0 = Sparks — particles fan outward from the slice in a
//     scattered cone, fading as they travel.
//   * 1 = Shockwave — three concentric rings cascade outward
//     from the slice's icon, like a ripple anchored at the
//     pressed wedge instead of the menu centre.
//   * 2 = Glow — single bright radial flash at the slice that
//     fades quickly. Fastest, most subtle.
//
// Coordinate convention: `p = (uv.x, -uv.y)` flips clip-space
// +Y-up to canvas +Y-down so the origin handed in from Rust
// (computed in canvas convention, slice 0 = top = (0, -0.6))
// lines up with the visible slice. Same trick the hover_glow
// shader uses — see render/hover_glow.wgsl for the full
// rationale.

struct Uniforms {
    progress: f32,
    intensity: f32,
    // 0 = Sparks, 1 = Shockwave, 2 = Glow.
    style: u32,
    _pad0: f32,
    // Burst origin in normalised half-extent units, canvas
    // convention (+Y down). Computed from the slice angle in
    // Rust so all three styles can anchor without re-deriving
    // the geometry on the GPU.
    origin: vec2<f32>,
    _pad1: vec2<f32>,
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

// Cheap hash for pseudo-random spark angles + speeds. Uses the
// classic sin-fract trick — quality is fine for 16 sparks at
// 60Hz, no need for anything fancier.
fn hash11(n: f32) -> f32 {
    return fract(sin(n * 78.233) * 43758.5453);
}

fn sparks(p: vec2<f32>, origin: vec2<f32>, pr: f32) -> f32 {
    // Outward axis: the direction from menu centre to the
    // slice's origin. Sparks fan around this axis so the burst
    // reads as "exploding away from the activated wedge",
    // not in a random direction.
    let outward = normalize(origin);
    let centre_angle = atan2(outward.y, outward.x);
    // ~115° fan. Wide enough that a couple of sparks always
    // travel slightly sideways, narrow enough that the burst
    // still feels directional.
    let spread = 2.0;

    var acc = 0.0;
    let life = 1.0 - pr;
    for (var i: i32 = 0; i < 16; i = i + 1) {
        let fi = f32(i);
        let h1 = hash11(fi * 1.31 + 0.7);
        let h2 = hash11(fi * 2.17 + 5.4);
        let h3 = hash11(fi * 3.71 + 9.1);
        let angle = centre_angle + (h1 - 0.5) * spread;
        // Speed scatter: closest sparks barely move, fastest
        // travel ~70% of the menu radius. Looks like a real
        // particle distribution rather than a uniform shell.
        let speed = 0.18 + h2 * 0.55;
        // Cubic ease-out on travel so sparks decelerate as
        // they fade — feels like real momentum loss.
        let t = 1.0 - pow(1.0 - pr, 3.0);
        let r = t * speed;
        let pos = origin + vec2<f32>(cos(angle), sin(angle)) * r;
        let d = length(p - pos);
        // Particles shrink as they fade so trailing sparks are
        // softer than leading ones.
        let size = 0.022 - pr * 0.010;
        let particle = 1.0 - smoothstep(0.0, max(size, 0.001), d);
        // Per-spark brightness jitter so the burst doesn't look
        // like a uniform cluster.
        acc = acc + particle * (0.45 + h3 * 0.55);
    }
    return clamp(acc * life * 1.4, 0.0, 1.0);
}

fn shockwave(p: vec2<f32>, origin: vec2<f32>, pr: f32) -> f32 {
    let d = length(p - origin);
    var acc = 0.0;
    // Three rings cascading at 0%, 21%, 42%. Each one takes
    // ~58% of the total duration to expand, so the final ring
    // finishes right at pr=1.0.
    for (var i: i32 = 0; i < 3; i = i + 1) {
        let fi = f32(i);
        let start = fi * 0.21;
        let local = (pr - start) / 0.58;
        if local < 0.0 || local > 1.0 {
            continue;
        }
        let eased = 1.0 - pow(1.0 - local, 2.0);
        let ring_r = eased * 0.7;
        let half_t = 0.025 + eased * 0.05;
        let edge = 1.0 - smoothstep(half_t * 0.4, half_t, abs(d - ring_r));
        let ring_life = 1.0 - local;
        acc = acc + edge * ring_life;
    }
    return clamp(acc, 0.0, 1.0);
}

fn glow(p: vec2<f32>, origin: vec2<f32>, pr: f32) -> f32 {
    let d = length(p - origin);
    // Radial Gaussian centred on origin, broadens slightly as
    // it fades so it reads as energy "releasing" outward.
    let sigma = 0.09 + pr * 0.05;
    let g = exp(-(d * d) / (2.0 * sigma * sigma));
    // Sharp attack (peak near pr=0.08), then a longer decay
    // tail. Uses two smoothsteps multiplied so the curve has
    // a clean rise-and-fall shape with no plateau.
    let attack = smoothstep(0.0, 0.08, pr);
    let decay = 1.0 - smoothstep(0.08, 0.95, pr);
    return g * attack * decay;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Flip Y: canvas convention has +Y down, but clip space
    // has +Y up. Rust computes origin in canvas convention,
    // so we match by flipping the fragment's UV here.
    let p = vec2<f32>(in.uv.x, -in.uv.y);
    if length(p) > 1.05 {
        // Tiny halo past the disc edge for sparks that travel
        // slightly outside the menu radius. Beyond this the
        // sparks would clip the window edge anyway.
        discard;
    }

    var contribution: f32 = 0.0;
    if u.style == 0u {
        contribution = sparks(p, u.origin, u.progress);
    } else if u.style == 1u {
        contribution = shockwave(p, u.origin, u.progress);
    } else {
        contribution = glow(p, u.origin, u.progress);
    }

    let alpha = u.intensity * contribution;
    return vec4<f32>(u.color.rgb * alpha, alpha);
}
