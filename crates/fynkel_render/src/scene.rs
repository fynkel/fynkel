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

/// A material's own shader (stage 10), as [`Assets::upload_shader`](crate::Assets::upload_shader)
/// names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShaderKey(pub u64);

/// A material drawn with its own shader: which, where its values are in
/// [`RenderScene::shader_values`], and its own textures, in the shader's
/// order (`None` reads white).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaderUse {
    pub shader: ShaderKey,
    /// The first of its values' 48-byte blocks.
    pub values: u32,
    pub textures: [Option<TextureKey>; crate::MAX_TEXTURES],
    /// How far, in metres, its `vertex` can move a vertex from where the
    /// mesh has it: the draw's bounds grow by this much for culling.
    pub reach: f32,
}

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
    /// Whether light brighter than white glows into what's around it, and
    /// how: `None`, it doesn't (a minimap's, say). Only where the quality
    /// draws it ([`Quality::bloom`]).
    pub bloom: Option<Bloom>,
    /// Whether surfaces darken where they meet, and how: `None`, they
    /// don't. Only where the quality level draws it
    /// ([`Quality::ambient_occlusion`]).
    pub ambient_occlusion: Option<AmbientOcclusion>,
    /// The pixel look: the scene drawn this many pixels high (and as wide
    /// as the view's shape makes it), then enlarged to the view with hard
    /// edges. The camera is moved to the nearest whole pixel of that
    /// picture, so what's still doesn't shimmer as it moves: exactly for
    /// an orthographic camera; for a perspective one, for what's where it
    /// looks at the ground (y = 0), or where it looks 10 m away when it
    /// doesn't. `None`, at the view's own size. At least 1, and no more
    /// than the view's height.
    pub pixels: Option<u32>,
}

/// Surfaces darkening where they meet, in corners and creases and under
/// things on the ground: a soft hint of contact
/// ([`CameraView::ambient_occlusion`]). Only the ambient and sky light
/// darken; lights and their shadows are left as they are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmbientOcclusion {
    /// How dark: 0, not at all; 1, as dark as the corner is closed in; up
    /// to about 4, darker.
    pub intensity: f32,
    /// How far, in metres, a surface darkens one near it.
    pub radius: f32,
}

impl AmbientOcclusion {
    pub const DEFAULT: Self = Self { intensity: 1.0, radius: 0.5 };
}

impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Light brighter than white glowing into what's around it: a lamp's
/// bulb, a glowing sword, the sun on metal ([`CameraView::bloom`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bloom {
    /// How much of the light glows: 0, none; 1, about as much as it gives
    /// off itself, spread wide.
    pub strength: f32,
    /// How bright light must be to glow, in linear light: at 1, only what's
    /// brighter than white (a lit white surface stays as it is), fading in
    /// over half as much again.
    pub threshold: f32,
}

impl Bloom {
    pub const DEFAULT: Self = Self { strength: 1.0, threshold: 1.0 };
}

impl Default for Bloom {
    fn default() -> Self {
        Self::DEFAULT
    }
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
            bloom: None,
            ambient_occlusion: None,
            pixels: None,
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

/// How much a frame may cost, for how it looks: what's drawn, and how
/// finely. A game sets its own, or starts from a level's
/// ([`QualityLevel::quality`]) and changes what it likes. Cameras ask for
/// their effects ([`CameraView::bloom`], [`CameraView::ambient_occlusion`]);
/// these say whether they're drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    /// Whether the scene is drawn into an image of its own (10 bits a
    /// colour, [`Quality::image_format`]), with a last pass after it,
    /// rather than straight to the screen: its brightest
    /// light is kept, for [`Quality::bloom`], and toned down evenly. Bloom
    /// draws into it whether this is on or not.
    pub hdr: bool,
    /// Whether cameras' bloom is drawn.
    pub bloom: bool,
    /// Whether cameras' ambient occlusion is drawn: a depth pass and two
    /// half-size passes more.
    pub ambient_occlusion: bool,
    /// Whether point and spot lights give shiny surfaces a highlight, as
    /// suns always do. Without, a lamp lights a surface evenly, in its
    /// colour (metal in its own, dimmer than its highlight would be), which
    /// keeps the cost down where many overlap (a Note10+ with 63 lamps over
    /// its screen: 54 fps with, 59 without), much as Unity's low tiers
    /// light extra lights per vertex.
    pub lamp_highlights: bool,
    /// How lights' shadows are drawn, or `None` for none at all, whatever
    /// the lights ask.
    pub shadows: Option<ShadowLevel>,
    /// How many samples each pixel is drawn with, which smooths edges
    /// (MSAA): 1 (none), 2 or 4. A device that can't draw a count draws the
    /// next below it. On phones' GPUs the samples stay on the chip, so 4
    /// costs less than it sounds.
    pub samples: u32,
    /// Whether particles with softness fade where they meet what's behind
    /// them, which keeps the scene's depth to read; without, they're hard.
    pub soft_particles: bool,
}

impl Quality {
    /// The cheapest: straight to the screen, hard shadows from the sun and
    /// one spot light, no effects.
    pub const LOW: Self = Self {
        samples: 4,
        hdr: false,
        bloom: false,
        ambient_occlusion: false,
        lamp_highlights: false,
        soft_particles: false,
        shadows: Some(ShadowLevel {
            cascades: 1,
            map_size: 1024,
            filter: ShadowFilter::Hard,
            spot_maps: 1,
            spot_map_size: 512,
            distance: 40.0,
        }),
    };

    /// Between: the scene image, bloom, soft shadows in two cascades. For
    /// a phone that keeps up with it; no device picks it itself.
    pub const MEDIUM: Self = Self {
        samples: 4,
        hdr: true,
        bloom: true,
        ambient_occlusion: false,
        lamp_highlights: true,
        soft_particles: true,
        shadows: Some(ShadowLevel {
            cascades: 2,
            map_size: 2048,
            filter: ShadowFilter::Soft,
            spot_maps: 2,
            spot_map_size: 1024,
            distance: 60.0,
        }),
    };

    /// Apple's GPUs, and desktops': everything, with softer shadows in
    /// three cascades.
    pub const HIGH: Self = Self {
        samples: 4,
        hdr: true,
        bloom: true,
        ambient_occlusion: true,
        lamp_highlights: true,
        soft_particles: true,
        shadows: Some(ShadowLevel {
            cascades: 3,
            map_size: 2048,
            filter: ShadowFilter::Softer,
            spot_maps: 4,
            spot_map_size: 1024,
            distance: 100.0,
        }),
    };

    /// The format of the image the scene is drawn into, with a last pass
    /// after it: 10 bits a colour, with [`Quality::hdr`] or bloom, or
    /// `None`, straight to the screen. It holds colour already tone mapped
    /// (0 to 1), so 10 bits are enough above the darkest tones, which are
    /// dithered (forward.wgsl, `fy_dither`). Half floats cost the Note10+
    /// about 2 ms more of a 22 ms frame at medium (stage 14, RN12).
    pub(crate) fn image_format(&self) -> Option<wgpu::TextureFormat> {
        (self.hdr || self.bloom).then_some(wgpu::TextureFormat::Rgb10a2Unorm)
    }

    /// Why it can't be drawn, if it can't: a shadow level out of range.
    pub fn invalid(&self) -> Option<String> {
        if ![1, 2, 4].contains(&self.samples) {
            return Some(format!("{} samples: 1, 2 or 4", self.samples));
        }
        let s = self.shadows?;
        let size = |n: u32| (256..=4096).contains(&n) && n.is_power_of_two();
        if !(1..=3).contains(&s.cascades) {
            Some(format!("{} cascades: 1 to 3", s.cascades))
        } else if !size(s.map_size) || !size(s.spot_map_size) {
            Some(format!("shadow maps of {} and {}: powers of two, 256 to 4096", s.map_size, s.spot_map_size))
        } else if s.spot_maps > 4 {
            Some(format!("{} spot maps: 0 to 4", s.spot_maps))
        } else if !(s.distance.is_finite() && s.distance > 0.0) {
            Some(format!("a shadow distance of {}: more than 0", s.distance))
        } else {
            None
        }
    }
}

/// The three [`Quality`]s an engine starts with, one of which a device
/// picks ([`QualityLevel::for_adapter`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QualityLevel {
    Low,
    Medium,
    High,
}

impl QualityLevel {
    /// Its settings: [`Quality::LOW`], [`Quality::MEDIUM`] or
    /// [`Quality::HIGH`].
    pub fn quality(self) -> Quality {
        match self {
            Self::Low => Quality::LOW,
            Self::Medium => Quality::MEDIUM,
            Self::High => Quality::HIGH,
        }
    }

    /// The level for a GPU: high on Apple's and on desktops', low on
    /// phones' (Mali, Adreno), on a software one, and on one not known.
    /// Phones were medium until the Android frame pacing work found a
    /// Galaxy Note10+ couldn't hold 60 at it (docs/android-pacing-report.md).
    pub fn for_adapter(info: &wgpu::AdapterInfo) -> Self {
        Self::for_adapter_on(info, cfg!(target_os = "android"))
    }

    /// [`QualityLevel::for_adapter`], on Android or not. On Android every
    /// GPU is a phone's, low whoever makes it: one isn't high because its
    /// maker isn't known.
    pub fn for_adapter_on(info: &wgpu::AdapterInfo, android: bool) -> Self {
        const APPLE: u32 = 0x106B;
        const ARM: u32 = 0x13B5;
        const QUALCOMM: u32 = 0x5143;
        use wgpu::DeviceType::Cpu;
        match (info.device_type, info.vendor) {
            (Cpu, _) => Self::Low,
            _ if android => Self::Low,
            (_, ARM | QUALCOMM) => Self::Low,
            (_, APPLE) => Self::High,
            (_, _) if info.backend == wgpu::Backend::Metal => Self::High,
            (wgpu::DeviceType::DiscreteGpu | wgpu::DeviceType::IntegratedGpu, _) => Self::High,
            _ => Self::Low,
        }
    }
}

/// How shadows are drawn ([`Quality::shadows`]).
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
    /// Whether what's behind shows through, and how.
    pub blend: Blend,
    pub textures: MaterialTextures,
    /// Its own shader, or `None` for the built-in one.
    pub shader: Option<ShaderUse>,
    /// A line drawn around it, as in a cartoon, or `None`. Opaque surfaces
    /// only: a cut-out or see-through one has none.
    pub outline: Option<Outline>,
}

/// A line around an object: its mesh drawn again behind it, inside out and
/// [`width`](Self::width) bigger all round, in one colour. Lit by nothing,
/// fogged as the object is. Moved and bent as the object is, by its
/// shader's `vertex` and its skeleton. A mesh whose faces don't share their
/// corners' normals (a cube's) shows gaps at its corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outline {
    /// How far out it reaches, in metres.
    pub width: f32,
    /// Linear RGB.
    pub color: [f32; 3],
}

/// How a surface covers what's behind it.
///
/// See-through surfaces are drawn after everything solid, the furthest
/// object first. Within one object the order is its triangles', so an
/// object that overlaps itself may draw its far side over its near one;
/// split it in two if that shows. They hide nothing behind them from
/// what's drawn later, and cast no shadow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Blend {
    /// Covers it, or with an alpha cutoff, covers it where its alpha
    /// reaches the cutoff.
    #[default]
    Opaque,
    /// Laid over it as glass, water or a fading object is: everything it
    /// shows (its light, highlights and glow) times its alpha, and what's
    /// behind times one minus its alpha (glTF's BLEND). With an alpha
    /// cutoff, it isn't there where its alpha is below it.
    Transparent,
    /// Adds its light, times its alpha, to what's behind: for glows,
    /// flames, sparks and beams.
    Additive,
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
        blend: Blend::Opaque,
        textures: MaterialTextures::NONE,
        shader: None,
        outline: None,
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
    /// For a mesh a skeleton bends: where its joints start in
    /// [`RenderScene::joints`]. Its vertices then go where their joints
    /// take them, and `model` doesn't move them (glTF's rule). `None`, or
    /// a mesh with no joints, draws it unbent, placed by `model`.
    pub skin: Option<u32>,
    /// The object's own four numbers, which a material's shader reads as
    /// its `PerObject` values (stage 10). Zeros for most.
    pub own: [f32; 4],
    /// Which of the mesh's submeshes (`fynkel_mesh::MeshData::submeshes`)
    /// it draws: 0, the whole of a mesh that doesn't split. One the mesh
    /// hasn't draws nothing.
    pub submesh: u32,
}

impl Draw {
    /// `mesh` placed by `model`, casting and receiving shadows.
    pub fn new(mesh: MeshKey, model: Mat4, material: PbrMaterial) -> Self {
        Self { mesh, model, material, cast_shadows: true, receive_shadows: true, skin: None, own: [0.0; 4], submesh: 0 }
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

/// A particle: a square facing the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    /// Its centre, in the world.
    pub position: Vec3,
    /// Across, in metres.
    pub size: f32,
    /// Linear RGBA, times the material's base colour.
    pub color: [f32; 4],
    /// Its turn round the way to the camera (a mesh's, round up), in
    /// radians.
    pub rotation: f32,
    /// How it moves, in metres a second: a stretched one is long that way.
    pub velocity: Vec3,
    /// Its frame of the material's texture sheet ([`ParticleDraw::sheet`]).
    pub frame: u32,
    /// How far through its life, 0 to 1, and a number of its own, 0 to 1:
    /// a shader's `particle_age()` and `particle_random()`.
    pub t: f32,
    pub random: f32,
}

/// How a system's particles are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ParticleMode {
    /// A square facing the camera.
    #[default]
    Billboard,
    /// Facing the camera, long the way it moves: `length_scale` times its
    /// size plus `speed_scale` times its speed.
    Stretched { length_scale: f32, speed_scale: f32 },
    /// Lying flat, facing up.
    Horizontal,
    /// Standing up, turned round up to face the camera.
    Vertical,
    /// A mesh, its size across a metre of it, turned round up.
    Mesh(MeshKey),
    /// A trail: a strip through the particles in order, facing the
    /// camera, each particle a point on it, its size the strip's width
    /// there, its velocity the way the strip goes, and its `t` how far
    /// along it is (the texture's u).
    Trail,
    /// Letters of flat text (stage 15), from [`RenderScene::glyphs`]: the
    /// material's base colour texture is their font's atlas of distances,
    /// one byte a pixel. `on_top`: drawn over everything, after the rest.
    Glyphs { on_top: bool },
    /// Sprites (stage 15), from [`RenderScene::sprites`]: the material's
    /// base colour texture on each, times its colour. Where the texture's
    /// alpha times the colour's is below `cutoff` (above 0), cut out.
    Sprites { cutoff: f32 },
}

/// A letter of flat text: a square of its font's atlas, as TextMeshPro
/// draws one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// Its middle, in the world.
    pub center: Vec3,
    /// Half its width and half its height, in the world: its right and
    /// its up.
    pub right: Vec3,
    pub up: Vec3,
    /// Where it is in the atlas: u and v scale, then offset, 0 to 1.
    pub uv: [f32; 4],
    /// Linear RGBA, not premultiplied: the letter's, and its outline's.
    pub color: [f32; 4],
    pub outline_color: [f32; 4],
    /// How wide the outline is, 0 to 1 of the atlas's reach outside the
    /// letter, and how soft the edges are, 0 to 1.
    pub outline: f32,
    pub softness: f32,
}

/// A quad of a sprite: a part of its texture, as Unity's Sprite Renderer
/// draws one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    /// Its middle, in the world.
    pub center: Vec3,
    /// Half its width and half its height, in the world: its right and
    /// its up.
    pub right: Vec3,
    pub up: Vec3,
    /// The part of its texture it shows: u and v scale, then offset, 0 to
    /// 1; a negative scale mirrors it.
    pub uv: [f32; 4],
    /// Linear RGBA, not premultiplied, times the texture's.
    pub color: [f32; 4],
}

/// A particle system's particles, drawn with `material`: unlit, its
/// texture and base colour times each particle's colour, plus its
/// emission; or `lit` as a surface facing the camera; or by the
/// material's own shader, its colour the particle's times the material's.
/// They're drawn among the blended draws, furthest first by their bounds'
/// centre; a transparent system's particles furthest first among
/// themselves too. A material that doesn't blend draws them as
/// transparent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleDraw {
    pub material: PbrMaterial,
    pub mode: ParticleMode,
    /// The material's texture as a grid of so many columns and rows, each
    /// particle showing its [`Particle::frame`]; `[1, 1]` for the whole.
    pub sheet: [u32; 2],
    /// Lit, with the built-in material: once a particle, at its middle
    /// (a trail's, at each point), as a matte surface; a mesh's pixel by
    /// pixel. A material's own shader is always used as it says.
    pub lit: bool,
    /// How far, in metres, a particle fades out in front of the solid
    /// scene behind it; 0 for not at all.
    pub softness: f32,
    /// The biggest particle's size: a mesh particle reaches that times
    /// the mesh's own reach from its origin.
    pub largest: f32,
    /// Its first particle in [`RenderScene::particles`], and how many.
    pub first: u32,
    pub count: u32,
    /// A ball round them all, for culling and sorting.
    pub center: Vec3,
    pub radius: f32,
    /// Its sorting layer and its order in it, as Unity's: the blended are
    /// drawn by these first, lower first, then furthest first. `[0, 0]`
    /// for all but sprites.
    pub sorting: [i32; 2],
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

impl SkyKind {
    /// Whether the light worked out for one is the other's too: any two
    /// gradients (theirs is worked out for unit colours, and coloured as
    /// it's read), or the same image however turned or bright (its light
    /// is worked out unturned at intensity 1, and turned and scaled as
    /// it's read).
    pub(crate) fn same_light(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Gradient { .. }, Self::Gradient { .. }) => true,
            (Self::Image { image: a, .. }, Self::Image { image: b, .. }) => Arc::ptr_eq(a, b),
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
    /// The joints of the skeletons that bend skinned draws
    /// ([`Draw::skin`]), each where it takes its meshes' vertices: its
    /// world matrix times its inverse bind matrix. A draw's mesh says how
    /// many it uses from where its skin starts; draws bent by the same
    /// skeleton share them.
    pub joints: Vec<Mat4>,
    /// Drawn after the draws, over them, one pixel wide.
    pub lines: Vec<Line>,
    /// Every particle system's particles, each system's together.
    pub particles: Vec<Particle>,
    /// The particle systems drawn: each its run of [`Self::particles`], or
    /// of [`Self::glyphs`] for [`ParticleMode::Glyphs`], or of
    /// [`Self::sprites`] for [`ParticleMode::Sprites`].
    pub particle_draws: Vec<ParticleDraw>,
    /// Every flat text's letters, each draw's together.
    pub glyphs: Vec<Glyph>,
    /// Every sprite's quads, each draw's together.
    pub sprites: Vec<Sprite>,
    /// Behind everything, and lighting it, as well as [`Self::ambient`].
    /// `None`: the camera's clear colour, and the ambient light alone.
    pub sky: Option<Sky>,
    pub fog: Option<Fog>,
    /// What to draw, and how finely; `None` for the device's level's
    /// ([`Kit::device_quality`](crate::Kit::device_quality)).
    pub quality: Option<Quality>,
    /// The game's time, in seconds, and this frame's step of it, as
    /// shaders see it (`frame_time()`, `frame_delta()`).
    pub time: [f32; 2],
    /// The values of the materials drawn with their own shaders, each from
    /// its [`ShaderUse::values`], laid out as its shader's `Values`, in
    /// blocks of 48 bytes (a joint's room: they go after the joints).
    pub shader_values: Vec<[[f32; 4]; 3]>,
    /// The camera's screen effects, run over the finished picture in
    /// order. One whose shader isn't there, or isn't an effect's
    /// ([`CustomShader::effect`](crate::CustomShader::effect)), is left out.
    pub effects: Vec<ScreenEffect>,
}

/// A screen effect: its shader (an effect's), the first of its values'
/// blocks in [`RenderScene::shader_values`], and its textures, in the
/// order its shader declares them (one not given reads white).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenEffect {
    pub shader: ShaderKey,
    pub values: u32,
    pub textures: [Option<TextureKey>; crate::MAX_TEXTURES],
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
        self.joints.clear();
        self.lines.clear();
        self.particles.clear();
        self.particle_draws.clear();
        self.glyphs.clear();
        self.sprites.clear();
        self.sky = None;
        self.fog = None;
        self.quality = None;
        self.time = [0.0; 2];
        self.shader_values.clear();
        self.effects.clear();
    }
}
