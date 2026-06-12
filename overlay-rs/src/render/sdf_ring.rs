//! SDF wedge ring spike. Replaces the canvas's tessellated wedge
//! fills with a signed-distance-field shader pass.
//!
//! Pipeline mirrors the dispatch-burst three-piece structure
//! (Program / Primitive / Pipeline). One uniform block carries:
//!
//!   * inner / outer radii (normalised half-extent units, same
//!     space as `aurora.rs` and friends),
//!   * gap angle between adjacent wedges,
//!   * intensity (0 disables the layer entirely from the caller),
//!   * slot count (2..=8, matches `RadialState::active_slot_count`),
//!   * a per-slot colour array (RGBA, alpha doubles as visibility).
//!
//! Hover highlights, icon glows, and the centre puck stay on the
//! iced canvas — see the `feedback_iced_shader_gotchas` memory
//! entry for the hybrid SDF + canvas rationale.

use iced::widget::shader::{self, Primitive};
use iced::{mouse, Event, Rectangle};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SdfRingUniformsRaw {
    inner_r: f32,
    outer_r: f32,
    gap_rad: f32,
    intensity: f32,
    slot_count: u32,
    base_alpha: f32,
    /// Stroke half-width in pixels (along the SDF normal). The
    /// canvas uses 1.0..=1.5 px depending on hover; we expand
    /// 1.2 → 2.0 here so the AA band reads visibly. Stroke
    /// band: `|d| < stroke_half_px * fwidth(d)`.
    stroke_half_px: f32,
    /// Hover wash peak alpha (matches the canvas's 70/255).
    hover_wash_peak: f32,
    /// Distance from menu centre to each slot's icon centre, in
    /// the same normalised half-extent units as `inner_r` /
    /// `outer_r`. Mirrors `Geometry::icon_radius / half_extent`.
    icon_r: f32,
    /// Radius of the icon background disc, normalised. Mirrors
    /// `slices::ICON_BG_RADIUS / half_extent`.
    icon_bg_radius: f32,
    _pad0: [f32; 2],
    /// Per-slot RGBA. Alpha is reserved for future per-slot
    /// visibility tricks but currently unused (canvas paints
    /// every wedge regardless of `visible_if`).
    colors: [[f32; 4]; 8],
    /// Per-slot highlight progress (0..=1) packed into 2 vec4s.
    /// Slot i lives at `highlights_packed[i >> 2][i & 3]`.
    highlights_packed: [[f32; 4]; 2],
    /// Stroke base colour (`surface2`). Hover interpolates from
    /// this toward `accent_color`.
    stroke_color: [f32; 4],
    /// Hover accent (`accent`). Used for stroke interpolation,
    /// hover wash overlay, and the icon glow ring.
    accent_color: [f32; 4],
    /// Icon background base colour (`surface1`). Hover lerps
    /// toward `surface2_color`.
    surface1_color: [f32; 4],
    /// Icon background hover-target colour (`surface2`).
    surface2_color: [f32; 4],
    /// Menu-element transform — see render/animation.rs::MenuXformRaw.
    /// Lets the SDF wedges follow the canvas when the user adds
    /// translate/rotate/flip custom tracks to the menu element.
    menu_xform: super::animation::MenuXformRaw,
}

pub struct SdfRingProgram {
    pub inner_r: f32,
    pub outer_r: f32,
    pub gap_rad: f32,
    pub intensity: f32,
    pub slot_count: u32,
    pub base_alpha: f32,
    pub stroke_half_px: f32,
    pub hover_wash_peak: f32,
    pub icon_r: f32,
    pub icon_bg_radius: f32,
    pub colors: [[f32; 4]; 8],
    /// Per-slot 0..=1 highlight progress (mirrors
    /// `RadialState::highlights[i].current`).
    pub highlights: [f32; 8],
    pub stroke_color: [f32; 4],
    pub accent_color: [f32; 4],
    pub surface1_color: [f32; 4],
    pub surface2_color: [f32; 4],
    /// Menu-element transform — see app.rs for construction
    /// from the menu's `ComposedTransform`.
    pub menu_xform: super::animation::MenuXformRaw,
}

impl<Message> shader::Program<Message> for SdfRingProgram {
    type State = ();
    type Primitive = SdfRingPrimitive;

    fn update(
        &self,
        _state: &mut (),
        _event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        // No internal animation — every uniform we care about
        // (highlights, intensity, menu_alpha-derived radii) is
        // derived from `RadialState` which the 60 Hz Tick
        // subscription advances. `view()` is rebuilt every tick
        // and constructs a fresh Program, so the renderer
        // already re-renders us at frame rate. Returning None
        // here avoids stacking an extra redraw request on top
        // of every iced event during menu open / hover, which
        // is what was nudging the open animation into noticeable
        // lag at SDF intensity = 1.
        None
    }

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Self::Primitive {
        // Pack the 8 per-slot highlight scalars into two
        // vec4-sized arrays. WGSL uniform std140 layout pads
        // `array<f32, N>` elements to 16 bytes each, so a flat
        // f32 array would be 128 bytes; packing into two vec4s
        // keeps it 32 bytes (8 floats × 4 bytes).
        let mut packed = [[0.0_f32; 4]; 2];
        for (i, h) in self.highlights.iter().enumerate() {
            packed[i >> 2][i & 3] = h.clamp(0.0, 1.0);
        }
        SdfRingPrimitive {
            uniforms: SdfRingUniformsRaw {
                inner_r: self.inner_r,
                outer_r: self.outer_r,
                gap_rad: self.gap_rad,
                intensity: self.intensity.clamp(0.0, 1.0),
                slot_count: self.slot_count.clamp(2, 8),
                base_alpha: self.base_alpha.clamp(0.0, 1.0),
                stroke_half_px: self.stroke_half_px.max(0.0),
                hover_wash_peak: self.hover_wash_peak.clamp(0.0, 1.0),
                icon_r: self.icon_r,
                icon_bg_radius: self.icon_bg_radius.max(0.0),
                _pad0: [0.0; 2],
                colors: self.colors,
                highlights_packed: packed,
                stroke_color: self.stroke_color,
                accent_color: self.accent_color,
                surface1_color: self.surface1_color,
                surface2_color: self.surface2_color,
                menu_xform: self.menu_xform,
            },
        }
    }
}

#[derive(Debug)]
pub struct SdfRingPrimitive {
    uniforms: SdfRingUniformsRaw,
}

impl Primitive for SdfRingPrimitive {
    type Pipeline = SdfRingPipeline;

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

const SHADER_SOURCE: &str = include_str!("sdf_ring.wgsl");

pub struct SdfRingPipeline {
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl iced::widget::shader::Pipeline for SdfRingPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sdf_ring.uniforms"),
            size: std::mem::size_of::<SdfRingUniformsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sdf_ring.bgl"),
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
            label: Some("sdf_ring.bg"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sdf_ring.pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sdf_ring.wgsl"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sdf_ring.pipeline"),
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
