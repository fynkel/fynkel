//! The wgpu renderer. It draws a [`RenderScene`] into a texture view.
//!
//! See `docs/stage-1.md` §3.6. Nothing here knows about the world,
//! platforms, Flutter or Dart.

mod gpu;
mod mesh;
mod renderer;
mod scene;

pub use glam;
pub use gpu::{Gpu, GpuError, instance_flags};
pub use mesh::{MeshData, Vertex};
pub use renderer::{FrameStats, Renderer};
pub use scene::{CameraView, Draw, MeshKey, PbrMaterial, RenderScene, Sun};
pub use wgpu;
