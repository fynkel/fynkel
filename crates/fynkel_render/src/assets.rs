//! What renderers share. A [`Kit`] is one GPU device's: its shaders,
//! pipelines and sampler, made once however many views draw on it.
//! [`Assets`] are one world's meshes and textures on that device, uploaded
//! once however many views show the world.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use glam::Vec3;

use crate::ao::AoKit;
use crate::bloom::BloomKit;
use crate::gpu::Gpu;
use crate::renderer::Pipelines;
use crate::scene::{ColorSpace, MeshKey, QualityLevel, ShaderKey, TextureKey};
use crate::shader::CustomShader;
use crate::shadows::ShadowKit;
use crate::sky::SkyKit;
use crate::texture::{GpuTexture, TextureKit, Textures};
use fynkel_mesh::MeshData;

/// One GPU device and what every renderer on it shares: the frame layout,
/// the texture kit, and the pipelines for each target format, made the
/// first time a renderer asks for that format. Also the quality level the
/// device picks.
pub struct Kit {
    pub(crate) gpu: Gpu,
    quality: QualityLevel,
    /// [`crate::gathers_shadows`] for this device, for pipelines made from
    /// now on.
    gather_shadows: AtomicBool,
    pub(crate) frame_layout: wgpu::BindGroupLayout,
    /// The last pass's input: the scene's image.
    pub(crate) image_layout: wgpu::BindGroupLayout,
    /// The multisampled depth one sample of is copied from, for shaders
    /// that read what's behind (behind.wgsl). Drawn with one sample, it's
    /// read through [`Kit::image_layout`].
    pub(crate) msaa_depth_layout: wgpu::BindGroupLayout,
    pub(crate) textures: TextureKit,
    /// Shadow maps' layouts, sampler and pipelines.
    pub(crate) shadows: ShadowKit,
    /// Skies' layouts, samplers and the pipelines that work out their light.
    pub(crate) sky: SkyKit,
    /// Bloom's layout, sampler and pipelines.
    pub(crate) bloom: BloomKit,
    /// Ambient occlusion's.
    pub(crate) ao: AoKit,
    /// Screen effects' layout and sampler.
    pub(crate) effects: crate::effects::EffectKit,
    /// By the format they draw into and their samples.
    pipelines: Mutex<HashMap<(wgpu::TextureFormat, u32), Arc<Pipelines>>>,
    pub(crate) maker: PipelineMaker,
}

/// Where compiled pipelines are kept between runs, when the app has said
/// ([`set_pipeline_cache_dir`]).
static PIPELINE_CACHE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Keeps the GPU's compiled pipelines in `dir`, so the next run's first
/// frame doesn't compile them again. Only Vulkan (Android) has such a
/// cache; Apple's driver keeps its own. Takes effect for devices made
/// after it. package:fynkel_flutter's Android plugin passes the app's
/// cache directory.
pub fn set_pipeline_cache_dir(dir: impl Into<PathBuf>) {
    *PIPELINE_CACHE_DIR.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir.into());
}

/// Makes a device's render pipelines, and counts them.
#[derive(Clone, Debug)]
pub(crate) struct PipelineMaker {
    device: wgpu::Device,
    made: Arc<AtomicU32>,
    disk: Option<DiskCache>,
}

/// A pipeline cache, and the file it's kept in.
#[derive(Clone, Debug)]
struct DiskCache {
    cache: wgpu::PipelineCache,
    file: PathBuf,
    /// Writes it a moment after the last new pipeline, not after each.
    saver: Debounced,
}

impl DiskCache {
    /// The cache for `gpu`'s device, from its file if one was kept, if the
    /// device has caches and the app gave a directory.
    #[allow(unsafe_code)] // wgpu can't prove the data is its own.
    fn open(gpu: &Gpu) -> Option<Self> {
        if !gpu.device.features().contains(wgpu::Features::PIPELINE_CACHE) {
            return None;
        }
        let dir = PIPELINE_CACHE_DIR.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
        // Named for the GPU and its driver's device: another's is no use.
        let file = dir.join(wgpu::util::pipeline_cache_key(&gpu.adapter.get_info())?);
        let data = std::fs::read(&file).ok();
        // SAFETY: the data is what `get_data` gave for a device with this
        // key, or a damaged copy of it, which wgpu checks (its header and
        // a hash) and, with `fallback`, ignores.
        let cache = unsafe {
            gpu.device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("fynkel"),
                data: data.as_deref(),
                fallback: true,
            })
        };
        Some(Self { cache, file, saver: Debounced::new(SAVE_AFTER) })
    }

    /// Writes the cache to its file soon, off this thread: a new pipeline
    /// was just made. Whole or not at all, through a file beside it.
    fn save(&self) {
        let (cache, file) = (self.cache.clone(), self.file.clone());
        self.saver.ask(move || {
            let Some(data) = cache.get_data() else { return };
            let temporary = file.with_extension("partial");
            // Nothing to report: without the file, the next run compiles.
            if std::fs::write(&temporary, data).is_ok() {
                let _ = std::fs::rename(&temporary, &file);
            }
        });
    }
}

/// How long the pipeline cache waits after a new pipeline before it's
/// written: a first run makes dozens in its first seconds, and writing the
/// whole cache after each was a hitch on Android (stage 14's RN3).
const SAVE_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// Runs the last job it was asked for once no more have come for a while,
/// on a thread of its own, and at most once each while: the latest job is
/// always run, after it was asked for.
#[derive(Clone, Debug)]
struct Debounced {
    wait: std::time::Duration,
    state: Arc<Mutex<Waiting>>,
}

#[derive(Default)]
struct Waiting {
    job: Option<Box<dyn FnOnce() + Send>>,
    /// A thread waits to run it.
    running: bool,
}

impl std::fmt::Debug for Waiting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Waiting").field("job", &self.job.is_some()).field("running", &self.running).finish()
    }
}

impl Debounced {
    fn new(wait: std::time::Duration) -> Self {
        Self { wait, state: Arc::default() }
    }

    /// Runs `job` in a while, instead of any asked for before it that
    /// hasn't run yet.
    fn ask(&self, job: impl FnOnce() + Send + 'static) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.job = Some(Box::new(job));
        if state.running {
            return;
        }
        state.running = true;
        let (wait, shared) = (self.wait, Arc::clone(&self.state));
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(wait);
                let job = {
                    let mut state = shared.lock().unwrap_or_else(PoisonError::into_inner);
                    match state.job.take() {
                        Some(job) => job,
                        None => {
                            state.running = false;
                            return;
                        }
                    }
                };
                job();
            }
        });
    }
}

impl PipelineMaker {
    fn new(gpu: &Gpu) -> Self {
        Self { device: gpu.device.clone(), made: Arc::default(), disk: DiskCache::open(gpu) }
    }

    pub(crate) fn make<'a>(&'a self, mut descriptor: wgpu::RenderPipelineDescriptor<'a>) -> wgpu::RenderPipeline {
        self.made.fetch_add(1, Ordering::Relaxed);
        descriptor.cache = self.disk.as_ref().map(|disk| &disk.cache);
        let pipeline = self.device.create_render_pipeline(&descriptor);
        if let Some(disk) = &self.disk {
            disk.save();
        }
        pipeline
    }

    /// Whether pipelines go through a cache kept between runs.
    pub(crate) fn caches(&self) -> bool {
        self.disk.is_some()
    }
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
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(crate::renderer::FRAME_UNIFORMS_SIZE),
                    },
                    count: None,
                },
                // The view's cells and their lamps (`clusters`).
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(crate::clusters::BYTES),
                    },
                    count: None,
                },
                joints_entry(),
            ],
        });
        let image_layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene image"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        // Read as floats, not as depth: Mali's compiler (the Note10+'s)
        // fails on a pipeline that reads multisampled depth as depth.
        let msaa_depth_layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("multisampled depth"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            }],
        });
        let maker = PipelineMaker::new(gpu);
        let textures = TextureKit::new(gpu, &maker);
        Arc::new(Self {
            shadows: ShadowKit::new(&gpu.device, &textures.layout, &maker),
            sky: SkyKit::new(&gpu.device, &gpu.queue, &maker),
            bloom: BloomKit::new(&gpu.device, &maker),
            ao: AoKit::new(&gpu.device, &gpu.queue, &maker, &frame_layout, crate::renderer::INSTANCE_SIZE),
            effects: crate::effects::EffectKit::new(&gpu.device),
            maker,
            gpu: gpu.clone(),
            quality: QualityLevel::for_adapter(&gpu.adapter.get_info()),
            gather_shadows: AtomicBool::new(crate::scene::gathers_shadows(&gpu.adapter.get_info())),
            frame_layout,
            image_layout,
            msaa_depth_layout,
            textures,
            pipelines: Mutex::default(),
        })
    }

    /// The device this kit is for.
    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// The quality level this device picks ([`QualityLevel::for_adapter`]),
    /// for a scene that doesn't choose its quality.
    pub fn device_quality(&self) -> QualityLevel {
        self.quality
    }

    /// Whether this device's shaders read high's shadow filter with gathers.
    pub(crate) fn gather_shadows(&self) -> bool {
        self.gather_shadows.load(Ordering::Relaxed)
    }

    /// For tests: draw shadows as another GPU would. Before any pipelines
    /// are made.
    #[doc(hidden)]
    pub fn set_gather_shadows(&self, on: bool) {
        self.gather_shadows.store(on, Ordering::Relaxed);
    }

    /// How many render pipelines it has made: each is a shader compiled
    /// for the GPU, the bulk of the time to a first frame.
    pub fn pipelines_made(&self) -> u32 {
        self.maker.made.load(Ordering::Relaxed)
    }

    /// How many pipelines of materials' own shaders are being made on
    /// threads of their own, for views that don't wait for them
    /// ([`crate::Renderer::set_wait_for_shaders`]).
    pub fn shaders_waiting(&self) -> u32 {
        let pipelines = self.pipelines.lock().unwrap_or_else(PoisonError::into_inner);
        pipelines.values().map(|p| p.waiting() as u32).sum()
    }

    /// A mesh on the GPU, for [`Assets::insert_mesh`]: made without any
    /// world's assets held.
    pub fn make_mesh(&self, data: &MeshData) -> MadeMesh {
        let device = &self.gpu.device;
        let dynamic = data.is_dynamic() && data.skin().is_none();
        let buffer = if dynamic { roomy_buffer } else { filled_buffer };
        let made = SETTLED.with_borrow_mut(|scratch| {
            let settled = data.settled_vertices(scratch);
            // Worked out once for a mesh that doesn't change: a hash map of
            // every vertex is too slow for one that changes every frame.
            let outline_normals = (!dynamic).then(|| outline_normals(settled)).flatten();
            (
                buffer(device, "mesh vertices", wgpu::BufferUsages::VERTEX, bytemuck::cast_slice(settled)),
                outline_normals,
            )
        });
        let (vertices, outline_normals) = made;
        let indices = buffer(device, "mesh indices", wgpu::BufferUsages::INDEX, bytemuck::cast_slice(data.indices()));
        let (centre, radius) = sphere(data);
        let skin = data.skin().map(|skin| GpuSkin {
            buffer: filled_buffer(device, "mesh joints", wgpu::BufferUsages::VERTEX, bytemuck::cast_slice(skin)),
            joints: joint_bounds(data.vertices(), skin),
        });
        let outline_normals = outline_normals.map(|normals| {
            filled_buffer(device, "mesh outline normals", wgpu::BufferUsages::VERTEX, bytemuck::cast_slice(&normals))
        });
        MadeMesh(GpuMesh {
            vertices,
            indices,
            index_count: data.indices().len() as u32,
            submeshes: data.submeshes().to_vec(),
            centre,
            radius,
            skin,
            outline_normals,
            dynamic,
        })
    }

    /// A texture on the GPU, for [`Assets::insert_texture`]: its pixels
    /// written and its mips drawn, without any world's assets held. As
    /// [`Assets::upload_texture`] says, but for the key.
    pub fn make_texture(&self, space: ColorSpace, width: u32, height: u32, rgba: &[u8]) -> MadeTexture {
        MadeTexture { space, texture: GpuTexture::new(&self.gpu, &self.textures, space, width, height, rgba) }
    }

    /// A font's atlas of distances, one byte a pixel, row by row from the
    /// top, as [`ParticleMode::Glyphs`](crate::ParticleMode::Glyphs) draws
    /// read it from the base colour slot.
    pub fn make_distances(&self, width: u32, height: u32, pixels: &[u8]) -> MadeTexture {
        MadeTexture { space: ColorSpace::Srgb, texture: GpuTexture::distances(&self.gpu, width, height, pixels) }
    }

    /// GPU memory of the shadow maps its renderers hold.
    pub fn shadow_bytes(&self) -> u64 {
        self.shadows.bytes.load(Ordering::Relaxed)
    }

    /// Whether its pipelines are kept between runs
    /// ([`set_pipeline_cache_dir`]).
    pub fn caches_pipelines(&self) -> bool {
        self.maker.caches()
    }

    /// The pipelines that draw into `format`, made on first use.
    pub(crate) fn pipelines(&self, format: wgpu::TextureFormat, samples: u32) -> Arc<Pipelines> {
        let mut made = self.pipelines.lock().unwrap_or_else(PoisonError::into_inner);
        made.entry((format, samples)).or_insert_with(|| Arc::new(Pipelines::new(self, format, samples))).clone()
    }

    /// The most samples, up to `wanted`, that this device draws every one
    /// of `formats` and the depth with: 1 and 4 everywhere WebGPU runs, 2
    /// where the device has its adapter's own format features.
    pub(crate) fn samples(&self, wanted: u32, formats: &[wgpu::TextureFormat]) -> u32 {
        let adapter = &self.gpu.adapter;
        let own = self.gpu.device.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);
        let drawn = |n: u32| {
            n == 1
                || n == 4
                || own
                    && formats
                        .iter()
                        .chain([&crate::renderer::DEPTH_FORMAT])
                        .all(|&f| adapter.get_texture_format_features(f).flags.sample_count_supported(n))
        };
        [4, 2, 1].into_iter().find(|&n| n <= wanted && drawn(n)).unwrap_or(1)
    }
}

/// Each vertex's normal averaged with those of every other vertex at the
/// same place, unit length; `None` when every vertex at a place has the
/// same normal already, as on a smooth mesh. Where they cancel out, as at a
/// double-walled mesh's corners: away from the mesh's middle for a wall
/// facing out, and none for one facing in.
fn outline_normals(vertices: &[fynkel_mesh::Vertex]) -> Option<Vec<[f32; 3]>> {
    /// Added up, the normals at a place shorter than this cancel out.
    const CANCELLED: f32 = 0.05;
    let mut at = HashMap::<[u32; 3], (Vec3, Vec3)>::with_capacity(vertices.len());
    let mut split = false;
    let (mut min, mut max) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for v in vertices {
        let normal = Vec3::from(v.normal);
        let (first, sum) = at.entry(v.position.map(f32::to_bits)).or_insert((normal, Vec3::ZERO));
        split |= first.dot(normal) < 0.999;
        *sum += normal;
        min = min.min(v.position.into());
        max = max.max(v.position.into());
    }
    let middle = (min + max) * 0.5;
    split.then(|| {
        vertices
            .iter()
            .map(|v| {
                let sum = at[&v.position.map(f32::to_bits)].1;
                let out = Vec3::from(v.position) - middle;
                let way = match sum.length() >= CANCELLED {
                    true => sum,
                    // A wall facing in stays: pushed, it'd come in front of the
                    // outer wall it lies on.
                    false if out.dot(v.normal.into()) < 0.0 => Vec3::ZERO,
                    false => out,
                };
                way.normalize_or_zero().into()
            })
            .collect()
    })
}

/// A sphere around every vertex of `data`: its centre and radius.
fn sphere(data: &MeshData) -> (Vec3, f32) {
    let (min, max) = data.bounds();
    let centre = (min + max) * 0.5;
    (centre, data.vertices().iter().map(|v| Vec3::from(v.position).distance(centre)).fold(0.0, f32::max))
}

pub(crate) struct GpuMesh {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) index_count: u32,
    /// Each submesh's indices.
    pub(crate) submeshes: Vec<std::ops::Range<u32>>,
    /// A sphere around every vertex, in the mesh's own space: its centre
    /// and radius, for culling.
    pub(crate) centre: Vec3,
    pub(crate) radius: f32,
    /// A skinned mesh's joints and weights.
    pub(crate) skin: Option<GpuSkin>,
    /// For a mesh whose corners have a normal for each face that meets
    /// there, a cube's: each vertex's normal averaged with those of the
    /// others where it is, which an outline is pushed out along so it
    /// doesn't split at the corners. `[f32; 3]` each.
    pub(crate) outline_normals: Option<wgpu::Buffer>,
    /// Its buffers have room to grow and are written in place
    /// ([`MeshData::mark_dynamic`]).
    dynamic: bool,
}

thread_local! {
    /// The vertices of a mesh owing its normals, with them worked out
    /// ([`MeshData::settled_vertices`]): kept on the thread, so a mesh
    /// changed each frame allocates nothing (stage 16's phase 4).
    static SETTLED: std::cell::RefCell<Vec<fynkel_mesh::Vertex>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// What a skeleton bends a mesh with: each vertex's joints and weights
/// (`fynkel_mesh::SkinVertex`, at locations 14 and 15), and, for culling,
/// a sphere around the vertices each joint moves, in the mesh's space as
/// it was bound, index for index with the joints it uses.
pub(crate) struct GpuSkin {
    pub(crate) buffer: wgpu::Buffer,
    /// `None` for a joint that moves no vertex.
    pub(crate) joints: Vec<Option<(Vec3, f32)>>,
}

impl GpuMesh {
    /// The indices of submesh `n`, or none for one it hasn't.
    pub(crate) fn submesh(&self, n: u32) -> std::ops::Range<u32> {
        self.submeshes.get(n as usize).cloned().unwrap_or(0..0)
    }

    /// Writes `data` into a dynamic mesh's buffers where they are, if it
    /// fits; returns whether it did.
    fn write(&mut self, queue: &wgpu::Queue, data: &MeshData) -> bool {
        let indices: &[u8] = bytemuck::cast_slice(data.indices());
        let vertex_bytes = std::mem::size_of_val(data.vertices()) as u64;
        let fits = vertex_bytes <= self.vertices.size() && indices.len() as u64 <= self.indices.size();
        if !self.dynamic || !data.is_dynamic() || data.skin().is_some() || !fits {
            return false;
        }
        // Queued: frames already sent draw with what was there.
        if vertex_bytes > 0 {
            SETTLED.with_borrow_mut(|scratch| {
                queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(data.settled_vertices(scratch)));
            });
        }
        if !indices.is_empty() {
            queue.write_buffer(&self.indices, 0, indices);
        }
        self.index_count = data.indices().len() as u32;
        self.submeshes.clear();
        self.submeshes.extend_from_slice(data.submeshes());
        (self.centre, self.radius) = sphere(data);
        true
    }

    fn bytes(&self) -> u64 {
        self.vertices.size()
            + self.indices.size()
            + self.skin.as_ref().map_or(0, |s| s.buffer.size())
            + self.outline_normals.as_ref().map_or(0, wgpu::Buffer::size)
    }
}

/// For each joint up to the last one used, a sphere around the vertices
/// it moves at all: the box around them, and the sphere around that.
fn joint_bounds(vertices: &[fynkel_mesh::Vertex], skin: &[fynkel_mesh::SkinVertex]) -> Vec<Option<(Vec3, f32)>> {
    let mut boxes: Vec<Option<(Vec3, Vec3)>> = Vec::new();
    for (vertex, s) in vertices.iter().zip(skin) {
        let p = Vec3::from(vertex.position);
        for (&joint, &weight) in s.joints.iter().zip(&s.weights) {
            if weight == 0 {
                continue;
            }
            let j = usize::from(joint);
            if boxes.len() <= j {
                boxes.resize(j + 1, None);
            }
            boxes[j] = Some(boxes[j].map_or((p, p), |(lo, hi)| (lo.min(p), hi.max(p))));
        }
    }
    boxes.into_iter().map(|b| b.map(|(lo, hi)| ((lo + hi) * 0.5, (hi - lo).length() * 0.5))).collect()
}

/// One world's meshes and textures on a [`Kit`]'s device. Its views'
/// renderers share it ([`SharedAssets`]), so each is uploaded once.
pub struct Assets {
    pub(crate) kit: Arc<Kit>,
    pub(crate) meshes: HashMap<MeshKey, GpuMesh>,
    pub(crate) textures: Textures,
    /// Materials' own shaders (stage 10).
    pub(crate) shaders: HashMap<ShaderKey, Arc<CustomShader>>,
}

impl std::fmt::Debug for Assets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assets")
            .field("meshes", &self.meshes.len())
            .field("texture_bytes", &self.textures.bytes())
            .finish_non_exhaustive()
    }
}

/// A mesh made on the GPU by [`Kit::make_mesh`], to put in a world's
/// [`Assets`].
pub struct MadeMesh(GpuMesh);

/// A texture made on the GPU by [`Kit::make_texture`], to put in a world's
/// [`Assets`].
pub struct MadeTexture {
    space: ColorSpace,
    texture: GpuTexture,
}

impl std::fmt::Debug for MadeMesh {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MadeMesh").field("bytes", &self.0.bytes()).finish()
    }
}

impl std::fmt::Debug for MadeTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MadeTexture").field("space", &self.space).field("bytes", &self.texture.bytes).finish()
    }
}

/// [`Assets`] shared by the renderers of every view of one world.
pub type SharedAssets = Arc<Mutex<Assets>>;

impl Assets {
    /// No meshes or textures yet, on `kit`'s device.
    pub fn new(kit: &Arc<Kit>) -> Self {
        Self { kit: kit.clone(), meshes: HashMap::new(), textures: Textures::default(), shaders: HashMap::new() }
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
    /// [`Kit::make_mesh`] and [`Assets::insert_mesh`] do it without holding
    /// the assets while the GPU's buffers are made.
    /// A dynamic mesh ([`MeshData::mark_dynamic`]) already under `key` is
    /// written where it is, while it fits.
    pub fn upload_mesh(&mut self, key: MeshKey, data: &MeshData) {
        if let Some(mesh) = self.meshes.get_mut(&key)
            && mesh.write(&self.kit.gpu.queue, data)
        {
            return;
        }
        let made = self.kit.make_mesh(data);
        self.insert_mesh(key, made);
    }

    /// Puts a mesh made by [`Kit::make_mesh`] under `key`, replacing any
    /// there.
    pub fn insert_mesh(&mut self, key: MeshKey, mesh: MadeMesh) {
        self.meshes.insert(key, mesh.0);
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
    ///
    /// [`Kit::make_texture`] and [`Assets::insert_texture`] do it without
    /// holding the assets while the GPU works.
    pub fn upload_texture(&mut self, key: TextureKey, space: ColorSpace, width: u32, height: u32, rgba: &[u8]) {
        let made = self.kit.make_texture(space, width, height, rgba);
        self.insert_texture(key, made);
    }

    /// Puts a texture made by [`Kit::make_texture`] under `key`, replacing
    /// any there in its space. Draws use it from the next frame.
    pub fn insert_texture(&mut self, key: TextureKey, texture: MadeTexture) {
        self.textures.insert(key, texture.space, texture.texture);
    }

    /// Frees the textures under `key`, in both spaces. Returns whether
    /// there were any.
    pub fn remove_texture(&mut self, key: TextureKey) -> bool {
        self.textures.remove(key)
    }

    /// GPU memory held by textures, in bytes: every mip level, as stored.
    /// Puts `shader` under `key`, replacing any there: materials drawn with
    /// `key` draw with it from the next frame.
    pub fn upload_shader(&mut self, key: ShaderKey, shader: Arc<CustomShader>) {
        self.shaders.insert(key, shader);
    }

    /// Frees `key`'s shader. Returns whether there was one. Its pipelines
    /// stay with the device's, for another world with the same code.
    pub fn remove_shader(&mut self, key: ShaderKey) -> bool {
        self.shaders.remove(&key).is_some()
    }

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

/// The skeletons' joints (`skin.wgsl`), binding 2 of the frame's group and
/// of the shadow views': read by the vertex stage of skinned draws.
pub(crate) fn joints_entry() -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 2,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(crate::frame_ring::JOINT_SIZE),
        },
        count: None,
    }
}

/// A buffer holding `contents` with room for half as much again, for a
/// mesh that changes: written in place by the queue, so not mapped.
fn roomy_buffer(device: &wgpu::Device, label: &str, usage: wgpu::BufferUsages, contents: &[u8]) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (contents.len() as u64 * 3 / 2).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(256),
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    buffer.get_mapped_range_mut(..).expect("mapped at creation").slice(..contents.len()).copy_from_slice(contents);
    buffer.unmap();
    buffer
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

#[cfg(test)]
mod tests {
    use super::Debounced;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    /// Forty pipelines made one after another write the cache once or
    /// twice, not forty times (stage 14's RN3), and the last is always
    /// written.
    #[test]
    fn many_saves_close_together_write_once_and_the_last_is_kept() {
        let writes = Arc::new(AtomicU32::new(0));
        let seen = Arc::new(AtomicU32::new(0));
        let asked = Arc::new(AtomicU32::new(0));
        let saver = Debounced::new(Duration::from_millis(100));
        let start = std::time::Instant::now();
        for _ in 0..40 {
            asked.fetch_add(1, Ordering::SeqCst);
            let (writes, seen, asked) = (writes.clone(), seen.clone(), asked.clone());
            saver.ask(move || {
                writes.fetch_add(1, Ordering::SeqCst);
                seen.store(asked.load(Ordering::SeqCst), Ordering::SeqCst);
            });
            std::thread::sleep(Duration::from_millis(1));
        }
        // A slow machine's 1 ms sleeps can take several: one write more for
        // each 100 ms the saves took.
        let most = 2 + (start.elapsed().as_millis() / 100) as u32;
        std::thread::sleep(Duration::from_millis(400));
        let written = writes.load(Ordering::SeqCst);
        assert!((1..=most).contains(&written), "{written} writes for 40 saves");
        assert_eq!(seen.load(Ordering::SeqCst), 40, "the last write came after the last save");
        // Another, later on, is written too.
        asked.fetch_add(1, Ordering::SeqCst);
        let (w, s, a) = (writes.clone(), seen.clone(), asked.clone());
        saver.ask(move || {
            w.fetch_add(1, Ordering::SeqCst);
            s.store(a.load(Ordering::SeqCst), Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(writes.load(Ordering::SeqCst), written + 1);
        assert_eq!(seen.load(Ordering::SeqCst), 41);
    }
}
