//! What the renderer draws: a plain value, filled in by whoever owns the
//! world (`fynkel_ffi`'s extract) or by a test.
//!
//! Everything here is linear and already resolved: colours are linear RGB,
//! light colours are multiplied by their intensity, and matrices are final.
//! The renderer converts nothing and looks nothing up except mesh keys.

use glam::{Mat4, Vec3};

/// Names a mesh uploaded with [`crate::Renderer::upload_mesh`]. The
/// caller chooses the values (the world uses its `MeshId`s).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshKey(pub u64);

/// The camera a frame is seen through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    /// World → view (the inverse of the camera's transform).
    pub view: Mat4,
    /// Where the camera is, in world space (for specular highlights).
    pub position: Vec3,
    /// Vertical field of view in radians.
    pub fov_y: f32,
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

/// A metallic-roughness material, resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PbrMaterial {
    /// Linear RGBA.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
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
