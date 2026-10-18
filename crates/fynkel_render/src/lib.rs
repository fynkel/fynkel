//! The wgpu renderer. It draws a [`RenderScene`] into a texture view.
//!
//! See `docs/stage-1.md` §3.6. Nothing here knows about the world,
//! platforms, Flutter or Dart.

mod ao;
mod assets;
mod bloom;
mod clusters;
mod frame_ring;
mod gpu;
mod renderer;
mod scene;
mod shader;
mod shadows;
mod sky;
mod texture;

pub use assets::{Assets, Kit, MadeMesh, MadeTexture, SharedAssets, lock as lock_assets, set_pipeline_cache_dir};
pub use fynkel_mesh::{MeshData, MeshParts, SkinVertex, Vertex};
pub use glam;
pub use gpu::{Gpu, GpuError, instance_flags};
pub use renderer::{FrameStats, Renderer};
pub use scene::{
    AmbientOcclusion, Blend, Bloom, CameraView, ColorSpace, Draw, Fog, Light, LightKind, Line, MaterialTextures,
    MeshKey, Outline, PbrMaterial, Quality, QualityLevel, RenderScene, ShaderKey, ShaderUse, ShadowFilter, ShadowLevel,
    Sky, SkyImage, SkyKind, TextureKey,
};
pub use shader::{
    CustomShader, Lighting, MAX_OWN, MAX_TEXTURES, MAX_VALUES_BYTES, ShaderError, ShaderTexture, ShaderValue, ValueKind,
};
pub use wgpu;
