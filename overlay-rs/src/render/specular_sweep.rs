//! Specular sweep — animated narrow band of light that rotates
//! around the disc rim. Time-driven; works with disc_bevel by
//! adding motion on top of the static rim light.
//!
//! See render/specular_sweep.wgsl for the per-pixel math.

use std::time::Instant;

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SpecularSweepUniformsRaw {
    inner_r: f32,
    outer_r: f32,
    intensity: f32,
    time: f32,
    period_s: f32,
    half_width_rad: f32,
    light_angle: f32,
    _pad0: f32,
    sweep_color: [f32; 4],
}

pub struct SpecularSweepProgram {
    pub start: Instant,
    pub inner_r: f32,
    pub outer_r: f32,
    pub intensity: f32,
    pub period_s: f32,
    pub half_width_rad: f32,
    pub light_angle: f32,
    pub sweep_color: [f32; 4],
}

impl<Message> shader::Program<Message> for SpecularSweepProgram {
    type State = ();
    type Primitive = SpecularSweepPrimitive;

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
        SpecularSweepPrimitive {
            uniforms: SpecularSweepUniformsRaw {
                inner_r: self.inner_r,
                outer_r: self.outer_r,
                intensity: self.intensity.clamp(0.0, 1.0),
                time: self.start.elapsed().as_secs_f32(),
                period_s: self.period_s.max(0.5),
                half_width_rad: self.half_width_rad.max(0.001),
                light_angle: self.light_angle,
                _pad0: 0.0,
                sweep_color: self.sweep_color,
            },
        }
    }
}

#[derive(Debug)]
pub struct SpecularSweepPrimitive {
    uniforms: SpecularSweepUniformsRaw,
}

impl Primitive for SpecularSweepPrimitive {
    type Pipeline = SpecularSweepPipeline;

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

const SHADER_SOURCE: &str = include_str!("specular_sweep.wgsl");

pub struct SpecularSweepPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for SpecularSweepPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("specular_sweep.uniforms"),
            size: std::mem::size_of::<SpecularSweepUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("specular_sweep.bgl"),
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
            label: Some("specular_sweep.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("specular_sweep.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("specular_sweep.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("specular_sweep.pipeline"),
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
