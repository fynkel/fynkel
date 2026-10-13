//! What renderers share. A [`Kit`] is one GPU device's: its shaders,
//! pipelines and sampler, made once however many views draw on it.
//! [`Assets`] are one world's meshes and textures on that device, uploaded
//! once however many views show the world.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use glam::Vec3;

use crate::gpu::Gpu;
use crate::renderer::Pipelines;
use crate::scene::{ColorSpace, MeshKey, TextureKey};
use crate::texture::{TextureKit, Textures};
use fynkel_mesh::MeshData;

/// One GPU device and what every renderer on it shares: the frame layout,
/// the texture kit, and the pipelines for each target format, made the
/// first time a renderer asks for that format.
pub struct Kit {
    pub(crate) gpu: Gpu,
    pub(crate) frame_layout: wgpu::BindGroupLayout,
    pub(crate) textures: TextureKit,
    pipelines: Mutex<HashMap<wgpu::TextureFormat, Arc<Pipelines>>>,
}

impl std::fmt::Debug for Kit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kit").finish_non_exhaustive()
    }
}

impl Kit {
    /// The kit for `gpu`'s device.
    pub fn new(gpu: &Gpu) -> Arc<Self> {
        let frame_layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(crate::renderer::FRAME_UNIFORMS_SIZE),
                },
                count: None,
            }],
        });
        Arc::new(Self { gpu: gpu.clone(), frame_layout, textures: TextureKit::new(gpu), pipelines: Mutex::default() })
    }

    /// The device this kit is for.
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// The pipelines that draw into `format`, made on first use.
    pub(crate) fn pipelines(&self, format: wgpu::TextureFormat) -> Arc<Pipelines> {
        let mut made = self.pipelines.lock().unwrap_or_else(PoisonError::into_inner);
        made.entry(format).or_insert_with(|| Arc::new(Pipelines::new(self, format))).clone()
    }
}

pub(crate) struct GpuMesh {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) index_count: u32,
    /// A sphere around every vertex, in the mesh's own space: its centre
    /// and radius, for culling.
    pub(crate) centre: Vec3,
    pub(crate) radius: f32,
}

impl GpuMesh {
    fn bytes(&self) -> u64 {
        self.vertices.size() + self.indices.size()
    }
}

/// One world's meshes and textures on a [`Kit`]'s device. Its views'
/// renderers share it ([`SharedAssets`]), so each is uploaded once.
pub struct Assets {
    pub(crate) kit: Arc<Kit>,
    pub(crate) meshes: HashMap<MeshKey, GpuMesh>,
    pub(crate) textures: Textures,
}

impl std::fmt::Debug for Assets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assets")
            .field("meshes", &self.meshes.len())
            .field("texture_bytes", &self.textures.bytes())
            .finish_non_exhaustive()
    }
}

/// [`Assets`] shared by the renderers of every view of one world.
pub type SharedAssets = Arc<Mutex<Assets>>;

impl Assets {
    /// No meshes or textures yet, on `kit`'s device.
    pub fn new(kit: &Arc<Kit>) -> Self {
        Self { kit: kit.clone(), meshes: HashMap::new(), textures: Textures::default() }
    }

    /// [`Assets::new`], to share.
    pub fn shared(kit: &Arc<Kit>) -> SharedAssets {
        Arc::new(Mutex::new(Self::new(kit)))
    }

    /// The kit these are on.
    pub fn kit(&self) -> &Arc<Kit> {
        &self.kit
    }

    /// Uploads `data` under `key`, replacing any mesh already there.
    pub fn upload_mesh(&mut self, key: MeshKey, data: &MeshData) {
        let device = &self.kit.gpu.device;
        let vertices =
            filled_buffer(device, "mesh vertices", wgpu::BufferUsages::VERTEX, bytemuck::cast_slice(data.vertices()));
        let indices =
            filled_buffer(device, "mesh indices", wgpu::BufferUsages::INDEX, bytemuck::cast_slice(data.indices()));
        let (min, max) = data.bounds();
        let centre = (min + max) * 0.5;
        let radius = data.vertices().iter().map(|v| Vec3::from(v.position).distance(centre)).fold(0.0, f32::max);
        self.meshes
            .insert(key, GpuMesh { vertices, indices, index_count: data.indices().len() as u32, centre, radius });
    }

    /// Frees the mesh under `key`. Returns whether there was one.
    pub fn remove_mesh(&mut self, key: MeshKey) -> bool {
        self.meshes.remove(&key).is_some()
    }

    /// Uploads an image under `key`, to be read in `space`, replacing any
    /// texture there in that space. RGBA8, rows from the top. The full mip
    /// chain is drawn on the GPU, averaging in `space`. Draws whose
    /// material names a slot of that space use it from the next frame.
    ///
    /// The image becomes one texture per space it's uploaded in; a draw
    /// whose texture isn't uploaded in its slot's space uses a texture that
    /// changes nothing (white, or a flat normal).
    ///
    /// # Panics
    ///
    /// If `rgba` isn't `width × height × 4` bytes, or a side is 0. A side
    /// over the device's limit (8192 on most) is halved until it fits.
    pub fn upload_texture(&mut self, key: TextureKey, space: ColorSpace, width: u32, height: u32, rgba: &[u8]) {
        let kit = &*self.kit;
        self.textures.upload(&kit.gpu, &kit.textures, key, space, width, height, rgba);
    }

    /// Frees the textures under `key`, in both spaces. Returns whether
    /// there were any.
    pub fn remove_texture(&mut self, key: TextureKey) -> bool {
        self.textures.remove(key)
    }

    /// GPU memory held by textures, in bytes: every mip level, as stored.
    pub fn texture_bytes(&self) -> u64 {
        self.textures.bytes()
    }

    /// GPU memory held by meshes' vertex and index buffers, in bytes.
    pub fn mesh_bytes(&self) -> u64 {
        self.meshes.values().map(GpuMesh::bytes).sum()
    }
}

/// Locks `assets`. A renderer that panicked while holding them left them
/// whole (each change is one map insert or remove), so a poisoned lock is
/// taken anyway.
pub fn lock(assets: &SharedAssets) -> MutexGuard<'_, Assets> {
    assets.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A buffer holding `contents`, written by the CPU where the GPU reads it:
/// no staging copy, so no blit (see `frame_ring`).
fn filled_buffer(device: &wgpu::Device, label: &str, usage: wgpu::BufferUsages, contents: &[u8]) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (contents.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(wgpu::COPY_BUFFER_ALIGNMENT),
        usage: usage | wgpu::BufferUsages::MAP_WRITE,
        mapped_at_creation: true,
    });
    buffer.get_mapped_range_mut(..).expect("mapped at creation").slice(..contents.len()).copy_from_slice(contents);
    buffer.unmap();
    buffer
}
