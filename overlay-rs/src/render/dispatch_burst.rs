//! Dispatch-burst shader — a one-shot flourish anchored at the
//! activated slice when the user fires a slice. Three styles
//! (Sparks / Shockwave / Glow) share the same pipeline; the
//! fragment branches on the `style` uniform so adding new
//! variants stays cheap.
//!
//! Same three-piece structure as the other render-time shaders
//! in this directory:
//!
//!   * [`DispatchBurstProgram`] — `iced::widget::shader::Program`
//!     impl, stamped fresh each frame with progress + slice
//!     anchor + colour.
//!   * [`DispatchBurstPrimitive`] — per-frame render command.
//!   * [`DispatchBurstPipeline`] — lazily-instantiated render
//!     pipeline shared across all primitive instances.
//!
//! Origin convention: `[x, y]` in normalised half-extent units,
//! canvas convention (+Y down). Computed in Rust from the
//! activated slice's index + slot count via [`slice_origin`] so
//! the WGSL doesn't have to re-derive the geometry.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

/// Numeric discriminator the WGSL fragment branches on. Keep
/// this in sync with the `if u.style == 0u` chain in the
/// shader.
#[repr(u32)]
#[derive(Copy, Clone, Debug)]
pub enum DispatchBurstStyleGpu {
    Sparks = 0,
    Shockwave = 1,
    Glow = 2,
}

impl From<juhradial_shared::DispatchBurstStyle> for DispatchBurstStyleGpu {
    fn from(s: juhradial_shared::DispatchBurstStyle) -> Self {
        use juhradial_shared::DispatchBurstStyle as S;
        match s {
            S::Sparks => DispatchBurstStyleGpu::Sparks,
            S::Shockwave => DispatchBurstStyleGpu::Shockwave,
            S::Glow => DispatchBurstStyleGpu::Glow,
        }
    }
}

/// Compute the origin point a slice's burst should anchor at,
/// in normalised half-extent units (canvas convention, +Y
/// down). Slice 0 sits at the top, indices increment clockwise.
/// The 0.6 factor places the anchor at the icon, which sits
/// between the inner and outer rim of the wedge.
pub fn slice_origin(slot_idx: usize, slot_count: usize) -> [f32; 2] {
    let n = slot_count.max(1) as f32;
    let angle =
        (slot_idx as f32) * std::f32::consts::TAU / n - std::f32::consts::FRAC_PI_2;
    let r = 0.6;
    [r * angle.cos(), r * angle.sin()]
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct DispatchBurstUniformsRaw {
    progress: f32,
    intensity: f32,
    style: u32,
    _pad0: f32,
    origin: [f32; 2],
    _pad1: [f32; 2],
    color: [f32; 4],
}

pub struct DispatchBurstProgram {
    progress: f32,
    intensity: f32,
    style: DispatchBurstStyleGpu,
    origin: [f32; 2],
    color: [f32; 4],
}

impl DispatchBurstProgram {
    pub fn new(
        progress: f32,
        intensity: f32,
        style: DispatchBurstStyleGpu,
        origin: [f32; 2],
        color: [f32; 4],
    ) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            intensity: intensity.clamp(0.0, 1.0),
            style,
            origin,
            color,
        }
    }
}

impl<Message> shader::Program<Message> for DispatchBurstProgram {
    type State = ();
    type Primitive = DispatchBurstPrimitive;

    fn update(
        &self,
        _state: &mut (),
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        // Keep ticking until the radial state clears
        // `dispatch_started`. The widget is removed from the
        // tree the moment the duration elapses, so this is
        // bounded.
        Some(shader::Action::request_redraw())
    }

    fn draw(
        &self,
        _state: &(),
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        DispatchBurstPrimitive {
            uniforms: DispatchBurstUniformsRaw {
                progress: self.progress,
                intensity: self.intensity,
                style: self.style as u32,
                _pad0: 0.0,
                origin: self.origin,
                _pad1: [0.0; 2],
                color: self.color,
            },
        }
    }
}

#[derive(Debug)]
pub struct DispatchBurstPrimitive {
    uniforms: DispatchBurstUniformsRaw,
}

impl Primitive for DispatchBurstPrimitive {
    type Pipeline = DispatchBurstPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        queue.write_buffer(&pipeline.uniforms, 0, bytemuck::bytes_of(&self.uniforms));
    }

    fn draw(
        &self,
        pipeline: &Self::Pipeline,
        render_pass: &mut wgpu::RenderPass<'_>,
    ) -> bool {
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        true
    }
}

const SHADER_SOURCE: &str = include_str!("dispatch_burst.wgsl");

pub struct DispatchBurstPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for DispatchBurstPipeline {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dispatch_burst.uniforms"),
            size: std::mem::size_of::<DispatchBurstUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dispatch_burst.bgl"),
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
            label: Some("dispatch_burst.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dispatch_burst.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dispatch_burst.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("dispatch_burst.pipeline"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
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
