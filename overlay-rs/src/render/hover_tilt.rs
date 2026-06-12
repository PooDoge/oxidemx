//! Parallax-tilt shader — paints a directional specular highlight +
//! soft inset shadow inside the hovered wedge. The hover_glow
//! shader handles the OUTER aura along the wedge boundary; this
//! one handles the INNER lighting across the wedge surface, giving
//! the slice a "tilted button" feel.
//!
//! Design — see render/hover_tilt.wgsl for the per-pixel math:
//!  * Inside-wedge mask via the same SDF as hover_glow.
//!  * Specular dot at cursor position (gaussian falloff, tightness
//!    controlled by `sharpness`).
//!  * Side-wash + shadow based on the dot product of (frag-from-
//!    wedge-centre) with (cursor-from-wedge-centre).
//!
//! Same Pipeline/Primitive/Program shape as the other shaders.
//! Shared cursor uniform comes from RadialState's `pointer_dx /
//! pointer_dy`, normalised to clip-space [-1, 1] in app.rs.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct HoverTiltUniformsRaw {
    bisector_rad: f32,
    half_sweep: f32,
    inner_r: f32,
    outer_r: f32,
    progress: f32,
    intensity: f32,
    shadow_amount: f32,
    sharpness: f32,
    cursor_uv: [f32; 2],
    _pad0: [f32; 2],
    highlight_color: [f32; 4],
    accent_color: [f32; 4],
}

pub struct HoverTiltProgram {
    pub bisector_rad: f32,
    pub half_sweep: f32,
    pub inner_r: f32,
    pub outer_r: f32,
    pub progress: f32,
    pub intensity: f32,
    pub shadow_amount: f32,
    pub sharpness: f32,
    pub cursor_uv: [f32; 2],
    pub highlight_color: [f32; 4],
    pub accent_color: [f32; 4],
}

impl<Message> shader::Program<Message> for HoverTiltProgram {
    type State = ();
    type Primitive = HoverTiltPrimitive;

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
        HoverTiltPrimitive {
            uniforms: HoverTiltUniformsRaw {
                bisector_rad: self.bisector_rad,
                half_sweep: self.half_sweep,
                inner_r: self.inner_r,
                outer_r: self.outer_r,
                progress: self.progress.clamp(0.0, 1.0),
                intensity: self.intensity.clamp(0.0, 1.0),
                shadow_amount: self.shadow_amount.clamp(0.0, 1.0),
                sharpness: self.sharpness.clamp(0.0, 1.0),
                cursor_uv: self.cursor_uv,
                _pad0: [0.0; 2],
                highlight_color: self.highlight_color,
                accent_color: self.accent_color,
            },
        }
    }
}

#[derive(Debug)]
pub struct HoverTiltPrimitive {
    uniforms: HoverTiltUniformsRaw,
}

impl Primitive for HoverTiltPrimitive {
    type Pipeline = HoverTiltPipeline;

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

const SHADER_SOURCE: &str = include_str!("hover_tilt.wgsl");

pub struct HoverTiltPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for HoverTiltPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hover_tilt.uniforms"),
            size: std::mem::size_of::<HoverTiltUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hover_tilt.bgl"),
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
            label: Some("hover_tilt.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hover_tilt.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hover_tilt.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hover_tilt.pipeline"),
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
