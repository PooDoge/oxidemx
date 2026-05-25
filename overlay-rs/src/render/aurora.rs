//! Aurora backdrop — a wgpu shader that paints an animated
//! conic gradient swirl behind the radial menu using the active
//! theme's accents. Layered behind the canvas widget via
//! `iced::widget::Stack` so it composites cleanly with the
//! existing wedge/icon/tooltip render path without touching it.
//!
//! ## Adding new shader effects: read this first
//!
//! Hard-won lessons from this + the ripple + the hover glow.
//! Shortcuts here paid for in real debugging time.
//!
//! 1. **Use clip-space UV from the vertex stage**, not
//!    `@builtin(position)` in the fragment. Position is in
//!    framebuffer pixels (= logical_px × HiDPI scale), so any
//!    hardcoded "menu is 484×484" works only at 100 % display
//!    scale. Vertex stage gets clip-space coords whose
//!    `[-1, 1]²` always maps to the visible viewport.
//!
//! 2. **Clip space has +Y up; iced canvas has +Y down**. For
//!    direction-sensitive shaders (anything taking a
//!    `bisector_rad` from the canvas side), flip `y` in the
//!    fragment before computing `atan2`. Skipping it gives a
//!    180° rotation. Radially-symmetric effects (aurora,
//!    ripple) don't care — skip the flip + save one ALU op.
//!
//! 3. **iced 0.14 has a strong-typed `Pipeline` trait** —
//!    don't follow the halo example's `Storage`+`dyn Any`
//!    pattern (older iced API). `Primitive::prepare` takes
//!    `&mut Self::Pipeline` directly; iced auto-instantiates
//!    the pipeline once and hands it back typed.
//!
//! 4. **wgpu + bytemuck versions pin to `iced_wgpu`'s** —
//!    bumping iced means bumping these in lockstep, otherwise
//!    ABI breakage at runtime.
//!
//! 5. **Stack effects with `iced::widget::Stack`** — don't
//!    replace the canvas. Skip a layer when intensity = 0 so
//!    disabled effects cost zero GPU.
//!
//! 6. **Always 0-disables intensity slider per effect** —
//!    GPU work isn't free on integrated graphics / battery
//!    laptops.
//!
//! Architecture mirrors the upstream iced + bungoboingo/halo
//! example, adapted to iced 0.14's strongly-typed Pipeline trait
//! and our overlay's render loop:
//!
//!   * [`AuroraProgram`] is the user-facing widget, implements
//!     `iced::widget::shader::Program<Message>`. Owns the time
//!     origin + the colour/intensity uniforms.
//!   * [`AuroraPrimitive`] is the per-frame render command. It
//!     holds the uniforms and forwards them to the pipeline.
//!   * [`AuroraPipeline`] is iced's lazily-instantiated render
//!     pipeline (one per primitive type, shared across all
//!     instances). Owns the wgpu buffer + bind group + render
//!     pipeline, and exposes `write_uniforms` + `draw_in` for
//!     the primitive to invoke.
//!
//! The shader itself is a single full-screen triangle fed three
//! palette-derived colours and a time scalar. Cheap on any GPU:
//! one draw call, ~30 ALU ops per pixel.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};
use std::time::Instant;

/// Per-frame parameters fed to the shader. Layout matches the
/// `Uniforms` block in `aurora.wgsl` — keep them in sync.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct AuroraUniformsRaw {
    /// Seconds since menu open. Drives all motion in the shader.
    time: f32,
    /// User-tunable strength multiplier 0..=1. 0 hides the
    /// effect; 1 is the full default look.
    intensity: f32,
    /// Padding so the next vec4 is 16-byte aligned.
    _pad0: [f32; 2],
    /// Three palette-derived RGBA colours blended in conic
    /// fashion. `.a` is unused (we composite via the alpha
    /// computed in the shader) but kept so the layout stays a
    /// clean `vec4`.
    accent: [f32; 4],
    accent2: [f32; 4],
    accent_dim: [f32; 4],
    /// Menu-element translate / rotate / flip. Lets the aurora
    /// follow the canvas when the user adds a translate/rotate
    /// custom track to the menu element. Identity unless tracks
    /// are set; see render/animation.rs::MenuXformRaw.
    menu_xform: super::animation::MenuXformRaw,
}

/// Iced widget program. Stamped from the radial render path each
/// frame with a fresh palette + intensity.
pub struct AuroraProgram {
    start: Instant,
    accent: [f32; 4],
    accent2: [f32; 4],
    accent_dim: [f32; 4],
    intensity: f32,
    menu_xform: super::animation::MenuXformRaw,
}

impl AuroraProgram {
    pub fn new(
        start: Instant,
        accent: [f32; 4],
        accent2: [f32; 4],
        accent_dim: [f32; 4],
        intensity: f32,
        menu_xform: super::animation::MenuXformRaw,
    ) -> Self {
        Self {
            start,
            accent,
            accent2,
            accent_dim,
            intensity: intensity.clamp(0.0, 1.0),
            menu_xform,
        }
    }
}

impl<Message> shader::Program<Message> for AuroraProgram {
    type State = ();
    type Primitive = AuroraPrimitive;

    fn update(
        &self,
        _state: &mut (),
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        // Always request the next frame so time-driven animation
        // keeps stepping. The overlay's existing 60 fps tick keeps
        // the canvas redrawing too — the redraw request here is
        // belt-and-suspenders for headless / paused-canvas cases.
        Some(shader::Action::request_redraw())
    }

    fn draw(
        &self,
        _state: &(),
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        AuroraPrimitive {
            uniforms: AuroraUniformsRaw {
                time: self.start.elapsed().as_secs_f32(),
                intensity: self.intensity,
                _pad0: [0.0; 2],
                accent: self.accent,
                accent2: self.accent2,
                accent_dim: self.accent_dim,
                menu_xform: self.menu_xform,
            },
        }
    }
}

#[derive(Debug)]
pub struct AuroraPrimitive {
    uniforms: AuroraUniformsRaw,
}

impl Primitive for AuroraPrimitive {
    type Pipeline = AuroraPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        pipeline.write_uniforms(queue, &self.uniforms);
    }

    fn draw(
        &self,
        pipeline: &Self::Pipeline,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        // The render pass already has its viewport + scissor set
        // to our widget's bounds (handled by iced before this
        // call), so a clip-space full-screen triangle covers
        // exactly the widget pixels.
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        true
    }
}

const SHADER_SOURCE: &str = include_str!("aurora.wgsl");

pub struct AuroraPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl AuroraPipeline {
    fn write_uniforms(&self, queue: &wgpu::Queue, raw: &AuroraUniformsRaw) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(raw));
    }
}

impl iced::widget::shader::Pipeline for AuroraPipeline {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aurora.uniforms"),
            size: std::mem::size_of::<AuroraUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aurora.bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aurora.bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aurora.pipeline_layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let shader_module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("aurora.shader"),
                source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(
                    SHADER_SOURCE,
                )),
            });
        let pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("aurora.pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader_module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        // Pre-multiplied alpha blending — iced's
                        // surface alpha mode is PreMultiplied per
                        // the wgpu compositor selection at boot
                        // (see overlay startup logs).
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview: None,
                cache: None,
            });
        Self {
            uniforms,
            bind_group,
            pipeline,
        }
    }
}
