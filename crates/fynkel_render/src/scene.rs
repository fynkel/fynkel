//! What the renderer draws: a plain value, filled in by whoever owns the
//! world (`fynkel_ffi`'s extract) or by a test.
//!
//! Everything here is linear and already resolved: colours are linear RGB,
//! light colours are multiplied by their intensity, and matrices are final.
//! The renderer converts nothing and looks nothing up except mesh and
//! texture keys.

use std::sync::Arc;

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
    /// colour picked is the colour seen. Behind a [`RenderScene::sky`],
    /// only when [`CameraView::sky`] is off.
    pub clear_color: [f32; 3],
    /// Whether it shows the scene's sky behind everything, if there is
    /// one: off, its clear colour (a minimap's, say). The sky lights the
    /// scene either way.
    pub sky: bool,
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
            sky: true,
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

    /// Whether point and spot lights give shiny surfaces a highlight, as
    /// suns always do. Without, a lamp lights a surface evenly, in its
    /// colour (metal in its own, dimmer than its highlight would be), which
    /// keeps a low level's cost down where many overlap (a Note10+ with 63
    /// lamps over its screen: 54 fps with, 59 without), much as Unity's low
    /// tiers light extra lights per vertex.
    pub fn lamp_highlights(self) -> bool {
        !matches!(self, Self::Low)
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

/// What is seen behind everything, and lights it: every surface takes its
/// light from the sky around it, a rough one its colour, a shiny one its
/// mirror image.
#[derive(Clone, Debug, PartialEq)]
pub struct Sky {
    pub kind: SkyKind,
    /// Multiplies the light it gives (not what's seen of it): 1 lights a
    /// rough white surface under a plain sky of colour c to c.
    pub lighting: f32,
}

/// What a [`Sky`] is. Two images are the same when they're the same
/// [`Arc`], not when their pixels are.
#[derive(Clone, Debug)]
pub enum SkyKind {
    /// One colour, linear, seen as-is: [`RenderScene::ambient`] of that
    /// colour, behind everything.
    Color(Vec3),
    /// Linear colours seen as-is: straight up the zenith, level the
    /// horizon, straight down the ground, blended between.
    Gradient { zenith: Vec3, horizon: Vec3, ground: Vec3 },
    /// A picture all round, light values (tone mapped as the scene is).
    Image {
        image: Arc<SkyImage>,
        /// Multiplies its values.
        intensity: f32,
        /// Turns it about the vertical, in radians: positive to the right
        /// (clockwise, seen from above).
        rotation: f32,
    },
}

impl PartialEq for SkyKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Color(a), Self::Color(b)) => a == b,
            (
                Self::Gradient { zenith: a, horizon: b, ground: c },
                Self::Gradient { zenith: d, horizon: e, ground: f },
            ) => (a, b, c) == (d, e, f),
            (
                Self::Image { image: a, intensity: b, rotation: c },
                Self::Image { image: d, intensity: e, rotation: f },
            ) => Arc::ptr_eq(a, d) && (b, c) == (e, f),
            _ => false,
        }
    }
}

/// A picture of everything around, equirectangular: x the way round from
/// behind (−Z is the middle, +X three quarters across), y from straight up
/// to straight down. Linear RGB light values, as `.hdr` files hold.
#[derive(Debug)]
pub struct SkyImage {
    width: u32,
    height: u32,
    /// Each pixel packed as `Rgb9e5Ufloat`, rows from the top.
    packed: Vec<u32>,
    horizon: Vec3,
}

impl SkyImage {
    /// The most pixels on a side.
    pub const MAX_SIDE: u32 = 8192;

    /// A sky image `width` × `height` from `rgb`, three linear values a
    /// pixel, rows from the top. Refused when the length doesn't match, a
    /// side is 0 or over [`SkyImage::MAX_SIDE`], or a value is negative or
    /// not finite.
    pub fn from_rgb(width: u32, height: u32, rgb: &[f32]) -> Result<Self, String> {
        if width == 0 || height == 0 || width > Self::MAX_SIDE || height > Self::MAX_SIDE {
            return Err(format!("a sky image is 1 to {} pixels a side, not {width} × {height}", Self::MAX_SIDE));
        }
        let pixels = width as usize * height as usize;
        if rgb.len() != pixels * 3 {
            return Err(format!("{width} × {height} needs {} values, not {}", pixels * 3, rgb.len()));
        }
        if let Some(bad) = rgb.iter().find(|v| !v.is_finite() || **v < 0.0) {
            return Err(format!("a sky image's values are light, 0 or more, not {bad}"));
        }
        let packed = rgb.chunks_exact(3).map(|c| pack_rgb9e5(c[0], c[1], c[2])).collect();
        // The rows within 10° of level, or the two (or one) nearest it.
        let degrees = |row: u32| 90.0 - (row as f32 + 0.5) * 180.0 / height as f32;
        let mut rows: Vec<u32> = (0..height).filter(|&r| degrees(r).abs() <= 10.0).collect();
        if rows.is_empty() {
            rows = if height.is_multiple_of(2) { vec![height / 2 - 1, height / 2] } else { vec![height / 2] };
        }
        let mut sum = Vec3::ZERO;
        for &row in &rows {
            let start = row as usize * width as usize * 3;
            for c in rgb[start..start + width as usize * 3].chunks_exact(3) {
                sum += Vec3::new(c[0], c[1], c[2]);
            }
        }
        let horizon = sum / (rows.len() as f32 * width as f32);
        Ok(Self { width, height, packed, horizon })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The average colour within 10° of level: what fog fades into by
    /// default.
    pub fn horizon(&self) -> Vec3 {
        self.horizon
    }

    pub(crate) fn packed(&self) -> &[u32] {
        &self.packed
    }

    /// Its size in memory, on the GPU as here.
    pub fn bytes(&self) -> u64 {
        self.packed.len() as u64 * 4
    }
}

/// Three non-negative values as one `Rgb9e5Ufloat`: a shared exponent and
/// nine bits each (the GL_EXT_texture_shared_exponent rules).
fn pack_rgb9e5(r: f32, g: f32, b: f32) -> u32 {
    const BIAS: i32 = 15;
    const MANTISSA: i32 = 9;
    const MAX: f32 = 65408.0;
    let (r, g, b) = (r.clamp(0.0, MAX), g.clamp(0.0, MAX), b.clamp(0.0, MAX));
    let largest = r.max(g).max(b);
    if largest == 0.0 {
        return 0;
    }
    let mut exponent = (largest.log2().floor() as i32).max(-BIAS - 1) + 1 + BIAS;
    let mut scale = 2f32.powi(exponent - BIAS - MANTISSA);
    if (largest / scale + 0.5).floor() as i32 == 1 << MANTISSA {
        exponent += 1;
        scale *= 2.0;
    }
    let m = |v: f32| ((v / scale + 0.5).floor() as u32).min(511);
    (exponent as u32) << 27 | m(b) << 18 | m(g) << 9 | m(r)
}

/// Fog: what's far fades into its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    /// Linear, seen as-is; `None` for the sky's horizon (tone mapped for an
    /// image's), or with no sky the camera's clear colour.
    pub color: Option<Vec3>,
    /// From this far from the camera, in metres...
    pub start: f32,
    /// ...to this far, where there's only fog.
    pub end: f32,
    /// Thinner with height: as thick as given at y = 0 and below, half as
    /// thick this many metres above, a quarter twice as high. `None`: the
    /// same at every height.
    pub height: Option<f32>,
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
    /// ones nearest the camera, each lighting everything it reaches.
    pub lights: Vec<Light>,
    /// Linear radiance arriving equally from every direction. 1.0 alone
    /// shows a rough surface at its albedo.
    pub ambient: Vec3,
    pub draws: Vec<Draw>,
    /// Drawn after the draws, over them, one pixel wide.
    pub lines: Vec<Line>,
    /// Behind everything, and lighting it, as well as [`Self::ambient`].
    /// `None`: the camera's clear colour, and the ambient light alone.
    pub sky: Option<Sky>,
    pub fog: Option<Fog>,
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
        self.sky = None;
        self.fog = None;
        self.quality = None;
    }
}
