//! The renderer's GPU-side records: instances, lights, uniforms, loaded
//! meshes, textures and materials, and the batches a frame draws.

use super::*;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Instance {
    pub(crate) model: [[f32; 4]; 4],
    /// Base colour factor.
    pub(crate) tint: [f32; 4],
    /// Emissive × strength (relative to the exposure's white); w is the
    /// skinned draw's first joint in the palette buffer (M3-19).
    pub(crate) emissive: [f32; 4],
    /// metallic, roughness, normal_scale, occlusion_strength
    pub(crate) params: [f32; 4],
    /// alpha cutoff (or `NO_CUTOFF` / `BLEND`), unlit, the object's light
    /// list in the frame's list buffer (E1 — `z` was the double-sided
    /// flag, which only the CPU ever read: it picks the pipeline), and
    /// whether it receives shadows
    pub(crate) flags: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuLight {
    pub(crate) position: [f32; 4],
    pub(crate) color: [f32; 4],
    pub(crate) direction: [f32; 4],
    pub(crate) params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FrameUniforms {
    pub(crate) view_proj: [[f32; 4]; 4],
    pub(crate) inv_view_proj: [[f32; 4]; 4],
    pub(crate) cascades: [GpuCascade; shadow::CASCADES],
    pub(crate) camera_pos: [f32; 4],
    pub(crate) sun_dir: [f32; 4],
    pub(crate) sun_color: [f32; 4],
    pub(crate) ambient: [f32; 4],
    pub(crate) sky: [f32; 4],
    pub(crate) shadow: [f32; 4],
    pub(crate) ao: [f32; 4],
    pub(crate) contact: [f32; 4],
    pub(crate) bloom: [f32; 4],
    pub(crate) bloom_tint: [f32; 4],
    pub(crate) grade: GpuGrade,
    pub(crate) counts: [u32; 4],
    /// x: the world's time (s); y: seconds since the previous frame's;
    /// zw: the internal target's size in pixels (ADR-0031 `globals`).
    pub(crate) time: [f32; 4],
}

/// The frame's colour grading as the post pass reads it (R3c,
/// `Frame.grade` in `shaders/frame.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuGrade {
    /// x: the contrast exponent; y: 1 when the grade runs; z: the tone
    /// mapper (0 ACES, 1 neutral, 2 none); w unused.
    pub(crate) params: [f32; 4],
    /// Before the contrast curve: post-exposure × white balance, columns.
    pub(crate) pre: [[f32; 4]; 3],
    /// After it: saturation × hue × colour filter, columns.
    pub(crate) post: [[f32; 4]; 3],
    pub(crate) lift: [f32; 4],
    /// 1 / gamma per channel.
    pub(crate) inv_gamma: [f32; 4],
    pub(crate) gain: [f32; 4],
}

impl GpuGrade {
    pub(crate) fn new(g: &volume::Grading) -> GpuGrade {
        let cols = |m: glam::Mat3| {
            [
                m.x_axis.extend(0.0).to_array(),
                m.y_axis.extend(0.0).to_array(),
                m.z_axis.extend(0.0).to_array(),
            ]
        };
        let tonemapper = match g.tonemapper {
            volume::Tonemapper::Aces => 0.0,
            volume::Tonemapper::Neutral => 1.0,
            volume::Tonemapper::None => 2.0,
        };
        GpuGrade {
            params: [
                g.contrast_exponent(),
                g.is_active() as u32 as f32,
                tonemapper,
                0.0,
            ],
            pre: cols(g.pre_contrast()),
            post: cols(g.post_contrast()),
            lift: g
                .lift
                .clamp(Vec3::splat(-1.0), Vec3::ONE)
                .extend(0.0)
                .to_array(),
            inv_gamma: g
                .gamma
                .max(Vec3::splat(0.01))
                .recip()
                .extend(0.0)
                .to_array(),
            gain: g.gain.max(Vec3::ZERO).extend(0.0).to_array(),
        }
    }
}

/// One cascade as the mesh shader reads it (R1): where it looks from,
/// which corner of the atlas it lives in, and what a receiver must be
/// pushed along its normal to stay out of its own shadow.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GpuCascade {
    /// World → this cascade's light clip.
    pub(crate) view_proj: [[f32; 4]; 4],
    /// The cascade's rectangle in atlas uv: origin, then size.
    pub(crate) rect: [f32; 4],
    /// x: normal offset in world units; yzw unused.
    pub(crate) params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ShadowUniforms {
    pub(crate) view_proj: [[f32; 4]; 4],
    /// `FrameUniforms::time`, and xyz the camera's position: a shader
    /// asset's shadow stage fills its `globals` from them (ADR-0031).
    pub(crate) time: [f32; 4],
    pub(crate) camera_pos: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PostUniforms {
    pub(crate) clear_color: [f32; 4],
}

/// A mesh on the GPU, in the cooked vertex layout.
pub(crate) struct GpuMesh {
    pub(crate) vertex_buffer: wgpu::Buffer,
    pub(crate) index_buffer: wgpu::Buffer,
    pub(crate) index_count: u32,
    /// A skinned mesh's joints and weights (`SkinWeight` per vertex),
    /// vertex slot 2 of the skinned pipelines (M3-19).
    pub(crate) skin_buffer: Option<wgpu::Buffer>,
    /// The cooked bounds as a sphere, in mesh space (culling).
    pub(crate) bounds: Sphere,
    pub(crate) skin_bounds: Option<crate::culling::SkinBounds>,
}

/// A texture ready for sampling.
pub(crate) struct GpuTexture {
    pub(crate) view: wgpu::TextureView,
    /// Bytes uploaded (every mip), for the GPU memory estimate.
    pub(crate) bytes: u64,
}

/// A material's group 1 — the built-in's five maps, or a shader asset's
/// block, textures and samplers — with the factors the instance carries.
pub(crate) struct GpuMaterial {
    pub(crate) material: PbrMaterial,
    pub(crate) bind_group: wgpu::BindGroup,
    /// The texture refs the bind group holds views of.
    pub(crate) textures: Vec<AssetRef>,
    /// The shader asset it is drawn with (ADR-0031); `None`: the built-in.
    pub(crate) shader: Option<Uuid>,
    /// A shader asset material's payload and its block's uniform buffer,
    /// rewritten in place when only the values change.
    pub(crate) custom: Option<Arc<CustomMaterialPayload>>,
    pub(crate) block: Option<wgpu::Buffer>,
}

/// Whether `b` differs from `a` in its property values alone: the same
/// shader, textures, samplers, alpha and sides.
pub(crate) fn same_but_values(a: &CustomMaterialPayload, b: &CustomMaterialPayload) -> bool {
    a.shader == b.shader
        && a.alpha_mode == b.alpha_mode
        && a.alpha_cutoff == b.alpha_cutoff
        && a.double_sided == b.double_sided
        && a.slots == b.slots
        && a.block.len() == b.block.len()
}

/// What [`Renderer::material`] found for a draw.
pub(crate) struct LoadedMaterial {
    /// The batch key's material; `None`: the default bind group.
    pub(crate) key: Option<AssetRef>,
    /// The factors for the instance (a shader asset's: white, its alpha
    /// mode, cutoff and sides).
    pub(crate) material: PbrMaterial,
    /// `false` for a shader with `shadows off`.
    pub(crate) casts: bool,
    /// A shader asset's material whose shader is missing or does not
    /// compile: drawn unlit magenta, like Unity's pink.
    pub(crate) broken: bool,
    /// No declared displacement envelope exists for custom vertex hooks.
    pub(crate) deforms: bool,
}

/// One draw: a mesh, a material, a contiguous range of instances.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BatchKey {
    /// `None`: the built-in cube.
    pub(crate) mesh: Option<AssetRef>,
    /// `None`: the default material (every map at its fallback).
    pub(crate) material: Option<AssetRef>,
    /// Drawn through the skinned pipelines: the mesh carries skin weights
    /// and every instance a joint palette (M3-19).
    pub(crate) skinned: bool,
}

/// Where an opaque instance is drawn; the order is the batch layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Visibility {
    /// A caster outside the camera frustum but inside the sun's box.
    ShadowOnly = 0,
    /// A caster in view.
    Both = 1,
    /// In view, not casting.
    SceneOnly = 2,
}

/// An opaque (or mask) batch: one contiguous instance range for the
/// scene draw and one for the shadow draw.
pub(crate) struct Batch {
    pub(crate) key: BatchKey,
    pub(crate) double_sided: bool,
    /// The scene draw: `count` instances from `first` (those inside the
    /// camera frustum).
    pub(crate) first: u32,
    pub(crate) count: u32,
    /// The shadow draw: `shadow_count` instances from `shadow_first`
    /// (casters inside the light's box). The two ranges overlap on the
    /// casters that are in both; see `prepare` for the layout.
    pub(crate) shadow_first: u32,
    pub(crate) shadow_count: u32,
}

/// One outlined instance's draw into the selection outline's mask: its
/// batch's mesh and material, and its slot in the outline's own instance
/// buffer (the scene's batches are left as they are).
pub(crate) struct OutlineDraw {
    pub(crate) key: BatchKey,
    pub(crate) double_sided: bool,
    pub(crate) index: u32,
}

/// One `blend` instance, drawn on its own in back-to-front order.
pub(crate) struct TransparentDraw {
    pub(crate) key: BatchKey,
    pub(crate) double_sided: bool,
    pub(crate) index: u32,
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn frame_and_light_storage_match_the_shader_struct_layouts() {
        let shader =
            wgpu::naga::front::wgsl::parse_str(include_str!("shaders/frame.wgsl")).unwrap();
        for (name, bytes) in [
            ("Frame", std::mem::size_of::<FrameUniforms>()),
            ("Light", std::mem::size_of::<GpuLight>()),
        ] {
            let (_, ty) = shader
                .types
                .iter()
                .find(|(_, ty)| ty.name.as_deref() == Some(name))
                .unwrap();
            let wgpu::naga::TypeInner::Struct { span, .. } = ty.inner else {
                panic!("expected struct")
            };
            assert_eq!(span as usize, bytes, "{name} CPU/GPU layout mismatch");
        }
    }
}
