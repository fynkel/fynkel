//! The frame's passes, one file each, in the order [`Renderer::encode`]
//! records them: the shadow maps (`shadows`), the solid scene's depth for
//! ambient occlusion and soft particles (`depth`), the scene and what's
//! behind see-through surfaces (`scene`), and the last pass and the
//! screen effects (`post`). Bloom's and ambient occlusion's own passes are
//! in `bloom` and `ao`.

use super::*;
use crate::assets::GpuMesh;
use crate::frame_ring::Written;
use crate::texture::Textures;

mod depth;
mod post;
mod scene;
mod shadows;

/// What every pass reads while a frame is recorded: the kit, the
/// pipelines in the view's format, the assets, the frame's data and the
/// shadow maps.
pub(in crate::renderer) struct Recording<'a> {
    pub(in crate::renderer) kit: &'a Kit,
    pub(in crate::renderer) pipelines: &'a Arc<Pipelines>,
    pub(in crate::renderer) meshes: &'a HashMap<MeshKey, GpuMesh>,
    pub(in crate::renderer) textures: &'a Textures,
    pub(in crate::renderer) shaders: &'a HashMap<ShaderKey, Arc<CustomShader>>,
    pub(in crate::renderer) frame: &'a Written<'a>,
    pub(in crate::renderer) shadow_maps: &'a ShadowMaps,
    pub(in crate::renderer) timer: &'a GpuTimer,
    /// Whether a pipeline not made yet is waited for, or its draws left
    /// out this frame.
    pub(in crate::renderer) wait: bool,
}

/// What the scene's passes read besides: the pipelines in the scene's
/// format, what's behind's and soft particles' bind groups, the sky's
/// light, and what every draw's pipeline is chosen by.
pub(in crate::renderer) struct Scene<'a> {
    pub(in crate::renderer) rec: &'a Recording<'a>,
    pub(in crate::renderer) pipelines: &'a Arc<Pipelines>,
    pub(in crate::renderer) behind_group: Option<&'a wgpu::BindGroup>,
    pub(in crate::renderer) soft_group: Option<&'a wgpu::BindGroup>,
    pub(in crate::renderer) sky_light: &'a wgpu::BindGroup,
    pub(in crate::renderer) sky_lit: bool,
    pub(in crate::renderer) ao: bool,
    pub(in crate::renderer) shadowed: bool,
    pub(in crate::renderer) line_count: u32,
}

/// A mesh's part drawn: its vertices, indices, and `skin` when bent by a
/// skeleton.
fn draw_mesh(
    pass: &mut wgpu::RenderPass<'_>,
    mesh: &GpuMesh,
    skin: Option<&crate::assets::GpuSkin>,
    part: (u32, u32, u32),
) {
    let (submesh, first, count) = part;
    if let Some(skin) = skin {
        pass.set_vertex_buffer(2, skin.buffer.slice(..));
    }
    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(mesh.submesh(submesh), 0, first..first + count);
}
