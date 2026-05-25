//! Haptic-ripple shader — a one-shot expanding ring kicked off
//! every time a haptic event fires. Visual + physical feedback
//! synchronised: the user feels the click *and* sees a soft
//! pulse from the menu centre.
//!
//! Same three-piece structure as the aurora backdrop:
//!
//!   * [`RippleProgram`] — `iced::widget::shader::Program` impl,
//!     stamped fresh each frame with progress + colour.
//!   * [`RipplePrimitive`] — per-frame render command.
//!   * [`RipplePipeline`] — lazily-instantiated render pipeline
//!     shared across all primitive instances.
//!
//! The shader is a single triangle + a fragment that paints a
//! thin SDF-shaded ring, expanding outward from the centre as
//! `progress` ramps 0 → 1.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct RippleUniformsRaw {
    /// 0..=1 — drives the ring's outward expansion + alpha decay.
    progress: f32,
    /// Strength multiplier 0..=1 from settings.
    intensity: f32,
    _pad0: [f32; 2],
    /// Ripple ring colour (typically the active accent).
    color: [f32; 4],
}

pub struct RippleProgram {
    progress: f32,
    intensity: f32,
    color: [f32; 4],
}

impl RippleProgram {
    pub fn new(progress: f32, intensity: f32, color: [f32; 4]) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            intensity: intensity.clamp(0.0, 1.0),
            color,
        }
    }
}

impl<Message> shader::Program<Message> for RippleProgram {
    type State = ();
    type Primitive = RipplePrimitive;

    fn update(
        &self,
        _state: &mut (),
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        // Mid-ripple → keep redrawing until the radial state
        // clears `ripple_started`. The widget will be removed
        // from the tree once that happens, so this just
        // guarantees frame-rate while the ripple is alive.
        Some(shader::Action::request_redraw())
    }

    fn draw(
        &self,
        _state: &(),
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        RipplePrimitive {
            uniforms: RippleUniformsRaw {
                progress: self.progress,
                intensity: self.intensity,
                _pad0: [0.0; 2],
                color: self.color,
            },
        }
    }
}

#[derive(Debug)]
pub struct RipplePrimitive {
    uniforms: RippleUniformsRaw,
}

impl Primitive for RipplePrimitive {
    type Pipeline = RipplePipeline;

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

const SHADER_SOURCE: &str = include_str!("ripple.wgsl");

pub struct RipplePipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for RipplePipeline {
    fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ripple.uniforms"),
            size: std::mem::size_of::<RippleUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ripple.bgl"),
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
            label: Some("ripple.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ripple.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ripple.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ripple.pipeline"),
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
