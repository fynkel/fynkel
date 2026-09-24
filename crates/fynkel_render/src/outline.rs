//! The selection outline (Unity's Scene view): an orange ring around the
//! selected entities' silhouettes and a blue one around their children,
//! a width in target pixels, drawn over everything — `docs/renderer.md`
//! § The overlay. A host queues it on the [`Overlay`](crate::Overlay)
//! with [`Overlay::outline`](crate::Overlay::outline); `prepare` picks the
//! outlined records out of the extract, and the frame gains two passes
//! after post, before the overlay's lines and triangles:
//!
//! - **The mask**: the outlined instances again, through the mesh vertex
//!   stages, into an `R8Unorm` target at the host target's size — one
//!   sample, no depth test, so what other geometry hides is outlined too.
//!   A selected instance writes 1, a child 0.5, and the target keeps the
//!   larger (`fs_mask`, blended with `Max`).
//! - **The composite**: a fullscreen pass that rings the mask in the
//!   target (`shaders/outline.wgsl`), alpha blended and anti-aliased.
//!
//! Without an outline neither pass exists, and neither does the mask.

use crate::overlay::{OutlineStyle, MAX_OUTLINE_WIDTH};

/// The mask's format: one channel, 0 / 0.5 / 1.
pub(crate) const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// The composite's uniform (`shaders/outline.wgsl`, `Outline`).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct OutlineUniforms {
    selected: [f32; 4],
    children: [f32; 4],
    params: [f32; 4],
}

/// The mask at one size, and the composite's bind group over it.
struct Mask {
    view: wgpu::TextureView,
    size: (u32, u32),
    bind_group: wgpu::BindGroup,
}

/// The composite pipeline, its uniform and the mask. The mask pipelines
/// are the mesh pipelines' siblings (`Pipelines::mask`): they share the
/// mesh layouts and vertex stages.
pub(crate) struct OutlinePass {
    layout: wgpu::BindGroupLayout,
    composite: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    /// Created by the first frame with an outline, re-created when the
    /// target's size changes; kept while outlines come and go.
    mask: Option<Mask>,
    /// This frame draws the outline.
    active: bool,
}

impl OutlinePass {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fynkel outline bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fynkel outline shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/outline.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fynkel outline pl"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        // Straight alpha over the target, as the overlay's primitives.
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
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fynkel outline composite"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
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
        });
        OutlinePass {
            layout,
            composite,
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("fynkel outline uniform"),
                size: std::mem::size_of::<OutlineUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            mask: None,
            active: false,
        }
    }

    /// Whether this frame draws the outline (the mask pass and the
    /// composite).
    pub fn active(&self) -> bool {
        self.active
    }

    /// The mask target, while the frame draws an outline.
    pub fn mask_view(&self) -> Option<&wgpu::TextureView> {
        self.mask.as_ref().filter(|_| self.active).map(|m| &m.view)
    }

    pub fn bytes(&self) -> u64 {
        self.uniform.size()
            + self
                .mask
                .as_ref()
                .map_or(0, |m| u64::from(m.size.0) * u64::from(m.size.1))
    }

    /// Readies the frame's outline — the mask at the target's size, the
    /// style — or, with `style` `None`, leaves the frame without one.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        style: Option<OutlineStyle>,
        target: (u32, u32),
        encode_srgb: bool,
    ) {
        let width = style.map_or(0.0, |s| s.width.clamp(0.0, MAX_OUTLINE_WIDTH));
        // NaN fails the comparison: no outline.
        self.active = width > 0.0 && target.0 > 0 && target.1 > 0;
        let Some(style) = style.filter(|_| self.active) else {
            return;
        };
        if self.mask.as_ref().is_none_or(|m| m.size != target) {
            let view = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("fynkel outline mask"),
                    size: wgpu::Extent3d {
                        width: target.0,
                        height: target.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: MASK_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("fynkel outline bg"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            });
            self.mask = Some(Mask {
                view,
                size: target,
                bind_group,
            });
        }
        let u = OutlineUniforms {
            selected: style.selected,
            children: style.children,
            params: [width, width.ceil() + 1.0, encode_srgb as u32 as f32, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
    }

    /// Rings the mask in `target`, over what it holds.
    pub fn composite(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let Some(mask) = self.mask.as_ref().filter(|_| self.active) else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fynkel outline composite"),
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
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &mask.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uniform_is_laid_out_as_the_shader_reads_it() {
        assert_eq!(std::mem::size_of::<OutlineUniforms>(), 48);
    }
}
