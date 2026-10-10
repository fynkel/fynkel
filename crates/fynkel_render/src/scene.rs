//! What the renderer draws: a plain value, filled in by whoever owns the
//! world (`fynkel_ffi`'s extract) or by a test.
//!
//! Everything here is linear and already resolved: colours are linear RGB,
//! light colours are multiplied by their intensity, and matrices are final.
//! The renderer converts nothing and looks nothing up except mesh and
//! texture keys.

use glam::{Mat4, Vec3};

/// Names a mesh uploaded with [`crate::Renderer::upload_mesh`]. The
/// caller chooses the values (the world uses its `MeshId`s).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshKey(pub u64);

/// Names an image uploaded with [`crate::Renderer::upload_texture`]. The
/// caller chooses the values (the world uses its `TextureId`s).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureKey(pub u64);

/// How a texture's values are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpace {
    /// Colour, as images store it: sampling decodes to linear first.
    Srgb,
    /// Data used as-is: normals, roughness, occlusion.
    Linear,
}

impl ColorSpace {
    pub(crate) fn format(self) -> wgpu::TextureFormat {
        match self {
            ColorSpace::Srgb => wgpu::TextureFormat::Rgba8UnormSrgb,
            ColorSpace::Linear => wgpu::TextureFormat::Rgba8Unorm,
        }
    }
}

/// A material's textures, `None` where it has none. Each slot reads its
/// texture in a fixed colour space ([`MaterialTextures::slots`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaterialTextures {
    /// Multiplies the base colour (and its alpha).
    pub base_color: Option<TextureKey>,
    /// A tangent-space normal map, glTF's convention: +X along +u, +Y up
    /// the image.
    pub normal: Option<TextureKey>,
    /// Roughness in G, metallic in B, multiplying the factors.
    pub metallic_roughness: Option<TextureKey>,
    /// Ambient occlusion in R.
    pub occlusion: Option<TextureKey>,
    /// Multiplies the emissive colour.
    pub emissive: Option<TextureKey>,
}

impl MaterialTextures {
    pub const NONE: Self =
        Self { base_color: None, normal: None, metallic_roughness: None, occlusion: None, emissive: None };

    /// The index of the normal slot in [`MaterialTextures::slots`].
    pub(crate) const NORMAL: usize = 1;

    /// Each slot's texture and how it's read, in binding order.
    pub fn slots(&self) -> [(Option<TextureKey>, ColorSpace); 5] {
        [
            (self.base_color, ColorSpace::Srgb),
            (self.normal, ColorSpace::Linear),
            (self.metallic_roughness, ColorSpace::Linear),
            (self.occlusion, ColorSpace::Linear),
            (self.emissive, ColorSpace::Srgb),
        ]
    }
}

/// The camera a frame is seen through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    /// World → view (the inverse of the camera's transform).
    pub view: Mat4,
    /// Where the camera is, in world space (for specular highlights).
    pub position: Vec3,
    /// Vertical field of view in radians.
    pub fov_y: f32,
    /// Orthographic, with half the view's height in world units; `fov_y`
    /// is then unused.
    pub orthographic: Option<f32>,
    pub near: f32,
    pub far: f32,
    /// Linear RGB. Shown as-is behind everything (not tonemapped), so the
    /// colour picked is the colour seen.
    pub clear_color: [f32; 3],
}

impl CameraView {
    /// A camera at `position` looking at `target`, with +Y up.
    pub fn looking_at(position: Vec3, target: Vec3) -> Self {
        Self {
            view: glam::camera::rh::view::look_at_mat4(position, target, Vec3::Y),
            position,
            fov_y: 60f32.to_radians(),
            orthographic: None,
            near: 0.1,
            far: 1000.0,
            clear_color: [0.0; 3],
        }
    }
}

/// The sun: parallel light from one direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// The direction the light travels (from the sun towards the scene).
    /// Needn't be normalized.
    pub direction: Vec3,
    /// Linear colour × intensity. 1.0 lights a rough surface that faces
    /// the sun to its albedo (stage 1 plan §3.6).
    pub color: Vec3,
}

/// A metallic-roughness material, resolved: glTF's, factors times
/// textures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PbrMaterial {
    /// Linear RGBA.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Linear light the surface gives off, colour × intensity.
    pub emissive: [f32; 3],
    /// Texture coordinates are multiplied by this, per axis: 10 repeats a
    /// texture ten times across a mesh with 0–1 UVs.
    pub uv_scale: [f32; 2],
    /// Then this is added (glTF's texture transform, without rotation).
    pub uv_offset: [f32; 2],
    /// 0 for an opaque surface. Otherwise a cut-out (glTF's MASK): the
    /// surface is there only where its alpha reaches this.
    pub alpha_cutoff: f32,
    /// Draw the back of each triangle too, lit as its front, as for
    /// leaves or a sheet of paper. Otherwise backs aren't drawn.
    pub double_sided: bool,
    pub textures: MaterialTextures,
}

impl PbrMaterial {
    /// White, rough-ish, untextured.
    pub const DEFAULT: Self = Self {
        base_color: [1.0; 4],
        metallic: 0.0,
        roughness: 0.5,
        emissive: [0.0; 3],
        uv_scale: [1.0; 2],
        uv_offset: [0.0; 2],
        alpha_cutoff: 0.0,
        double_sided: false,
        textures: MaterialTextures::NONE,
    };
}

impl Default for PbrMaterial {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One mesh drawn once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub mesh: MeshKey,
    pub model: Mat4,
    pub material: PbrMaterial,
}

/// One frame's worth of things to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderScene {
    /// `None` draws nothing but black.
    pub camera: Option<CameraView>,
    pub sun: Option<Sun>,
    /// Linear radiance arriving equally from every direction. 1.0 alone
    /// shows a rough surface at its albedo.
    pub ambient: Vec3,
    pub draws: Vec<Draw>,
}

impl RenderScene {
    /// Empties the scene but keeps its memory, so refilling it every frame
    /// allocates nothing.
    pub fn clear(&mut self) {
        self.camera = None;
        self.sun = None;
        self.ambient = Vec3::ZERO;
        self.draws.clear();
    }
}
