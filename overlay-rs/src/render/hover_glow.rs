//! SDF hover glow — wgpu shader that paints a soft glowing outline
//! around the hovered slice's wedge boundary using a signed
//! distance field.
//!
//! Replaces the canvas-side glow ring (a few pixels of alpha-
//! blended white circle around the icon) with proper SDF math:
//!   * Distance to wedge boundary computed analytically per
//!     pixel
//!   * Alpha falls off as `exp(-d / sigma)` so the glow is
//!     soft without ever being aliased
//!   * Coloured by the active palette accent
//!
//! Same Program/Primitive/Pipeline structure as the aurora and
//! ripple shaders. Stateless — re-renders every frame from
//! current target_slice + highlight tween + slot count.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct HoverGlowUniformsRaw {
    /// Bisector of the hovered slice in radians, measured the
    /// same way the canvas does (`-π/2` = 12 o'clock).
    bisector_rad: f32,
    /// Half-sweep of the wedge in radians (= `π / slot_count`).
    half_sweep: f32,
    /// Inner ring radius, normalised to the half-extent (so 1.0
    /// = the disc's edge).
    inner_r: f32,
    /// Outer ring radius, normalised the same way.
    outer_r: f32,
    /// Hover progress 0..=1 from the highlight tween. 0 = no
    /// glow, 1 = full glow.
    progress: f32,
    /// User-tunable strength.
    intensity: f32,
    _pad0: [f32; 2],
    /// Glow colour (typically the active palette accent or the
    /// slice's own colour).
    color: [f32; 4],
}

pub struct HoverGlowProgram {
    pub bisector_rad: f32,
    pub half_sweep: f32,
    pub inner_r: f32,
    pub outer_r: f32,
    pub progress: f32,
    pub intensity: f32,
    pub color: [f32; 4],
}

impl<Message> shader::Program<Message> for HoverGlowProgram {
    type State = ();
    type Primitive = HoverGlowPrimitive;

    fn update(
        &self,
        _state: &mut (),
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        Some(shader::Action::request_redraw())
    }

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Self::Primitive {
        HoverGlowPrimitive {
            uniforms: HoverGlowUniformsRaw {
                bisector_rad: self.bisector_rad,
                half_sweep: self.half_sweep,
                inner_r: self.inner_r,
                outer_r: self.outer_r,
                progress: self.progress.clamp(0.0, 1.0),
                intensity: self.intensity.clamp(0.0, 1.0),
                _pad0: [0.0; 2],
                color: self.color,
            },
        }
    }
}

#[derive(Debug)]
pub struct HoverGlowPrimitive {
    uniforms: HoverGlowUniformsRaw,
}

impl Primitive for HoverGlowPrimitive {
    type Pipeline = HoverGlowPipeline;

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

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        true
    }
}

const SHADER_SOURCE: &str = include_str!("hover_glow.wgsl");

pub struct HoverGlowPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for HoverGlowPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hover_glow.uniforms"),
            size: std::mem::size_of::<HoverGlowUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hover_glow.bgl"),
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
            label: Some("hover_glow.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hover_glow.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hover_glow.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hover_glow.pipeline"),
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
