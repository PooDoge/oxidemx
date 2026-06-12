//! AI-chat status effect shader — host side of `status_fx.wgsl`.
//!
//! One `Program` type drives every effect (glow / starfield /
//! fibers / grid+sun / plasma / rings) via a `mode` uniform, so
//! iced's per-TYPE pipeline storage holds exactly one pipeline and
//! one uniform buffer for the whole feature. Only one status is
//! ever active at a time, so a single buffer can't self-clobber.
//! "aurora" and "none" never reach this shader — the view keeps
//! using the dedicated `ChatAuroraProgram` / no layer for those.
//!
//! Architecture and conventions mirror `render/aurora.rs` — read
//! its module docs (and the numbered lessons) before changing the
//! uniform layout or vertex stage.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};
use std::time::Instant;

/// Matches the `Uniforms` block in `status_fx.wgsl` — keep in sync.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct StatusFxUniformsRaw {
    time: f32,
    intensity: f32,
    speed: f32,
    mode: f32,
    aspect: f32,
    _pad: [f32; 3],
    c0: [f32; 4],
    c1: [f32; 4],
    c2: [f32; 4],
}

pub struct StatusFxProgram {
    start: Instant,
    mode: u32,
    intensity: f32,
    speed: f32,
    c0: [f32; 4],
    c1: [f32; 4],
    c2: [f32; 4],
}

impl StatusFxProgram {
    pub fn new(
        start: Instant,
        mode: u32,
        intensity: f32,
        speed: f32,
        c0: [f32; 4],
        c1: [f32; 4],
        c2: [f32; 4],
    ) -> Self {
        Self {
            start,
            mode,
            intensity: intensity.clamp(0.0, 1.0),
            speed: speed.clamp(0.05, 4.0),
            c0,
            c1,
            c2,
        }
    }
}

impl<Message> shader::Program<Message> for StatusFxProgram {
    type State = ();
    type Primitive = StatusFxPrimitive;

    fn update(
        &self,
        _state: &mut (),
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        Some(shader::Action::request_redraw())
    }

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, bounds: Rectangle) -> Self::Primitive {
        StatusFxPrimitive {
            uniforms: StatusFxUniformsRaw {
                time: self.start.elapsed().as_secs_f32(),
                intensity: self.intensity,
                speed: self.speed,
                mode: self.mode as f32,
                aspect: if bounds.height > 1.0 {
                    bounds.width / bounds.height
                } else {
                    1.0
                },
                _pad: [0.0; 3],
                c0: self.c0,
                c1: self.c1,
                c2: self.c2,
            },
        }
    }
}

#[derive(Debug)]
pub struct StatusFxPrimitive {
    uniforms: StatusFxUniformsRaw,
}

impl Primitive for StatusFxPrimitive {
    type Pipeline = StatusFxPipeline;

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

const SHADER_SOURCE: &str = include_str!("status_fx.wgsl");

pub struct StatusFxPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for StatusFxPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("status_fx.uniforms"),
            size: std::mem::size_of::<StatusFxUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("status_fx.bind_group_layout"),
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
            label: Some("status_fx.bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("status_fx.pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("status_fx.shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("status_fx.pipeline"),
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

#[cfg(test)]
mod tests {
    /// Parse + validate every WGSL shader in the render tree with
    /// naga (the same frontend wgpu runs at pipeline creation), so
    /// a shader syntax/type error fails `cargo test` instead of
    /// panicking in the user's session the first time the layer
    /// appears on screen.
    #[test]
    fn all_wgsl_shaders_validate() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("render dir readable") {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("wgsl") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("shader readable");
            let module = naga::front::wgsl::parse_str(&src)
                .unwrap_or_else(|e| panic!("{}: {}", path.display(), e.emit_to_string(&src)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}: {:?}", path.display(), e));
            checked += 1;
        }
        assert!(
            checked >= 13,
            "expected to validate the full shader set, found {checked}"
        );
    }
}
