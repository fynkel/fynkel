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

/// A light, resolved: where it is or which way it shines, and its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// Linear colour × intensity. 1.0 from a sun lights a rough surface
    /// that faces it to its albedo (stage 1 plan §3.6); so does 1.0 from a
    /// point or spot light 1 m away.
    pub color: Vec3,
    pub kind: LightKind,
    /// Whether it casts shadows: a sun's, the first sun that does, and a
    /// spot light's, up to [`ShadowLevel::spot_maps`] a frame. Point lights
    /// cast none yet.
    pub shadows: bool,
}

/// What kind of light a [`Light`] is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LightKind {
    /// Parallel light from one direction, as from the sun. It lights every
    /// object.
    Sun {
        /// The direction the light travels (from the sun towards the
        /// scene). Needn't be normalized.
        direction: Vec3,
    },
    /// A lamp: light from a point in every direction, falling off with the
    /// square of the distance, and fading to nothing at `range`.
    Point { position: Vec3, range: f32 },
    /// A point light that shines only in a cone.
    Spot {
        position: Vec3,
        /// The cone's axis, the way the light shines. Needn't be
        /// normalized.
        direction: Vec3,
        range: f32,
        /// The cone's full angle, in radians, below π.
        angle: f32,
        /// How much of the cone fades, from its edge in: 0 for a hard
        /// edge, 1 to fade all the way to the axis.
        softness: f32,
    },
}

impl Light {
    /// A sun shining along `direction` with `color` (colour × intensity).
    pub fn sun(direction: Vec3, color: Vec3) -> Self {
        Self { color, kind: LightKind::Sun { direction }, shadows: false }
    }

    /// A lamp at `position` reaching `range` metres.
    pub fn point(position: Vec3, range: f32, color: Vec3) -> Self {
        Self { color, kind: LightKind::Point { position, range }, shadows: false }
    }

    /// A spotlight at `position` shining along `direction`, in a cone
    /// `angle` radians across whose outer `softness` fades.
    pub fn spot(position: Vec3, direction: Vec3, range: f32, angle: f32, softness: f32, color: Vec3) -> Self {
        Self { color, kind: LightKind::Spot { position, direction, range, angle, softness }, shadows: false }
    }

    /// The same light, casting shadows or not.
    pub fn with_shadows(self, shadows: bool) -> Self {
        Self { shadows, ..self }
    }

    /// Where a point or spot light is and how far it reaches; `None` for a
    /// sun.
    pub fn reach(&self) -> Option<(Vec3, f32)> {
        match self.kind {
            LightKind::Sun { .. } => None,
            LightKind::Point { position, range } | LightKind::Spot { position, range, .. } => Some((position, range)),
        }
    }
}

/// How much a frame may cost, for how it looks. Each level holds its
/// devices' frame rate: [`Quality::for_adapter`] picks one for a GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Quality {
    Low,
    Medium,
    High,
}

impl Quality {
    /// The level for a GPU: high on Apple's and on desktops', medium on
    /// phones' (Mali, Adreno), low on a software one or one not known.
    pub fn for_adapter(info: &wgpu::AdapterInfo) -> Self {
        Self::for_adapter_on(info, cfg!(target_os = "android"))
    }

    /// [`Quality::for_adapter`], on Android or not. On Android every real
    /// GPU is a phone's, medium whoever makes it: one isn't high because
    /// its maker isn't known.
    pub fn for_adapter_on(info: &wgpu::AdapterInfo, android: bool) -> Self {
        const APPLE: u32 = 0x106B;
        const ARM: u32 = 0x13B5;
        const QUALCOMM: u32 = 0x5143;
        use wgpu::DeviceType::{Cpu, Other, VirtualGpu};
        match (info.device_type, info.vendor) {
            (Cpu, _) => Self::Low,
            (Other | VirtualGpu, _) if android => Self::Low,
            _ if android => Self::Medium,
            (_, ARM | QUALCOMM) => Self::Medium,
            (_, APPLE) => Self::High,
            (_, _) if info.backend == wgpu::Backend::Metal => Self::High,
            (wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu, _) => Self::High,
            _ => Self::Low,
        }
    }

    /// How many point and spot lights may light one object: the ones
    /// brightest at it, which among lights alike are the nearest.
    ///
    /// Lighting a pixel with 8 is too much for a Mali G76 (a Note10+) with
    /// 32 lamps over its whole screen (stage 8 phase 2), so phones' level
    /// takes 4, as low does.
    pub fn lights_per_object(self) -> usize {
        match self {
            Self::Low | Self::Medium => 4,
            Self::High => 8,
        }
    }

    /// How its shadows are drawn.
    pub fn shadows(self) -> ShadowLevel {
        match self {
            Self::Low => ShadowLevel {
                cascades: 1,
                map_size: 1024,
                filter: ShadowFilter::Hard,
                spot_maps: 1,
                spot_map_size: 512,
                distance: 40.0,
            },
            Self::Medium => ShadowLevel {
                cascades: 2,
                map_size: 2048,
                filter: ShadowFilter::Soft,
                spot_maps: 2,
                spot_map_size: 1024,
                distance: 60.0,
            },
            Self::High => ShadowLevel {
                cascades: 3,
                map_size: 2048,
                filter: ShadowFilter::Softer,
                spot_maps: 4,
                spot_map_size: 1024,
                distance: 100.0,
            },
        }
    }

    /// The format of the image the scene is drawn into, with a last pass
    /// after it, which later effects (bloom) need: half floats, or `None`
    /// at low, which draws straight to the screen.
    pub(crate) fn image_format(self) -> Option<wgpu::TextureFormat> {
        (self != Self::Low).then_some(wgpu::TextureFormat::Rgba16Float)
    }
}

/// How a quality level draws shadows ([`Quality::shadows`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowLevel {
    /// How many maps the sun's shadows are split into, nearest the camera
    /// first: each covers further than the one before with as many texels,
    /// so near shadows are sharp and far ones still there. 1 to 3.
    pub cascades: usize,
    /// Each of the sun's maps' side, in texels.
    pub map_size: u32,
    pub filter: ShadowFilter,
    /// The most spot lights casting shadows in a frame: the ones nearest
    /// the camera.
    pub spot_maps: usize,
    /// Each spot light's map's side, in texels.
    pub spot_map_size: u32,
    /// How far from the camera the sun's shadows reach, in metres (or the
    /// camera's far plane, if nearer). They fade out before it.
    pub distance: f32,
}

/// How a shadow's edge is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowFilter {
    /// Sharp: one sample, smoothed across a texel.
    Hard,
    /// Soft: 4 samples, across about 2 texels.
    Soft,
    /// Softer: 9 samples, across about 3 texels.
    Softer,
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
    /// Whether it casts shadows.
    pub cast_shadows: bool,
    /// Whether shadows fall on it.
    pub receive_shadows: bool,
}

impl Draw {
    /// `mesh` placed by `model`, casting and receiving shadows.
    pub fn new(mesh: MeshKey, model: Mat4, material: PbrMaterial) -> Self {
        Self { mesh, model, material, cast_shadows: true, receive_shadows: true }
    }
}

/// A debug line, drawn on top of the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub a: Vec3,
    pub b: Vec3,
    /// Linear RGBA; alpha blends it over the scene.
    pub color: [f32; 4],
}

/// One frame's worth of things to draw.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderScene {
    /// `None` draws nothing but black.
    pub camera: Option<CameraView>,
    /// Every light. Suns light every object, up to
    /// [`Renderer::MAX_SUNS`](crate::Renderer::MAX_SUNS) of them in order.
    /// Point and spot lights light the objects they reach: up to
    /// [`Renderer::MAX_LIGHTS`](crate::Renderer::MAX_LIGHTS) a frame, the
    /// ones nearest the camera, and each part of the view the ones
    /// brightest at it, up to [`Quality::lights_per_cell`].
    pub lights: Vec<Light>,
    /// Linear radiance arriving equally from every direction. 1.0 alone
    /// shows a rough surface at its albedo.
    pub ambient: Vec3,
    pub draws: Vec<Draw>,
    /// Drawn after the draws, over them, one pixel wide.
    pub lines: Vec<Line>,
    /// The level to draw at; `None` for the device's own
    /// ([`Kit::quality`](crate::Kit::quality)).
    pub quality: Option<Quality>,
}

impl RenderScene {
    /// The first sun in [`RenderScene::lights`]: its direction and colour.
    pub fn sun(&self) -> Option<(Vec3, Vec3)> {
        self.lights.iter().find_map(|l| match l.kind {
            LightKind::Sun { direction } => Some((direction, l.color)),
            _ => None,
        })
    }

    /// Empties the scene but keeps its memory, so refilling it every frame
    /// allocates nothing.
    pub fn clear(&mut self) {
        self.camera = None;
        self.lights.clear();
        self.ambient = Vec3::ZERO;
        self.draws.clear();
        self.lines.clear();
        self.quality = None;
    }
}
