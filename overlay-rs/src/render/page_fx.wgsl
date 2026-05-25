// Page-transition overlay shaders. Two looks selected by
// `style`: 0 = Dissolve, 1 = Plasma. Both run on top of the
// canvas's existing two-ring crossfade — the shader adds visual
// flair, doesn't replace the underlying transition.

struct Uniforms {
    progress: f32,
    style: u32,
    intensity: f32,
    _pad0: f32,
    // Style-specific params packed into one vec4 — see
    // `PageFxUniformsRaw::params0` in render/page_fx.rs for
    // the ordering. Keep these reads consistent with that.
    //   params0.x = dissolve_noise_scale
    //   params0.y = dissolve_band_softness
    //   params0.z = plasma_wave_scale
    //   params0.w = plasma_wave_speed
    params0: vec4<f32>,
    color_a: vec4<f32>,
    color_b: vec4<f32>,
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

// Cheap deterministic noise — enough for a dissolve mask.
// Hash-then-fract keeps us GPU-only; no texture lookup.
fn hash21(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453);
}

// Smooth value noise (2-octave) for the plasma pattern. Cheap,
// not seamlessly tileable but we don't tile it — just paint it
// once per frame within the menu disc.
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Dissolve: noise threshold sweep. The mask is a noise field;
// pixels whose noise value is "near" the current progress glow.
// Reads as scattered particles fading in then out as the
// transition moves through.
fn fx_dissolve(uv: vec2<f32>) -> vec4<f32> {
    // Triangle wave on progress: ramp 0→1→0 over the transition
    // so the dissolve peaks at progress=0.5 and fades at the
    // edges. Without this it would just steadily dissolve and
    // never recover.
    let pulse = 1.0 - abs(2.0 * u.progress - 1.0);
    let noise = vnoise(uv * u.params0.x);
    // Soft band around the noise threshold so each pixel
    // smoothly fades in and out.
    let band = 1.0 - smoothstep(0.0, u.params0.y, abs(noise - pulse));
    // Two-tone: pixels above the threshold lean color_a, below
    // lean color_b. Ramp keeps them from being identical.
    let mix_t = step(pulse, noise);
    let col = mix(u.color_a.rgb, u.color_b.rgb, mix_t);
    let alpha = band * pulse * 0.7;
    return vec4<f32>(col * alpha, alpha);
}

// Plasma: classic 70s-demo plasma — sum of sin/cos waves at
// different frequencies. Phase advances with progress so the
// pattern looks like it's actively animating during the swap.
fn fx_plasma(uv: vec2<f32>) -> vec4<f32> {
    let pulse = 1.0 - abs(2.0 * u.progress - 1.0);
    // 2π over one transition × user-tunable speed multiplier
    // so users can dial the swirl rate.
    let phase = u.progress * 6.2831853 * u.params0.w;
    let s = u.params0.z;
    let v =
        sin(uv.x * s + phase)
        + sin(uv.y * s - phase * 1.3)
        + sin((uv.x + uv.y) * (s * 0.75) + phase * 0.7)
        + sin(length(uv) * (s * 2.0) - phase * 1.5);
    // v ranges roughly [-4, 4]. Map to [0, 1].
    let t = 0.5 + v * 0.125;
    let col = mix(u.color_a.rgb, u.color_b.rgb, clamp(t, 0.0, 1.0));
    // Cap alpha and ramp it via the triangle pulse so the
    // plasma fades in + out at the edges of the transition.
    let alpha = pulse * 0.55;
    return vec4<f32>(col * alpha, alpha);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let r = length(in.uv);
    if r > 1.0 {
        discard;
    }

    // Confine the effect to the disc with a soft inner+outer
    // fade so it doesn't paint the centre puck or cut hard at
    // the edge. Same treatment as the aurora's centre/edge
    // fades.
    let edge = 1.0 - smoothstep(0.65, 0.98, r);
    let centre_fade = smoothstep(0.18, 0.32, r);

    var rgba: vec4<f32>;
    if u.style == 0u {
        rgba = fx_dissolve(in.uv);
    } else {
        rgba = fx_plasma(in.uv);
    }

    let mask = u.intensity * edge * centre_fade;
    return vec4<f32>(rgba.rgb * mask, rgba.a * mask);
}
