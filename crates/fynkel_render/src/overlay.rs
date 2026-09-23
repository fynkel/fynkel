//! The overlay (ADR-0019, `docs/renderer.md` § The overlay): gizmos and
//! handles drawn after the post pass, into the display-referred target at
//! its own resolution — never exposed, tone mapped, bloomed or resolved
//! through MSAA coverage. A host queues primitives between frames through
//! [`ExtractRenderer::overlay`](crate::ExtractRenderer::overlay); `prepare`
//! uploads and clears the queue, so each frame draws what was queued since
//! the last one.
//!
//! - **Lines** have a width in target pixels and an anti-aliased edge. The
//!   part behind the scene's nearest surface keeps [`OCCLUDED_ALPHA`] of its
//!   alpha instead of disappearing; [`ON_TOP`] lines ignore the scene.
//! - **Triangles** are flat, unlit, blended and anti-aliased at their edges,
//!   for handle tips, plane squares and discs.
//! - **The grid** is the plane `y = 0`, computed per pixel: minor and major
//!   lines whose cell steps by ten as the view widens, the X and Z axes,
//!   faded with distance and at grazing angles, hidden behind geometry.
//!
//! "Behind" compares distances from the camera with a small tolerance, so
//! a line on a surface, or the grid on a floor at `y = 0`, never z-fights
//! with it. The [`DebugDraw`](crate::DebugDraw) queue is drawn here too, as
//! [`DEBUG_LINE_WIDTH`]-pixel lines.

use glam::{Mat4, Vec3};

use crate::debug::DebugDraw;

/// A primitive with this flag is drawn over everything (handles).
pub const ON_TOP: u32 = 1;

/// The alpha a line or triangle keeps where it is behind the scene.
pub const OCCLUDED_ALPHA: f32 = 0.25;

/// The width, in target pixels, of a [`DebugDraw`] line.
pub const DEBUG_LINE_WIDTH: f32 = 1.5;

/// One segment, as the GPU reads it (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OverlayLine {
    pub a: [f32; 3],
    /// Target pixels.
    pub width: f32,
    pub b: [f32; 3],
    /// [`ON_TOP`] or 0.
    pub flags: u32,
    /// Linear, display-referred RGBA (`[1, 0, 0, 1]` is pure red).
    pub color: [f32; 4],
}

/// One triangle, as the GPU reads it (64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OverlayTriangle {
    pub a: [f32; 3],
    pub flags: u32,
    pub b: [f32; 3],
    _pad0: u32,
    pub c: [f32; 3],
    _pad1: u32,
    pub color: [f32; 4],
}

/// The ground grid's look. Colours are linear, display-referred RGBA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub minor: [f32; 4],
    pub major: [f32; 4],
    /// The X axis (the line `z = 0`).
    pub x_axis: [f32; 4],
    /// The Z axis (the line `x = 0`).
    pub z_axis: [f32; 4],
    /// A fixed cell in metres; 0 lets it follow the view.
    pub cell: f32,
    /// Where the grid has faded out, in metres from the camera (perspective
    /// only); 0 is forty times the camera's height, at least forty metres.
    pub fade: f32,
}

impl Default for Grid {
    fn default() -> Self {
        Grid {
            minor: [1.0, 1.0, 1.0, 0.10],
            major: [1.0, 1.0, 1.0, 0.22],
            x_axis: [0.90, 0.25, 0.25, 0.8],
            z_axis: [0.25, 0.45, 0.95, 0.8],
            cell: 0.0,
            fade: 0.0,
        }
    }
}

/// The queue of overlay primitives for the next frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlay {
    lines: Vec<OverlayLine>,
    triangles: Vec<OverlayTriangle>,
    grid: Option<Grid>,
}

impl Overlay {
    /// A segment `width` target pixels wide, faded where it is behind the
    /// scene.
    pub fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4], width: f32) {
        self.push_line(a, b, color, width, 0);
    }

    /// A segment drawn over everything.
    pub fn line_on_top(&mut self, a: Vec3, b: Vec3, color: [f32; 4], width: f32) {
        self.push_line(a, b, color, width, ON_TOP);
    }

    /// A segment with explicit [`ON_TOP`] flags (the ABI's entry point).
    pub fn push_line(&mut self, a: Vec3, b: Vec3, color: [f32; 4], width: f32, flags: u32) {
        self.lines.push(OverlayLine {
            a: a.to_array(),
            width: width.max(0.0),
            b: b.to_array(),
            flags,
            color,
        });
    }

    /// A filled triangle, faded where it is behind the scene.
    pub fn triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4]) {
        self.push_triangle(a, b, c, color, 0);
    }

    /// A filled triangle drawn over everything.
    pub fn triangle_on_top(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4]) {
        self.push_triangle(a, b, c, color, ON_TOP);
    }

    pub fn push_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, color: [f32; 4], flags: u32) {
        self.triangles.push(OverlayTriangle {
            a: a.to_array(),
            flags,
            b: b.to_array(),
            _pad0: 0,
            c: c.to_array(),
            _pad1: 0,
            color,
        });
    }

    /// Draws the ground grid this frame.
    pub fn grid(&mut self, grid: Grid) {
        self.grid = Some(grid);
    }

    pub fn lines(&self) -> &[OverlayLine] {
        &self.lines
    }

    pub fn triangles(&self) -> &[OverlayTriangle] {
        &self.triangles
    }

    pub fn grid_style(&self) -> Option<Grid> {
        self.grid
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.triangles.is_empty() && self.grid.is_none()
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.triangles.clear();
        self.grid = None;
    }
}

/// The overlay pass's uniform (`shaders/overlay.wgsl`, `Overlay`).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct OverlayUniforms {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    camera: [f32; 4],
    sizes: [f32; 4],
    params: [f32; 4],
    grid_minor: [f32; 4],
    grid_major: [f32; 4],
    axis_x: [f32; 4],
    axis_z: [f32; 4],
}

/// What the frame's overlay reads besides its queue.
pub(crate) struct OverlayFrame<'a> {
    pub view_proj: Mat4,
    pub camera_pos: Vec3,
    pub target: (u32, u32),
    /// The scene depth and its size (the internal size).
    pub depth_view: &'a wgpu::TextureView,
    pub depth: (u32, u32),
    /// The target is a Unorm format: this pass encodes sRGB itself.
    pub encode_srgb: bool,
}

/// What `prepare` counted.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OverlayCounts {
    pub lines: u32,
    pub triangles: u32,
    pub grid: bool,
}

/// The pass: three pipelines (grid, lines, triangles), one bind group
/// layout (the uniform and the scene depth as the sample count stores it),
/// and the frame's buffers.
pub(crate) struct OverlayPass {
    layout: wgpu::BindGroupLayout,
    grid: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    triangles: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    line_buffer: wgpu::Buffer,
    line_capacity: u32,
    triangle_buffer: wgpu::Buffer,
    triangle_capacity: u32,
    /// Rebuilt by every `prepare` that has something to draw: it holds the
    /// scene depth, which changes with the attachments.
    bind_group: Option<wgpu::BindGroup>,
    counts: OverlayCounts,
    /// Reused: the queue's lines plus the debug lines.
    scratch: Vec<OverlayLine>,
}

impl OverlayPass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, sample_count: u32) -> Self {
        let (layout, grid, lines, triangles) = create(device, format, sample_count);
        OverlayPass {
            layout,
            grid,
            lines,
            triangles,
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fynkel overlay uniform"),
                size: std::mem::size_of::<OverlayUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            line_buffer: vertex_buffer(device, "fynkel overlay lines", 64 * 48),
            line_capacity: 64,
            triangle_buffer: vertex_buffer(device, "fynkel overlay triangles", 16 * 64),
            triangle_capacity: 16,
            bind_group: None,
            counts: OverlayCounts::default(),
            scratch: Vec::new(),
        }
    }

    /// A new sample count: the depth binding's type changes with it.
    pub fn rebuild(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) {
        let (layout, grid, lines, triangles) = create(device, format, sample_count);
        self.layout = layout;
        self.grid = grid;
        self.lines = lines;
        self.triangles = triangles;
        self.bind_group = None;
    }

    /// Whether the frame draws an overlay (the scene pass then keeps its
    /// depth for this pass to read).
    pub fn active(&self) -> bool {
        self.bind_group.is_some()
    }

    pub fn counts(&self) -> OverlayCounts {
        self.counts
    }

    pub fn bytes(&self) -> u64 {
        self.uniform.size() + self.line_buffer.size() + self.triangle_buffer.size()
    }

    /// Uploads the queue and the debug lines, then clears both.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        overlay: &mut Overlay,
        debug: &mut DebugDraw,
        frame: OverlayFrame<'_>,
    ) {
        self.scratch.clear();
        self.scratch.extend_from_slice(overlay.lines());
        for pair in debug.vertices().chunks_exact(2) {
            self.scratch.push(OverlayLine {
                a: pair[0].position,
                width: DEBUG_LINE_WIDTH,
                b: pair[1].position,
                flags: 0,
                color: pair[0].color,
            });
        }
        let grid = overlay.grid_style();
        self.counts = OverlayCounts {
            lines: self.scratch.len() as u32,
            triangles: overlay.triangles().len() as u32,
            grid: grid.is_some(),
        };
        let empty = self.scratch.is_empty() && overlay.triangles().is_empty() && grid.is_none();
        if empty || frame.target.0 == 0 || frame.target.1 == 0 {
            self.bind_group = None;
            overlay.clear();
            debug.clear();
            return;
        }

        if self.counts.lines > self.line_capacity {
            self.line_capacity = self.counts.lines.next_power_of_two();
            self.line_buffer = vertex_buffer(
                device,
                "fynkel overlay lines",
                u64::from(self.line_capacity) * 48,
            );
        }
        if self.counts.triangles > self.triangle_capacity {
            self.triangle_capacity = self.counts.triangles.next_power_of_two();
            self.triangle_buffer = vertex_buffer(
                device,
                "fynkel overlay triangles",
                u64::from(self.triangle_capacity) * 64,
            );
        }
        if !self.scratch.is_empty() {
            queue.write_buffer(&self.line_buffer, 0, bytemuck::cast_slice(&self.scratch));
        }
        if !overlay.triangles().is_empty() {
            queue.write_buffer(
                &self.triangle_buffer,
                0,
                bytemuck::cast_slice(overlay.triangles()),
            );
        }
        let g = grid.unwrap_or_default();
        let u = OverlayUniforms {
            view_proj: frame.view_proj.to_cols_array_2d(),
            inv_view_proj: frame.view_proj.inverse().to_cols_array_2d(),
            camera: frame.camera_pos.extend(OCCLUDED_ALPHA).to_array(),
            sizes: [
                frame.target.0 as f32,
                frame.target.1 as f32,
                frame.depth.0.max(1) as f32,
                frame.depth.1.max(1) as f32,
            ],
            params: [
                frame.encode_srgb as u32 as f32,
                g.cell.max(0.0),
                g.fade.max(0.0),
                0.0,
            ],
            grid_minor: g.minor,
            grid_major: g.major,
            axis_x: g.x_axis,
            axis_z: g.z_axis,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fynkel overlay bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(frame.depth_view),
                },
            ],
        }));
        overlay.clear();
        debug.clear();
    }

    /// Grid, then lines, then triangles, over what `target` holds.
    pub fn draw(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(bind_group) = &self.bind_group else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });
        pass.set_bind_group(0, bind_group, &[]);
        if self.counts.grid {
            pass.set_pipeline(&self.grid);
            pass.draw(0..3, 0..1);
        }
        if self.counts.lines > 0 {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, self.line_buffer.slice(..));
            pass.draw(0..6, 0..self.counts.lines);
        }
        if self.counts.triangles > 0 {
            pass.set_pipeline(&self.triangles);
            pass.set_vertex_buffer(0, self.triangle_buffer.slice(..));
            pass.draw(0..3, 0..self.counts.triangles);
        }
    }
}

/// `OverlayTriangle` with its pads skipped: `c` sits at byte 32, the
/// colour at 48.
const TRIANGLE_ATTRIBUTES: [wgpu::VertexAttribute; 5] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32,
        offset: 12,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 16,
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 32,
        shader_location: 3,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 48,
        shader_location: 4,
    },
];

fn vertex_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    sample_count: u32,
) -> (
    wgpu::BindGroupLayout,
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
    wgpu::RenderPipeline,
) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fynkel overlay bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: sample_count > 1,
                },
                count: None,
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fynkel overlay shader"),
        source: wgpu::ShaderSource::Wgsl(
            include_str!("shaders/overlay.wgsl")
                .replace(
                    "DEPTH_TEXTURE",
                    if sample_count > 1 {
                        "texture_depth_multisampled_2d"
                    } else {
                        "texture_depth_2d"
                    },
                )
                .replace("DEPTH_SAMPLES", &sample_count.max(1).to_string())
                .into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fynkel overlay pl"),
        bind_group_layouts: &[Some(&layout)],
        ..Default::default()
    });
    // Straight alpha over the target; the target's own alpha stays opaque.
    let blend = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let targets = [Some(wgpu::ColorTargetState {
        format,
        blend: Some(blend),
        write_mask: wgpu::ColorWrites::ALL,
    })];
    let pipeline = |label, vs, fs, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(vs),
                compilation_options: Default::default(),
                buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(fs),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let grid = pipeline("fynkel overlay grid", "vs_grid", "fs_grid", &[]);
    let lines = pipeline(
        "fynkel overlay lines",
        "vs_line",
        "fs_line",
        &[Some(wgpu::VertexBufferLayout {
            array_stride: 48,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3, 1 => Float32, 2 => Float32x3, 3 => Uint32, 4 => Float32x4
            ],
        })],
    );
    let triangles = pipeline(
        "fynkel overlay triangles",
        "vs_triangle",
        "fs_triangle",
        &[Some(wgpu::VertexBufferLayout {
            array_stride: 64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &TRIANGLE_ATTRIBUTES,
        })],
    );
    (layout, grid, lines, triangles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gpu_records_are_laid_out_as_the_shader_reads_them() {
        assert_eq!(std::mem::size_of::<OverlayLine>(), 48);
        assert_eq!(std::mem::size_of::<OverlayTriangle>(), 64);
        assert_eq!(std::mem::size_of::<OverlayUniforms>(), 240);
        let mut o = Overlay::default();
        o.line(Vec3::ZERO, Vec3::X, [1.0; 4], 2.0);
        o.triangle_on_top(Vec3::ZERO, Vec3::X, Vec3::Y, [1.0; 4]);
        o.grid(Grid::default());
        assert_eq!(o.lines()[0].flags, 0);
        assert_eq!(o.triangles()[0].flags, ON_TOP);
        assert!(!o.is_empty());
        o.clear();
        assert!(o.is_empty());
    }
}
