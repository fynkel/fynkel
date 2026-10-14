//! The wgpu renderer. It draws a [`RenderScene`] into a texture view.
//!
//! See `docs/stage-1.md` §3.6. Nothing here knows about the world,
//! platforms, Flutter or Dart.

mod assets;
mod frame_ring;
mod gpu;
mod renderer;
mod scene;
mod shadows;
mod texture;

pub use assets::{Assets, Kit, SharedAssets, lock as lock_assets};
pub use fynkel_mesh::{MeshData, Vertex};
pub use glam;
pub use gpu::{Gpu, GpuError, instance_flags};
pub use renderer::{FrameStats, Renderer};
pub use scene::{
    CameraView, ColorSpace, Draw, Light, LightKind, Line, MaterialTextures, MeshKey, PbrMaterial, Quality, RenderScene,
    ShadowFilter, ShadowLevel, TextureKey,
};
pub use wgpu;
