//! Page-transition overlay shader. Runs *during* a page-cycle
//! transition and adds a visual effect on top of the canvas's
//! existing two-ring crossfade. Style enum picks between two
//! looks (and is extensible to more):
//!
//!   * **Dissolve** — animated noise-mask flash that peaks
//!     mid-transition. Reads as "pixels scattering" without
//!     needing offscreen render targets to actually swap
//!     textures.
//!   * **Plasma** — sin/cos plasma waves washing across the
//!     menu, phase driven by `progress`. Theatrical / warp feel.
//!
//! Inactive (`progress < 0` or `progress > 1`) → caller should
//! not push the layer at all. The shader defends against bad
//! inputs by clamping but the cheaper guard is at the layer
//! level.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

/// Style discriminator passed to the shader as a u32 uniform —
/// keep in sync with the `select` block at the top of
/// `page_fx.wgsl::fs_main`.
#[derive(Copy, Clone, Debug)]
pub enum PageFxStyle {
    Dissolve,
    Plasma,
}

impl PageFxStyle {
    fn as_u32(self) -> u32 {
        match self {
            PageFxStyle::Dissolve => 0,
            PageFxStyle::Plasma => 1,
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PageFxUniformsRaw {
    /// 0..=1 transition progress.
    progress: f32,
    /// Style selector. Read the matching branch in WGSL.
    style: u32,
    /// User-tunable strength.
    intensity: f32,
    _pad0: f32,
    /// Style-specific knobs packed into one vec4 to keep the
    /// uniform block compact:
    ///   x = dissolve_noise_scale (UV multiplier feeding the
    ///       noise field; higher = finer grain).
    ///   y = dissolve_band_softness (smoothstep upper bound;
    ///       higher = wider, blurrier dissolve front).
    ///   z = plasma_wave_scale (UV multiplier for the wave
    ///       field; higher = more waves per pixel).
    ///   w = plasma_wave_speed (phase-progression multiplier;
    ///       higher = faster boil within the transition).
    /// Keep ordering in sync with `params0` reads in the WGSL.
    params0: [f32; 4],
    /// Two palette colours so the dissolve / plasma aren't
    /// monochromatic.
    color_a: [f32; 4],
    color_b: [f32; 4],
}

pub struct PageFxProgram {
    pub progress: f32,
    pub style: PageFxStyle,
    pub intensity: f32,
    /// Dissolve UV multiplier (higher = finer grain).
    pub dissolve_noise_scale: f32,
    /// Dissolve threshold-band softness.
    pub dissolve_band_softness: f32,
    /// Plasma UV multiplier.
    pub plasma_wave_scale: f32,
    /// Plasma phase-progression multiplier.
    pub plasma_wave_speed: f32,
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
}

impl<Message> shader::Program<Message> for PageFxProgram {
    type State = ();
    type Primitive = PageFxPrimitive;

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
        PageFxPrimitive {
            uniforms: PageFxUniformsRaw {
                progress: self.progress.clamp(0.0, 1.0),
                style: self.style.as_u32(),
                intensity: self.intensity.clamp(0.0, 1.0),
                _pad0: 0.0,
                params0: [
                    self.dissolve_noise_scale.max(0.1),
                    self.dissolve_band_softness.clamp(0.001, 1.0),
                    self.plasma_wave_scale.max(0.1),
                    self.plasma_wave_speed.max(0.0),
                ],
                color_a: self.color_a,
                color_b: self.color_b,
            },
        }
    }
}

#[derive(Debug)]
pub struct PageFxPrimitive {
    uniforms: PageFxUniformsRaw,
}

impl Primitive for PageFxPrimitive {
    type Pipeline = PageFxPipeline;

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

const SHADER_SOURCE: &str = include_str!("page_fx.wgsl");

pub struct PageFxPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for PageFxPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("page_fx.uniforms"),
            size: std::mem::size_of::<PageFxUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("page_fx.bgl"),
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
            label: Some("page_fx.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("page_fx.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("page_fx.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("page_fx.pipeline"),
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
