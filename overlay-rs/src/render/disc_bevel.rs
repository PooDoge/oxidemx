//! Disc bevel — paints a directional rim light at the outer
//! edge of the wedge ring + a carved inset shadow at the inner
//! edge. Always-on framing effect; reads as a polished, physical
//! disc regardless of hover or page state.
//!
//! See render/disc_bevel.wgsl for the per-pixel math.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct DiscBevelUniformsRaw {
    inner_r: f32,
    outer_r: f32,
    intensity: f32,
    rim_width: f32,
    inset_width: f32,
    light_angle: f32,
    shadow_strength: f32,
    _pad0: f32,
    rim_color: [f32; 4],
    shadow_color: [f32; 4],
}

pub struct DiscBevelProgram {
    pub inner_r: f32,
    pub outer_r: f32,
    pub intensity: f32,
    pub rim_width: f32,
    pub inset_width: f32,
    pub light_angle: f32,
    pub shadow_strength: f32,
    pub rim_color: [f32; 4],
    pub shadow_color: [f32; 4],
}

impl<Message> shader::Program<Message> for DiscBevelProgram {
    type State = ();
    type Primitive = DiscBevelPrimitive;

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
        DiscBevelPrimitive {
            uniforms: DiscBevelUniformsRaw {
                inner_r: self.inner_r,
                outer_r: self.outer_r,
                intensity: self.intensity.clamp(0.0, 1.0),
                rim_width: self.rim_width.max(0.001),
                inset_width: self.inset_width.max(0.001),
                light_angle: self.light_angle,
                shadow_strength: self.shadow_strength.clamp(0.0, 1.0),
                _pad0: 0.0,
                rim_color: self.rim_color,
                shadow_color: self.shadow_color,
            },
        }
    }
}

#[derive(Debug)]
pub struct DiscBevelPrimitive {
    uniforms: DiscBevelUniformsRaw,
}

impl Primitive for DiscBevelPrimitive {
    type Pipeline = DiscBevelPipeline;

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

const SHADER_SOURCE: &str = include_str!("disc_bevel.wgsl");

pub struct DiscBevelPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for DiscBevelPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("disc_bevel.uniforms"),
            size: std::mem::size_of::<DiscBevelUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("disc_bevel.bgl"),
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
            label: Some("disc_bevel.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("disc_bevel.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("disc_bevel.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("disc_bevel.pipeline"),
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
