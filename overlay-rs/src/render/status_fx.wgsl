// AI-chat status effects — one über-shader with a `mode` switch so
// every effect shares a single pipeline + uniform buffer (iced
// stores custom-shader pipelines per TYPE; N effect types would
// mean N pipelines and reintroduce the uniform-clobber footgun the
// ChatAurora newtype exists to avoid).
//
// Modes (keep in sync with `AiFxConfig::mode_index`):
//   1 = soft glow          4 = cyber grid + sun
//   2 = warping starfield  5 = plasma
//   3 = iridescent fibers  6 = pulse rings
//
// Colour inputs c0/c1/c2 default to the theme's accent / accent2 /
// accent_dim (or the user's custom hex overrides), so every effect
// adapts to the active theme.
//
// Conventions (see aurora.wgsl's header for the full story):
//   * uv from the vertex stage spans [-1, 1]² over the widget —
//     never use @builtin(position) (framebuffer px, breaks HiDPI).
//   * Clip space is +Y up; we convert to canvas space (+Y down)
//     once at the top of fs_main so "bottom of the window" is +y.
//   * Output is premultiplied alpha.

struct Uniforms {
    time: f32,
    intensity: f32,
    speed: f32,
    mode: f32,
    // Widget aspect ratio (width / height) so radial effects stay
    // circular in the non-square chat window.
    aspect: f32,
    // Scalar pads, NOT vec3: a vec3 is 16-byte aligned in WGSL,
    // which would push c0 to offset 48 and the struct to 96 bytes
    // while the host-side Pod struct is 80 — wgpu rejects the
    // bind at draw time.
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    c0: vec4<f32>,
    c1: vec4<f32>,
    c2: vec4<f32>,
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

const TAU: f32 = 6.28318530718;

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

/// Theme-adaptive replacement for the classic IQ cosine palette:
/// blends the three configured colours at 120° phase offsets, so
/// any `t` walks a smooth loop through the theme's accents.
fn themed_palette(t: f32) -> vec3<f32> {
    let w0 = 0.5 + 0.5 * cos(TAU * t);
    let w1 = 0.5 + 0.5 * cos(TAU * t + 2.0944);
    let w2 = 0.5 + 0.5 * cos(TAU * t + 4.18879);
    let sum = max(w0 + w1 + w2, 1e-3);
    return (u.c0.rgb * w0 + u.c1.rgb * w1 + u.c2.rgb * w2) / sum;
}

// ---- mode 1: soft glow -----------------------------------------------------
// A breathing radial wash — the quietest cue. Reads as "the window
// is alive / wants your attention" without any structure to parse.
fn fx_glow(pa: vec2<f32>, t: f32) -> vec4<f32> {
    let r = length(pa);
    let breathe = 0.6 + 0.4 * sin(t * 1.7);
    let g = (1.0 - smoothstep(0.0, 1.45, r)) * breathe;
    let col = mix(u.c1.rgb, u.c0.rgb, clamp(1.0 - r * 0.7, 0.0, 1.0));
    return vec4<f32>(col, g * g * 0.6);
}

// ---- mode 2: warping starfield ---------------------------------------------
// Streaks racing outward from the centre — hyperspace. Three depth
// layers of angular cells, each star a short radial streak whose
// position/phase comes from a per-cell hash.
fn fx_starfield(pa: vec2<f32>, t: f32) -> vec4<f32> {
    let r = max(length(pa), 1e-3);
    let ang = atan2(pa.y, pa.x) / TAU + 0.5; // 0..1
    var col = vec3<f32>(0.0);
    var acc = 0.0;
    for (var l = 0; l < 3; l++) {
        let fl = f32(l);
        let n = 28.0 + fl * 20.0;
        let cell = floor(ang * n);
        let h = hash21(vec2<f32>(cell, fl * 17.0));
        // Depth races 1 → 0 (far → near); streak brightens as it
        // approaches, then fades right at the edge.
        let depth = fract(h + t * (0.22 + 0.1 * fl));
        let near = 1.0 - depth;
        // Radial band the streak occupies at this instant — wide
        // enough to read as a travelling streak, not a speck.
        let band = smoothstep(0.5, 0.0, abs(r - near * 1.3) / (0.10 + 0.35 * near));
        // Angular sharpness — tighter when far, slightly wider near.
        let af = fract(ang * n) - 0.5;
        let wedge = smoothstep(0.5, 0.0, abs(af) * (5.0 - 2.5 * near));
        let b = band * wedge * smoothstep(0.0, 0.3, r) * (0.5 + 1.5 * near);
        col += mix(u.c0.rgb, vec3<f32>(1.0), near * 0.7) * b;
        acc += b;
    }
    return vec4<f32>(col * 1.6, clamp(acc * 1.6, 0.0, 1.0));
}

// ---- mode 3: iridescent fibers ---------------------------------------------
// Layered drifting sine filaments, each tinted by the themed
// palette — the user-contributed wave shader, adapted.
fn fx_fibers(pa: vec2<f32>, t: f32) -> vec4<f32> {
    var col = vec3<f32>(0.0);
    var acc = 0.0;
    for (var l = 0; l < 10; l++) {
        let layer = f32(l) * 0.1;
        let amp = 0.25 + 0.25 * sin(t + layer) * (1.0 - layer);
        let x = pa.x - t * (1.0 - layer) * 0.6;
        let y = pa.y + amp * sin(2.0 * x);
        let thick = 0.045 + 0.02 * sin(layer * 9.0 + 1.0);
        let bright = max(0.0, 1.0 - abs(y) / thick);
        let hue = themed_palette(0.5 * pa.x + layer - 0.5 * t * 0.4);
        col += bright * bright * hue * 0.55;
        acc += bright * 0.35;
    }
    return vec4<f32>(col, clamp(acc, 0.0, 0.9));
}

// ---- mode 4: cyber grid + sun ----------------------------------------------
// Synthwave horizon: perspective grid scrolling toward the viewer
// below the horizon line, striped sun above it. Canvas space → +y
// is the bottom of the window. Adapted from Jan Mróz's CC-BY 3.0
// "sun & grid" Shadertoy, re-themed to the palette.
fn fx_gridsun(pa: vec2<f32>, t: f32) -> vec4<f32> {
    let horizon = 0.18;
    var col = vec3<f32>(0.0);
    var a = 0.0;
    if pa.y > horizon {
        // Grid floor. Line thickness derives from the PERSPECTIVE
        // depth (pre-scroll g.y) — folding the time scroll in first
        // makes the thickness grow without bound until the whole
        // floor renders solid after a few seconds.
        var g = vec2<f32>(0.0);
        g.y = 3.0 / (abs(pa.y - horizon) + 0.05);
        g.x = pa.x * g.y;
        let size = vec2<f32>(g.y, g.y * g.y * 0.2) * 0.01;
        g.y += t * 2.4;
        let cell = abs(fract(g) - vec2<f32>(0.5));
        let lines = smoothstep(size, vec2<f32>(0.0), cell);
        let gv = clamp(lines.x + lines.y, 0.0, 1.2);
        col = u.c0.rgb * gv;
        a = gv * 0.8;
    } else {
        // Sun — centred horizontally, sitting on the horizon.
        let su = vec2<f32>(pa.x, pa.y - horizon + 0.34);
        let d = length(su);
        let disc = smoothstep(0.31, 0.295, d);
        let bloom = smoothstep(0.65, 0.0, d) * 0.5;
        // Solid crown, animated stripe cuts toward the horizon.
        let cut = clamp(
            3.0 * sin(su.y * 85.0 + t * 1.6) + clamp(-su.y * 16.0 + 1.0, -6.0, 6.0),
            0.0,
            1.0,
        );
        let sun_col = mix(u.c0.rgb, u.c1.rgb, clamp(-su.y * 2.2 + 0.6, 0.0, 1.0));
        col = sun_col * (disc * cut) + sun_col * bloom;
        a = max(disc * cut, bloom);
    }
    // Horizon fog knits the halves together.
    let fog = smoothstep(0.14, 0.0, abs(pa.y - horizon));
    col += fog * fog * u.c1.rgb * 0.5;
    a = max(a, fog * fog * 0.5);
    return vec4<f32>(col, a);
}

// ---- mode 5: plasma ----------------------------------------------------------
// Classic additive-sine plasma walked through the themed palette.
fn fx_plasma(pa: vec2<f32>, t: f32) -> vec4<f32> {
    let v = sin(pa.x * 3.1 + t)
        + sin((pa.x + pa.y) * 2.3 - t * 0.7)
        + sin(length(pa) * 4.2 - t * 1.3)
        + sin(pa.y * 2.7 + t * 0.9);
    let hue = themed_palette(v * 0.12 + t * 0.015);
    let a = 0.30 + 0.12 * sin(v * 1.5);
    return vec4<f32>(hue, clamp(a, 0.0, 0.6));
}

// ---- mode 6: pulse rings -----------------------------------------------------
// Concentric crests expanding from the centre — a sonar "working
// on it" read that's calmer than the starfield.
fn fx_rings(pa: vec2<f32>, t: f32) -> vec4<f32> {
    let r = length(pa);
    let crest = sin(r * 9.0 - t * 2.6);
    let ring = smoothstep(0.78, 1.0, crest);
    let fade = 1.0 - smoothstep(0.15, 1.35, r);
    let col = mix(u.c0.rgb, u.c1.rgb, 0.5 + 0.5 * sin(r * 4.0 - t * 1.3));
    return vec4<f32>(col, ring * fade * 0.55);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Canvas convention: +Y down (see header). Aspect-correct X so
    // radial effects stay circular in the non-square chat window.
    let p = vec2<f32>(in.uv.x, -in.uv.y);
    let pa = vec2<f32>(p.x * max(u.aspect, 0.1), p.y);
    let t = u.time * u.speed;

    var c: vec4<f32>;
    let mode = i32(round(u.mode));
    switch mode {
        case 1: { c = fx_glow(pa, t); }
        case 2: { c = fx_starfield(pa, t); }
        case 3: { c = fx_fibers(pa, t); }
        case 4: { c = fx_gridsun(pa, t); }
        case 5: { c = fx_plasma(pa, t); }
        case 6: { c = fx_rings(pa, t); }
        default: { c = vec4<f32>(0.0); }
    }

    // Soft edge mask so nothing paints into the window body's
    // 24 px rounded corners or hard-cuts at the bounds.
    let m = (1.0 - smoothstep(0.90, 1.0, abs(in.uv.x)))
        * (1.0 - smoothstep(0.90, 1.0, abs(in.uv.y)));

    let alpha = clamp(c.a, 0.0, 1.0) * u.intensity * m;
    return vec4<f32>(c.rgb * alpha, alpha);
}
