//! Shadows: where the shadow maps look from, and the maps themselves.
//!
//! The sun's shadows are cascaded: the view in front of the camera, out to
//! the level's shadow distance, is cut into slices along it
//! ([`ShadowLevel::cascades`]), and each slice gets a map of its own, seen
//! along the sun. Each map has as many texels, so the near ones, covering
//! less, are sharper. A slice is covered by the sphere around it, whose
//! size doesn't change as the camera turns, and the sphere's centre moves
//! only in whole texels across the map; so a still shadow keeps its texels
//! as the camera moves, and its edges don't crawl (shimmer).
//!
//! A spot light's shadows are one map, seen from the light along its axis,
//! as wide as its cone. Up to [`ShadowLevel::spot_maps`] spot lights cast
//! them in a frame, the ones nearest the camera.
//!
//! Maps hold plain depth (0 near the light, 1 far), and the forward pass
//! compares a surface's own against them (`forward.wgsl`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use glam::{Mat4, Vec3, Vec4};

use crate::assets::PipelineMaker;
use crate::scene::{CameraView, ShadowLevel};

/// The most cascades a level has.
pub(crate) const MAX_CASCADES: usize = 3;
/// The most spot maps a level has.
pub(crate) const MAX_SPOT_MAPS: usize = 4;
/// Every map a frame can draw: the cascades, then the spot maps.
pub(crate) const MAX_MAPS: usize = MAX_CASCADES + MAX_SPOT_MAPS;

/// How the practical split mixes its two schemes: 1 is all logarithmic
/// (each slice as many times further as the one before), 0 all even.
const SPLIT_LAMBDA: f32 = 0.75;

/// Three unit vectors at right angles, from a sun's `direction`, the way
/// its light travels: right and up across its maps, and forward into them.
/// They depend on the direction alone, so the camera moving doesn't turn
/// the maps.
pub(crate) fn light_basis(direction: Vec3) -> [Vec3; 3] {
    let forward = direction.normalize();
    let hint = if forward.y.abs() < 0.99 { Vec3::Y } else { Vec3::X };
    let right = hint.cross(forward).normalize();
    [right, forward.cross(right), forward]
}

/// Where each cascade ends, as distance along the camera's view from the
/// camera, for `count` cascades from `near` out to `distance`. The last is
/// `distance`.
pub(crate) fn cascade_splits(near: f32, distance: f32, count: usize) -> [f32; MAX_CASCADES] {
    let mut splits = [distance; MAX_CASCADES];
    let near = near.max(1e-3);
    for (i, split) in splits.iter_mut().enumerate().take(count.saturating_sub(1)) {
        let t = (i + 1) as f32 / count as f32;
        let log = near * (distance / near).powf(t);
        let even = near + (distance - near) * t;
        *split = SPLIT_LAMBDA * log + (1.0 - SPLIT_LAMBDA) * even;
    }
    splits
}

/// The sphere around the slice of `camera`'s view from distance `near` to
/// `far` along it: its centre's distance along the view, and its radius.
/// `aspect` is width over height.
pub(crate) fn slice_sphere(camera: &CameraView, aspect: f32, near: f32, far: f32) -> (f32, f32) {
    match camera.orthographic {
        // A box: half its height, its width, and its length.
        Some(half) => (0.5 * (near + far), Vec3::new(half * aspect, half, 0.5 * (far - near)).length()),
        // A frustum slice. Its corners at distance d are d·k from the axis;
        // the smallest sphere through all eight, or, when the far ones are
        // the furthest apart, the far ones' circle (Lengyel).
        None => {
            let tan = (camera.fov_y * 0.5).tan();
            let k2 = (1.0 + aspect * aspect) * tan * tan;
            if k2 >= (far - near) / (far + near) {
                (far, far * k2.sqrt())
            } else {
                let centre = 0.5 * (far + near) * (1.0 + k2);
                let radius = 0.5
                    * ((far - near).powi(2) + 2.0 * (far * far + near * near) * k2 + (far + near).powi(2) * k2 * k2)
                        .sqrt();
                (centre, radius)
            }
        }
    }
}

/// The way `camera` looks, as a unit vector.
pub(crate) fn camera_forward(camera: &CameraView) -> Vec3 {
    camera.view.inverse().transform_vector3(Vec3::NEG_Z).normalize_or(Vec3::NEG_Z)
}

/// One cascade's map, seen along the sun.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cascade {
    /// Across the map: the light-space x and y of its centre, snapped to
    /// whole texels, and its half width.
    pub(crate) centre: [f32; 2],
    pub(crate) radius: f32,
    /// Along the light: where its depth range starts and ends, from the
    /// sphere's own, and towards the sun as far as casters reach.
    pub(crate) near: f32,
    pub(crate) far: f32,
}

impl Cascade {
    /// The cascade around the sphere at `centre` (world space) with
    /// `radius`, in a map of `size` texels a side, seen along `basis`.
    /// `near` starts at the sphere's; casters push it towards the sun
    /// ([`Cascade::reach`]).
    pub(crate) fn around(basis: &[Vec3; 3], centre: Vec3, radius: f32, size: u32) -> Self {
        let texel = 2.0 * radius / size as f32;
        let snap = |v: f32| (v / texel).floor() * texel;
        let depth = basis[2].dot(centre);
        Self {
            centre: [snap(basis[0].dot(centre)), snap(basis[1].dot(centre))],
            radius,
            near: depth - radius,
            far: depth + radius,
        }
    }

    /// Whether a sphere at `at` with radius `r` can cast a shadow into this
    /// cascade: it overlaps the map across, and isn't wholly past its far
    /// end. If so, the depth range is stretched towards the sun to hold it,
    /// up to [`MAX_STRETCH`].
    pub(crate) fn reach(&mut self, basis: &[Vec3; 3], at: Vec3, r: f32) -> bool {
        let across = self.radius + r;
        let hit = (basis[0].dot(at) - self.centre[0]).abs() < across
            && (basis[1].dot(at) - self.centre[1]).abs() < across
            && basis[2].dot(at) - r < self.far;
        if hit {
            let limit = self.far - (2.0 + MAX_STRETCH) * self.radius;
            self.near = self.near.min(basis[2].dot(at) - r).max(limit);
        }
        hit
    }

    /// The world-to-map matrix: x and y from -1 to 1 across, depth from 0
    /// (nearest the sun) to 1.
    pub(crate) fn matrix(&self, basis: &[Vec3; 3]) -> Mat4 {
        let [right, up, forward] = basis;
        let depth = (self.far - self.near).max(1e-4);
        let row = |axis: Vec3, at: f32, scale: f32| (axis / scale).extend(-at / scale);
        Mat4::from_cols(
            row(*right, self.centre[0], self.radius),
            row(*up, self.centre[1], self.radius),
            row(*forward, self.near, depth),
            Vec4::W,
        )
        .transpose()
    }

    /// A texel's width, in metres.
    pub(crate) fn texel(&self, size: u32) -> f32 {
        2.0 * self.radius / size as f32
    }
}

/// The cascades of `camera`'s view for `level`, before casters stretch
/// them: each slice's sphere, and where each ends along the view. `aspect`
/// is width over height.
pub(crate) fn cascades(
    camera: &CameraView,
    aspect: f32,
    level: &ShadowLevel,
    basis: &[Vec3; 3],
) -> ([Cascade; MAX_CASCADES], [f32; MAX_CASCADES]) {
    let count = level.cascades.clamp(1, MAX_CASCADES);
    let distance = shadow_distance(camera, level);
    let splits = cascade_splits(camera.near, distance, count);
    let forward = camera_forward(camera);
    let mut out = [Cascade { centre: [0.0; 2], radius: 1.0, near: 0.0, far: 1.0 }; MAX_CASCADES];
    for (i, cascade) in out.iter_mut().enumerate().take(count) {
        // From where the one before starts blending into it.
        let start = if i == 0 { camera.near } else { blend_start(i - 1, &splits).max(camera.near) };
        let (along, radius) = slice_sphere(camera, aspect, start, splits[i]);
        *cascade = Cascade::around(basis, camera.position + forward * along, radius, level.map_size);
    }
    (out, splits)
}

/// Where cascade `c`'s blend into the next starts, along the view: its
/// last tenth, counted from the cascade before's end (or the camera).
pub(crate) fn blend_start(c: usize, splits: &[f32; MAX_CASCADES]) -> f32 {
    let from = if c == 0 { 0.0 } else { splits[c - 1] };
    splits[c] - 0.1 * (splits[c] - from)
}

/// How far from the camera the sun's shadows reach: the level's distance,
/// or less if the camera sees less far.
pub(crate) fn shadow_distance(camera: &CameraView, level: &ShadowLevel) -> f32 {
    level.distance.min(camera.far).max(camera.near + 1e-3)
}

/// A spot light's map: the world-to-map matrix (x, y from -1 to 1 across,
/// depth 0 to 1, perspective) and how wide a texel is per metre from the
/// light. Its view is a little wider than the cone, so the edge of the
/// light isn't the edge of the map.
pub(crate) fn spot_view(position: Vec3, direction: Vec3, angle: f32, range: f32, size: u32) -> (Mat4, f32) {
    let fov = (angle * 1.05).clamp(0.05, 160f32.to_radians());
    let forward = direction.normalize_or(Vec3::NEG_Y);
    let up = if forward.y.abs() < 0.99 { Vec3::Y } else { Vec3::X };
    let near = (range * 0.005).max(0.05);
    let projection = glam::camera::rh::proj::directx::perspective(fov, 1.0, near, range.max(near * 2.0));
    let view = glam::camera::rh::view::look_to_mat4(position, forward, up);
    (projection * view, 2.0 * (fov * 0.5).tan() / size as f32)
}

/// The format of the sun's maps: their depth is linear (an orthographic
/// view), so 16 bits are plenty, and half the memory and bandwidth.
pub(crate) const CASCADE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;
/// The format of spot maps: perspective, so their depth crowds near the
/// far end, and needs a float.
pub(crate) const SPOT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// One caster in a shadow map, as shadow.wgsl reads it (instance step).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Caster {
    pub(crate) model: [[f32; 4]; 4],
    /// UV scale (xy), then offset (zw).
    pub(crate) uv: [f32; 4],
    /// The base colour's alpha and the alpha cutoff (0 for opaque).
    pub(crate) alpha: [f32; 4],
}

/// How far casters may stretch a cascade's depth range towards the sun,
/// past its sphere, in its radius. Further ones are flattened onto the
/// map's near end (`shadow.wgsl`), so the depth bias, a share of the
/// range, stays about the same size in metres.
pub(crate) const MAX_STRETCH: f32 = 8.0;

pub(crate) const CASTER_SIZE: u64 = std::mem::size_of::<Caster>() as u64;

/// What every renderer on a device shares for shadows: the layouts, the
/// sampler the forward pass compares through, and the pipelines that draw
/// the maps, opaque and cut out, for each map format.
pub(crate) struct ShadowKit {
    /// A map's view matrix, at a dynamic offset into the frame's buffer.
    pub(crate) view_layout: wgpu::BindGroupLayout,
    /// The forward pass's group 2: the cascades, the spot maps, and the
    /// sampler.
    pub(crate) maps_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// How far apart the view matrices are in the frame's buffer: the
    /// device's alignment for dynamic offsets.
    pub(crate) view_stride: u64,
    maker: PipelineMaker,
    /// The casters' shader, and its layouts for opaque and cut-out ones.
    shader: wgpu::ShaderModule,
    opaque_layout: wgpu::PipelineLayout,
    cutout_layout: wgpu::PipelineLayout,
    /// GPU memory of every view's maps.
    pub(crate) bytes: Arc<AtomicU64>,
    /// The map arrays views hold, by kind, size and layers, for another
    /// view that wants the same to share.
    arrays: Mutex<HashMap<(usize, u32, u32), Weak<MapArray>>>,
    /// The 1×1 maps that stand in for none, of each kind.
    stand_ins: [OnceLock<Arc<MapArray>>; 2],
    /// The cascades' opaque and cut-out pipelines, then the spot maps',
    /// each made when first drawn with.
    pipelines: [OnceLock<wgpu::RenderPipeline>; 4],
}

impl ShadowKit {
    /// Maps of kind `k` (0 the cascades, 1 the spot maps) and `shape`: the
    /// ones another view holds, if it has them. 0 layers is a stand-in.
    pub(crate) fn array(&self, device: &wgpu::Device, k: usize, (size, count): (u32, u32)) -> Arc<MapArray> {
        if count == 0 {
            return self.stand_ins[k].get_or_init(|| Arc::new(MapArray::new(device, k, (1, 1), None))).clone();
        }
        let mut arrays = self.arrays.lock().unwrap_or_else(PoisonError::into_inner);
        arrays.retain(|_, held| held.strong_count() > 0);
        if let Some(held) = arrays.get(&(k, size, count)).and_then(Weak::upgrade) {
            return held;
        }
        let made = Arc::new(MapArray::new(device, k, (size, count), Some(&self.bytes)));
        arrays.insert((k, size, count), Arc::downgrade(&made));
        made
    }

    pub(crate) fn new(device: &wgpu::Device, textures_layout: &wgpu::BindGroupLayout, maker: &PipelineMaker) -> Self {
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(64),
                },
                count: None,
            }],
        });
        let map_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        };
        let maps_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow maps"),
            entries: &[
                map_entry(0),
                map_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                // And the ambient occlusion (`ao`), which comes and goes
                // with the view's size, as the maps with its level.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // Linear: each compare is smoothed across the four texels around
        // it, so even one tap gives an edge a texel wide, not a staircase.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow maps"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("shadow.wgsl"));
        let opaque_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[Some(&view_layout)],
            immediate_size: 0,
        });
        let cutout_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow cut-out"),
            bind_group_layouts: &[Some(&view_layout), Some(textures_layout)],
            immediate_size: 0,
        });
        Self {
            view_layout,
            maps_layout,
            sampler,
            view_stride: u64::from(device.limits().min_uniform_buffer_offset_alignment).max(64),
            maker: maker.clone(),
            bytes: Arc::default(),
            arrays: Mutex::default(),
            stand_ins: Default::default(),
            shader,
            opaque_layout,
            cutout_layout,
            pipelines: Default::default(),
        }
    }

    /// The pipeline that draws casters into a spot map or a cascade,
    /// opaque or cut out.
    pub(crate) fn pipeline(&self, spot: bool, cutout: bool) -> &wgpu::RenderPipeline {
        self.pipelines[usize::from(spot) * 2 + usize::from(cutout)]
            .get_or_init(|| self.make(if spot { SPOT_FORMAT } else { CASCADE_FORMAT }, cutout))
    }

    fn make(&self, format: wgpu::TextureFormat, cutout: bool) -> wgpu::RenderPipeline {
        // From the mesh, the position and the UV; from the caster, the rest.
        let vertex_attributes = [
            wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: std::mem::offset_of!(fynkel_mesh::Vertex, uv) as u64,
                shader_location: 3,
            },
        ];
        let caster_attributes = wgpu::vertex_attr_array![
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
        ];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<fynkel_mesh::Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vertex_attributes,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: CASTER_SIZE,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &caster_attributes,
            }),
        ];
        // Both sides of every caster: a thin wall or a leaf shadows from
        // either side. The bias keeps a surface from shadowing itself where
        // it slopes away from the light (acne); the forward pass also moves
        // its lookups out along the normal.
        let bias = match format {
            CASCADE_FORMAT => wgpu::DepthBiasState { constant: 2, slope_scale: 1.5, clamp: 0.0 },
            _ => wgpu::DepthBiasState { constant: 4, slope_scale: 1.5, clamp: 0.0 },
        };
        self.maker.make(wgpu::RenderPipelineDescriptor {
            label: Some(if cutout { "shadow cut-out" } else { "shadow" }),
            layout: Some(if cutout { &self.cutout_layout } else { &self.opaque_layout }),
            vertex: wgpu::VertexState {
                module: &self.shader,
                // The cascades' casters past the depth range are flattened
                // onto its near end ([`MAX_STRETCH`]).
                entry_point: Some(if format == CASCADE_FORMAT { "vs_cascade" } else { "vs_main" }),
                compilation_options: Default::default(),
                buffers: &buffers,
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias,
            }),
            multisample: Default::default(),
            fragment: cutout.then(|| wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_cutout"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            multiview_mask: None,
            cache: None,
        })
    }
}

/// One renderer's shadow maps: the cascades and the spot maps, each an
/// array of layers, and the forward pass's bind group for them. With no
/// shadows, each is a 1×1 map that nothing reads.
/// A view's shadow maps, the cascades' then the spot maps': each an array
/// shared with the device's other views that want the same, and the bind
/// group the forward pass reads them through.
pub(crate) struct ShadowMaps {
    /// Size and layers of the cascades, then of the spot maps.
    pub(crate) shape: [(u32, u32); 2],
    arrays: [Arc<MapArray>; 2],
    pub(crate) bind_group: wgpu::BindGroup,
    /// Which ambient occlusion the bind group reads ([`Ao`]).
    ao: u64,
}

/// The ambient occlusion the forward pass reads, bound with the maps: its
/// images' id (0 for the white stand-in), view and sampler.
pub(crate) type Ao<'a> = (u64, &'a wgpu::TextureView, &'a wgpu::Sampler);

impl ShadowMaps {
    /// Maps of `shape`: the cascades' size and layers, then the spot maps'.
    /// A shape of 0 layers is a 1×1 map that stands in for none.
    pub(crate) fn new(device: &wgpu::Device, kit: &ShadowKit, shape: [(u32, u32); 2], ao: Ao<'_>) -> Self {
        let arrays = [0, 1].map(|k| kit.array(device, k, shape[k]));
        let bind_group = Self::bind(device, kit, &arrays, ao);
        Self { shape, arrays, bind_group, ao: ao.0 }
    }

    /// Reads `ao` from now on, if it doesn't already.
    pub(crate) fn read_ao(&mut self, device: &wgpu::Device, kit: &ShadowKit, ao: Ao<'_>) {
        if self.ao != ao.0 {
            self.bind_group = Self::bind(device, kit, &self.arrays, ao);
            self.ao = ao.0;
        }
    }

    fn bind(
        device: &wgpu::Device,
        kit: &ShadowKit,
        arrays: &[Arc<MapArray>; 2],
        (_, view, sampler): Ao<'_>,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow maps"),
            layout: &kit.maps_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&arrays[0].array) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&arrays[1].array) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&kit.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    /// The view to draw cascade `i` into, or spot map `i`.
    pub(crate) fn layer(&self, spot: bool, i: usize) -> &wgpu::TextureView {
        &self.arrays[usize::from(spot)].layers[i]
    }

    /// The GPU memory of maps of `shape`.
    pub(crate) fn bytes(shape: [(u32, u32); 2]) -> u64 {
        (0..2).map(|k| map_bytes(k, shape[k])).sum()
    }
}

/// One texture array of shadow maps.
pub(crate) struct MapArray {
    /// A view per layer, to draw into.
    layers: Vec<wgpu::TextureView>,
    /// All of them, to read.
    array: wgpu::TextureView,
    /// Its memory, and the kit's count it's in: none for a stand-in.
    counted: Option<(u64, Arc<AtomicU64>)>,
}

impl MapArray {
    /// `count` maps of kind `k` (0 the cascades, 1 the spot maps), `size`
    /// texels square; its memory added to `counter`, if given.
    fn new(device: &wgpu::Device, k: usize, (size, count): (u32, u32), counter: Option<&Arc<AtomicU64>>) -> Self {
        let (format, label) = [(CASCADE_FORMAT, "sun shadow maps"), (SPOT_FORMAT, "spot shadow maps")][k];
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: count },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layers = (0..count)
            .map(|layer| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some(label),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let array = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(label),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let counted = counter.map(|counter| {
            let bytes = map_bytes(k, (size, count));
            counter.fetch_add(bytes, Ordering::Relaxed);
            (bytes, counter.clone())
        });
        Self { layers, array, counted }
    }
}

impl Drop for MapArray {
    fn drop(&mut self) {
        if let Some((bytes, counter)) = &self.counted {
            counter.fetch_sub(*bytes, Ordering::Relaxed);
        }
    }
}

/// The GPU memory of `count` maps of kind `k`, `size` texels square.
fn map_bytes(k: usize, (size, count): (u32, u32)) -> u64 {
    let format = [CASCADE_FORMAT, SPOT_FORMAT][k];
    u64::from(size) * u64::from(size) * u64::from(count) * texel_bytes(format)
}

/// A shadow map texel's bytes.
fn texel_bytes(format: wgpu::TextureFormat) -> u64 {
    u64::from(format.block_copy_size(Some(wgpu::TextureAspect::DepthOnly)).unwrap_or(4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Quality;

    fn camera(position: Vec3, target: Vec3) -> CameraView {
        CameraView { near: 0.1, far: 500.0, ..CameraView::looking_at(position, target) }
    }

    /// Where a world point falls in a cascade's map, in texels from its
    /// left and bottom edges.
    fn texel_at(cascade: &Cascade, basis: &[Vec3; 3], size: u32, p: Vec3) -> [f32; 2] {
        let m = cascade.matrix(basis).project_point3(p);
        [(m.x * 0.5 + 0.5) * size as f32, (m.y * 0.5 + 0.5) * size as f32]
    }

    #[test]
    fn splits_grow_and_end_at_the_distance() {
        for count in 1..=MAX_CASCADES {
            let splits = cascade_splits(0.1, 100.0, count);
            assert_eq!(splits[count - 1], 100.0);
            assert!(splits.windows(2).all(|w| w[0] <= w[1]), "{splits:?}");
            assert!(splits[0] > 0.1);
        }
    }

    #[test]
    fn the_slice_sphere_holds_every_corner() {
        let cam = camera(Vec3::ZERO, Vec3::NEG_Z);
        let aspect = 16.0 / 9.0;
        for (near, far) in [(0.1, 2.0), (2.0, 10.0), (10.0, 100.0), (0.1, 100.0)] {
            let (along, radius) = slice_sphere(&cam, aspect, near, far);
            let centre = Vec3::new(0.0, 0.0, -along);
            let tan = (cam.fov_y * 0.5).tan();
            for d in [near, far] {
                for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let corner = Vec3::new(sx * d * tan * aspect, sy * d * tan, -d);
                    assert!(corner.distance(centre) <= radius * 1.0001, "{near}..{far}: {corner}");
                }
            }
        }
    }

    // Done when (docs/stage-8.md phase 3): shadows that don't shimmer while
    // the camera moves. A still point must keep its place within a texel,
    // in every cascade, however the camera moves and turns, so the texels a
    // shadow edge falls on don't change.
    #[test]
    fn a_still_point_keeps_its_place_in_its_texel_as_the_camera_moves() {
        let level = Quality::High.shadows();
        let basis = light_basis(Vec3::new(-0.4, -1.0, -0.3));
        let point = Vec3::new(1.37, 0.0, -4.21);
        let mut first: Option<Vec<[f32; 2]>> = None;
        let mut radii: Option<Vec<f32>> = None;
        for step in 0..40 {
            let t = step as f32 * 0.137;
            // Walking and turning: the target swings round the point.
            let position = Vec3::new(t * 0.31, 1.7 + (t * 0.5).sin() * 0.2, 6.0 - t * 0.23);
            let target = point + Vec3::new((t * 0.9).sin() * 3.0, 0.0, (t * 0.7).cos() * 2.0);
            let cam = camera(position, target);
            let (cascades, _) = cascades(&cam, 16.0 / 9.0, &level, &basis);
            let fractions: Vec<[f32; 2]> = cascades[..level.cascades]
                .iter()
                .map(|c| texel_at(c, &basis, level.map_size, point).map(|v| v.rem_euclid(1.0)))
                .collect();
            let sizes: Vec<f32> = cascades[..level.cascades].iter().map(|c| c.radius).collect();
            match (&first, &radii) {
                (Some(f), Some(r)) => {
                    for (i, (a, b)) in f.iter().zip(&fractions).enumerate() {
                        for k in 0..2 {
                            let d = (a[k] - b[k]).abs();
                            assert!(d.min(1.0 - d) < 0.01, "cascade {i} step {step}: {a:?} → {b:?}");
                        }
                    }
                    for (a, b) in r.iter().zip(&sizes) {
                        assert!((a - b).abs() < 1e-4 * a, "a cascade's size changed: {a} → {b}");
                    }
                }
                _ => (first, radii) = (Some(fractions), Some(sizes)),
            }
        }
    }

    // Hardening R6: the last tenth of each cascade but the last blends into
    // the next, so the next must hold that band too.
    #[test]
    fn the_next_cascade_holds_the_band_it_blends_over() {
        let basis = light_basis(Vec3::new(-0.4, -1.0, -0.3));
        let aspect = 16.0 / 9.0;
        let perspective = camera(Vec3::new(0.0, 2.0, 5.0), Vec3::new(0.0, 0.0, -10.0));
        let ortho = CameraView { orthographic: Some(6.0), ..perspective };
        for cam in [perspective, ortho] {
            for quality in [Quality::Medium, Quality::High] {
                let level = quality.shadows();
                let (cascades, splits) = cascades(&cam, aspect, &level, &basis);
                let forward = camera_forward(&cam);
                let right = forward.cross(Vec3::Y).normalize();
                let up = right.cross(forward);
                for c in 0..level.cascades - 1 {
                    let m = cascades[c + 1].matrix(&basis);
                    for d in [blend_start(c, &splits), splits[c]] {
                        let (w, h) = match cam.orthographic {
                            Some(half) => (half * aspect, half),
                            None => {
                                let tan = (cam.fov_y * 0.5).tan();
                                (d * tan * aspect, d * tan)
                            }
                        };
                        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                            let p = cam.position + forward * d + right * (sx * w) + up * (sy * h);
                            let q = m.project_point3(p);
                            assert!(
                                q.x.abs() <= 1.0 && q.y.abs() <= 1.0 && (0.0..=1.0).contains(&q.z),
                                "{quality:?} {:?}: cascade {} misses {p} at {d} → {q}",
                                cam.orthographic,
                                c + 1
                            );
                        }
                    }
                }
            }
        }
    }

    // Hardening R10: the depth bias is a fixed share of the depth range, so
    // a caster far towards the sun mustn't stretch it without end. Past the
    // limit, casters are flattened onto the map's near end (shadow.wgsl).
    #[test]
    fn a_far_caster_stretches_the_depth_range_only_so_far() {
        let basis = light_basis(Vec3::new(0.3, -1.0, 0.2));
        let centre = Vec3::new(3.0, 1.0, -2.0);
        let mut cascade = Cascade::around(&basis, centre, 5.0, 1024);
        assert!(cascade.reach(&basis, centre - basis[2] * 1000.0, 1.0), "it still casts");
        let range = cascade.far - cascade.near;
        assert!(range <= (2.0 + 8.0) * 5.0 + 1e-3, "{range} m deep: the sphere and 8 radii more");
    }

    #[test]
    fn a_cascade_maps_its_sphere_into_the_map() {
        let basis = light_basis(Vec3::new(0.3, -1.0, 0.2));
        let centre = Vec3::new(3.0, 1.0, -2.0);
        let mut cascade = Cascade::around(&basis, centre, 5.0, 1024);
        // A caster above, towards the sun, stretches the depth range to it.
        let caster = centre - basis[2] * 20.0;
        assert!(cascade.reach(&basis, caster, 1.0));
        let m = cascade.matrix(&basis);
        for p in [centre, centre + basis[0] * 4.9, centre - basis[1] * 4.9, centre + basis[2] * 4.9, caster] {
            let q = m.project_point3(p);
            assert!(q.x.abs() <= 1.0 && q.y.abs() <= 1.0 && (0.0..=1.0).contains(&q.z), "{p} → {q}");
        }
        // Nearer the sun is nearer 0.
        assert!(m.project_point3(caster).z < m.project_point3(centre).z);
        // Off to the side, it can't shadow this map.
        assert!(!cascade.reach(&basis, centre + basis[0] * 20.0, 1.0));
    }

    #[test]
    fn a_spot_view_sees_along_its_axis() {
        let (m, texel) = spot_view(Vec3::new(0.0, 4.0, 0.0), Vec3::NEG_Y, 1.0, 10.0, 1024);
        let below = m.project_point3(Vec3::new(0.0, 0.0, 0.0));
        assert!(below.x.abs() < 1e-5 && below.y.abs() < 1e-5 && (0.0..1.0).contains(&below.z), "{below}");
        let further = m.project_point3(Vec3::new(0.0, -3.0, 0.0));
        assert!(further.z > below.z);
        assert!(texel > 0.0 && texel < 0.01);
    }
}
