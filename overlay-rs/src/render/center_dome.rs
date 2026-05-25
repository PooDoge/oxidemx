//! Centre dome — paints directional Lambert + Phong specular
//! over the centre puck so it reads as a 3D sphere instead of a
//! flat circle. Always-on overlay; the canvas keeps painting the
//! puck's base colour underneath, the shader just adds lighting.
//!
//! See render/center_dome.wgsl for the per-pixel sphere math.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CenterDomeUniformsRaw {
    radius: f32,
    intensity: f32,
    light_angle: f32,
    shininess: f32,
    rim_brightness: f32,
    shadow_amount: f32,
    _pad0: [f32; 2],
    specular_color: [f32; 4],
}

pub struct CenterDomeProgram {
    pub radius: f32,
    pub intensity: f32,
    pub light_angle: f32,
    pub shininess: f32,
    pub rim_brightness: f32,
    pub shadow_amount: f32,
    pub specular_color: [f32; 4],
}

impl<Message> shader::Program<Message> for CenterDomeProgram {
    type State = ();
    type Primitive = CenterDomePrimitive;

    fn update(
        &self,
        _state: &mut (),
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        Some(shader::Action::request_redraw())
    }

    fn draw(
        &self,
        _state: &(),
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        CenterDomePrimitive {
            uniforms: CenterDomeUniformsRaw {
                radius: self.radius.max(0.001),
                intensity: self.intensity.clamp(0.0, 1.0),
                light_angle: self.light_angle,
                shininess: self.shininess.max(1.0),
                rim_brightness: self.rim_brightness.clamp(0.0, 1.0),
                shadow_amount: self.shadow_amount.clamp(0.0, 1.0),
                _pad0: [0.0; 2],
                specular_color: self.specular_color,
            },
        }
    }
}

#[derive(Debug)]
pub struct CenterDomePrimitive {
    uniforms: CenterDomeUniformsRaw,
}

impl Primitive for CenterDomePrimitive {
    type Pipeline = CenterDomePipeline;

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

const SHADER_SOURCE: &str = include_str!("center_dome.wgsl");

pub struct CenterDomePipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for CenterDomePipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("center_dome.uniforms"),
            size: std::mem::size_of::<CenterDomeUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("center_dome.bgl"),
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
            label: Some("center_dome.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("center_dome.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("center_dome.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("center_dome.pipeline"),
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
