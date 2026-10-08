//! The wgpu renderer. It draws a [`RenderScene`] into a texture view.
//!
//! See `docs/stage-1.md` §3.6. Nothing here knows about the world,
//! platforms, Flutter or Dart.

mod frame_ring;
mod gpu;
mod renderer;
mod scene;
mod texture;

pub use fynkel_mesh::{MeshData, Vertex};
pub use glam;
pub use gpu::{Gpu, GpuError, instance_flags};
pub use renderer::{FrameStats, Renderer};
pub use scene::{CameraView, ColorSpace, Draw, MaterialTextures, MeshKey, PbrMaterial, RenderScene, Sun, TextureKey};
pub use wgpu;
