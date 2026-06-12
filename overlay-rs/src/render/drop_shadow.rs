//! Drop shadow — paints a soft falloff shadow OUTSIDE the disc,
//! offset away from the light source so the menu reads as a
//! physical floating object casting a real cast shadow.
//!
//! See render/drop_shadow.wgsl for the per-pixel math.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct DropShadowUniformsRaw {
    outer_r: f32,
    intensity: f32,
    spread: f32,
    falloff: f32,
    light_angle: f32,
    offset_dist: f32,
    _pad0: [f32; 2],
    shadow_color: [f32; 4],
}

pub struct DropShadowProgram {
    pub outer_r: f32,
    pub intensity: f32,
    pub spread: f32,
    pub falloff: f32,
    pub light_angle: f32,
    pub offset_dist: f32,
    pub shadow_color: [f32; 4],
}

impl<Message> shader::Program<Message> for DropShadowProgram {
    type State = ();
    type Primitive = DropShadowPrimitive;

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
        DropShadowPrimitive {
            uniforms: DropShadowUniformsRaw {
                outer_r: self.outer_r.max(0.001),
                intensity: self.intensity.clamp(0.0, 1.0),
                spread: self.spread.max(0.001),
                falloff: self.falloff.clamp(0.0, 1.0),
                light_angle: self.light_angle,
                offset_dist: self.offset_dist.max(0.0),
                _pad0: [0.0; 2],
                shadow_color: self.shadow_color,
            },
        }
    }
}

#[derive(Debug)]
pub struct DropShadowPrimitive {
    uniforms: DropShadowUniformsRaw,
}

impl Primitive for DropShadowPrimitive {
    type Pipeline = DropShadowPipeline;

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

const SHADER_SOURCE: &str = include_str!("drop_shadow.wgsl");

pub struct DropShadowPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for DropShadowPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("drop_shadow.uniforms"),
            size: std::mem::size_of::<DropShadowUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("drop_shadow.bgl"),
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
            label: Some("drop_shadow.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("drop_shadow.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("drop_shadow.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("drop_shadow.pipeline"),
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
