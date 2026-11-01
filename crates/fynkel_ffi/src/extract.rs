//! Copies what the renderer needs out of the world into a [`RenderScene`]
//! (stage 1 plan §3.5): resolved, linear values the render worker can draw
//! without looking anything up.
//!
//! The rules:
//! - The camera is the view's own when it has one ([`Extract::camera`]):
//!   drawn from while it's shown and has a `Camera` and a `Transform`, and
//!   nothing is drawn otherwise. Else it's the one on the object with the
//!   lowest id that has both. Its transform's scale is ignored. A shake
//!   ([`Extract::shake`]) moves it when it's drawn, not in the world. One
//!   whose near clip isn't below its far draws nothing, so the frame is
//!   refused with a message instead.
//! - Every directional, point and spot light with a `Transform` goes to the
//!   renderer, in the order of their ids. A directional light is a sun,
//!   shining along its world forward (-Z); a point light shines from its
//!   world position; a spot light from there along its forward. Without a
//!   transform, a light has no place and is ignored. The renderer chooses
//!   which light what (its `RenderScene::lights`).
//! - Every ambient light adds to the ambient term.
//! - An object is drawn when it has a `Transform` and a `MeshRenderer`
//!   with both a mesh and a material, and its layer is in the camera's
//!   culling mask (stage 4 plan §3.9). Lights light every layer.
//! - A particle system is drawn when its object is shown and in the
//!   camera's culling mask and it has a material and particles: one draw
//!   of them all, round the box they're in.
//! - A trail is drawn when its object is shown and in the camera's culling
//!   mask and it has a material and two points: one draw of its strip.
//! - The world's debug lines are drawn over the scene as they are.
//! - A texture is uploaded once for each colour space a material reads it
//!   in, the first frame a material uses it that way, and freed when the
//!   world removes it. Once for the world, however many views it has: what
//!   the GPU has is the world's [`Ledger`].
//! - A material's own shader (stage 10) is uploaded when the world adds
//!   it, again each time it's set, and freed when it's removed. A material
//!   with one gives its values to the scene, in 48-byte blocks; its
//!   textures go up in the colour spaces its shader reads them in.
//! - Only shown objects count, for all of the above: an inactive object and
//!   everything below it are left out (stage 3 plan §3.3).
//! - Everything is placed by its world transform: its own after its
//!   parents' (stage 2 plan §3.1). The camera and the lights take only the
//!   position and rotation from it; a parent's scale doesn't stretch them
//!   (a light's range is its own).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fynkel_render::glam::{Mat4, Vec3};
use fynkel_render::{
    AmbientOcclusion, Bloom, CameraView, ColorSpace, CustomShader, Draw, Fog, Glyph, Light as RenderLight, Line,
    MAX_TEXTURES, MaterialTextures, MeshKey, Outline, Particle, ParticleDraw, ParticleMode, PbrMaterial, Quality,
    RenderScene, ScreenEffect, ShaderKey, ShaderUse, Sky, TextureKey,
};
use fynkel_world::{
    AtlasPixels, Camera, Color, DrawnParticle, Facing, FontId, ImageData, Light, LightKind, MAX_TRAIL_POINTS, Material,
    MaterialId, MeshData, MeshId, MeshRenderer, ObjectId, Quat, RenderMode, ShaderId, TextureId, Transform, World,
    WorldMatrices,
};

/// A change to the meshes and textures the renderer holds. An upload
/// shares the world's data; nothing is copied until the GPU upload.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResourceOp {
    UploadMesh(MeshKey, Arc<MeshData>),
    RemoveMesh(MeshKey),
    UploadTexture(TextureKey, ColorSpace, Arc<ImageData>),
    /// In every colour space. Sent once for each space the texture was
    /// uploaded in; removing again does nothing.
    RemoveTexture(TextureKey),
    /// A font's atlas of distances, new or grown.
    UploadAtlas(TextureKey, Arc<AtlasPixels>),
    /// A material's own shader, new or set again.
    UploadShader(ShaderKey, Arc<CustomShader>),
    RemoveShader(ShaderKey),
}

/// Which meshes and textures the GPU has, or is about to: those uploaded,
/// so each goes up once and is freed when the world removes it.
#[derive(Debug, Default)]
pub(crate) struct Uploaded {
    /// Each mesh's version uploaded.
    meshes: HashMap<MeshId, u64>,
    textures: HashSet<(TextureId, ColorSpace)>,
    /// Each shader's version uploaded.
    shaders: HashMap<ShaderId, u64>,
    /// Each font's atlas version uploaded.
    atlases: HashMap<FontId, u64>,
}

/// A world's [`Uploaded`], shared by its views, and the changes still to
/// be made on the GPU, oldest first.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    pub(crate) uploaded: Uploaded,
    pub(crate) ops: Vec<ResourceOp>,
}

/// One view's extract.
#[derive(Debug, Default)]
pub(crate) struct Extract {
    /// What its own renderer has, when it shares nothing ([`Extract::run`]).
    #[cfg(test)]
    own: Uploaded,
    matrices: WorldMatrices,
    /// The view's own camera, or `None` for the world's main one.
    pub(crate) camera: Option<ObjectId>,
    /// The world's camera shake: how far its camera is moved, in the
    /// world, when this view draws from it.
    pub(crate) shake: Option<(ObjectId, Vec3)>,
    /// The world's quality, or `None` for the device's level's.
    pub(crate) quality: Option<Quality>,
    /// The world's sky and fog.
    pub(crate) sky: Option<Sky>,
    pub(crate) fog: Option<Fog>,
    /// The lights found, by id, before they're put in order. Kept so a
    /// frame allocates nothing.
    lights: Vec<(ObjectId, RenderLight)>,
    /// Each material as the renderer takes it, worked out once a frame:
    /// objects are many, materials few.
    materials: HashMap<MaterialId, PbrMaterial>,
    /// Where each skeleton's joints start in the scene's, found this frame
    /// ([`fynkel_world::Skin::key`]), or `None` for one missing a joint.
    skins: HashMap<usize, Option<u32>>,
    /// A particle system's particles as it gives them, before they go to
    /// the scene. Kept so a frame allocates nothing.
    drawn: Vec<DrawnParticle>,
    /// Flat text's draws this frame: each font's texts over everything or
    /// not, lit or not. Kept so a frame allocates nothing.
    text_groups: Vec<(FontId, bool, bool)>,
}

pub(crate) fn mesh_key(id: MeshId) -> MeshKey {
    MeshKey(id.raw())
}

pub(crate) fn texture_key(id: TextureId) -> TextureKey {
    TextureKey(id.raw())
}

/// A font's atlas's key among the textures: apart from the world's own.
pub(crate) fn atlas_key(id: FontId) -> TextureKey {
    TextureKey(1 << 62 | id.raw())
}

pub(crate) fn shader_key(id: ShaderId) -> ShaderKey {
    ShaderKey(id.raw())
}

/// The renderer's shader the world keeps for `id`, if it's one (it always
/// is: the world's shaders are made by `fy_shader_add`).
pub(crate) fn compiled(world: &World, id: ShaderId) -> Option<Arc<CustomShader>> {
    Arc::clone(&world.shader(id).ok()?.compiled).downcast::<CustomShader>().ok()
}

/// A material's textures as the renderer names them.
fn texture_keys(m: &Material) -> MaterialTextures {
    let key = |id: Option<TextureId>| id.map(texture_key);
    MaterialTextures {
        base_color: key(m.base_color_texture),
        normal: key(m.normal_texture),
        metallic_roughness: key(m.metallic_roughness_texture),
        occlusion: key(m.occlusion_texture),
        emissive: key(m.emissive_texture),
    }
}

impl Extract {
    /// Fills `scene` from `world` (emptying it first) and adds the mesh
    /// changes since the last run to `ops`, for a renderer of its own.
    /// Allocates nothing once `scene` and `ops` have grown to size and the
    /// meshes are unchanged.
    #[cfg(test)]
    pub(crate) fn run(&mut self, world: &World, scene: &mut RenderScene, ops: &mut Vec<ResourceOp>) {
        let mut own = std::mem::take(&mut self.own);
        let filled = self.fill(world, scene, &mut own, ops);
        self.own = own;
        filled.expect("a camera that can draw");
    }

    /// [`Extract::run`] for a view that shares its world's meshes and
    /// textures: the changes go to the world's `ledger`, once for all its
    /// views. Fails, saying why, when the camera can't draw (near ≥ far):
    /// `scene` is then not to be drawn.
    pub(crate) fn run_shared(
        &mut self,
        world: &World,
        scene: &mut RenderScene,
        ledger: &mut Ledger,
    ) -> Result<(), String> {
        let Ledger { uploaded, ops } = ledger;
        self.fill(world, scene, uploaded, ops)
    }

    fn fill(
        &mut self,
        world: &World,
        scene: &mut RenderScene,
        uploaded: &mut Uploaded,
        ops: &mut Vec<ResourceOp>,
    ) -> Result<(), String> {
        scene.clear();

        uploaded.meshes.retain(|&id, _| {
            let kept = world.mesh(id).is_ok();
            if !kept {
                ops.push(ResourceOp::RemoveMesh(mesh_key(id)));
            }
            kept
        });
        for (id, mesh, version) in world.meshes() {
            match uploaded.meshes.insert(id, version) {
                None => ops.push(ResourceOp::UploadMesh(mesh_key(id), Arc::clone(mesh))),
                Some(was) if was != version => {
                    // Changed again before its last change went up: that
                    // one is replaced, so a mesh changed every frame never
                    // queues up behind a slow worker.
                    let key = mesh_key(id);
                    let waiting = ops.iter_mut().rev().find_map(|op| match op {
                        ResourceOp::UploadMesh(k, waiting) if *k == key => Some(waiting),
                        _ => None,
                    });
                    match waiting {
                        Some(waiting) => *waiting = Arc::clone(mesh),
                        None => ops.push(ResourceOp::UploadMesh(key, Arc::clone(mesh))),
                    }
                }
                Some(_) => {}
            }
        }

        // Removals first, so a key is never freed after its new upload.
        uploaded.textures.retain(|&(id, _)| {
            let kept = world.texture(id).is_ok();
            if !kept {
                ops.push(ResourceOp::RemoveTexture(texture_key(id)));
            }
            kept
        });
        uploaded.shaders.retain(|&id, _| {
            let kept = world.shader(id).is_ok();
            if !kept {
                ops.push(ResourceOp::RemoveShader(shader_key(id)));
            }
            kept
        });
        uploaded.atlases.retain(|&id, _| {
            let kept = world.font(id).is_ok();
            if !kept {
                ops.push(ResourceOp::RemoveTexture(atlas_key(id)));
            }
            kept
        });
        for (id, font) in world.fonts() {
            let pixels = font.pixels();
            if uploaded.atlases.insert(id, pixels.version) != Some(pixels.version) {
                ops.push(ResourceOp::UploadAtlas(atlas_key(id), Arc::clone(pixels)));
            }
        }
        for (id, _, version) in world.shaders() {
            if uploaded.shaders.insert(id, version) != Some(version)
                && let Some(shader) = compiled(world, id)
            {
                ops.push(ResourceOp::UploadShader(shader_key(id), shader));
            }
        }
        scene.time = world.shader_time();

        // Materials are few next to objects, so looking at each one's
        // slots, and resolving it, every frame costs little.
        self.materials.clear();
        let mut upload = |id: TextureId, space: ColorSpace| {
            if uploaded.textures.insert((id, space)) {
                let image = world.texture(id).expect("the world refuses a material with a missing texture");
                ops.push(ResourceOp::UploadTexture(texture_key(id), space, Arc::clone(image)));
            }
        };
        for (id, material) in world.materials() {
            let mut m = pbr(material);
            for (key, space) in texture_keys(material).slots() {
                let Some(key) = key else { continue };
                upload(TextureId::from_raw(key.0).expect("made from a TextureId"), space);
            }
            if let Some(shader) = material.shader.and_then(|s| compiled(world, s).map(|c| (s, c))) {
                let (shader, compiled) = shader;
                let mut textures = [None; MAX_TEXTURES];
                for (i, declared) in compiled.textures.iter().enumerate() {
                    if let Some(texture) = material.shader_textures[i] {
                        upload(texture, declared.space);
                        textures[i] = Some(texture_key(texture));
                    }
                }
                let values = scene.shader_values.len() as u32;
                push_values(&mut scene.shader_values, world.material_values(id).unwrap_or_default());
                m.shader = Some(ShaderUse { shader: shader_key(shader), values, textures, reach: material.reach });
            }
            self.materials.insert(id, m);
        }

        self.matrices.update(world);
        let matrices = &self.matrices;
        // Where a shown object is and which way it faces, scale left out:
        // the rotation as Dart's, which a parent scaled to 0 doesn't spoil.
        let placed = |id: ObjectId, t: &Transform| -> Option<(Quat, Vec3)> {
            let position = matrices.world(world, id, t)?.w_axis.truncate();
            Some((world.world_transform(id).ok()?.rotation, position))
        };

        let mut camera: Option<(ObjectId, Camera, Quat, Vec3)> = None;
        let own = self.camera;
        world.for_each2::<Camera, Transform>(|id, c, t| {
            if own.is_none_or(|own| own == id)
                && camera.is_none_or(|(best, ..)| id < best)
                && let Some((rotation, position)) = placed(id, t)
            {
                camera = Some((id, *c, rotation, position));
            }
        });
        if let Some((id, c, ..)) = camera
            && c.near >= c.far
        {
            let name = world.name(id).ok().flatten().map_or_else(|| format!("{id:?}"), |n| format!("'{n}'"));
            return Err(format!(
                "the camera on {name} has near {} and far {}: near must be below far, or it draws nothing",
                c.near, c.far
            ));
        }
        let shake = self.shake;
        let camera = camera.map(|(id, c, rotation, position)| match shake {
            Some((shaken, by)) if shaken == id => (id, c, rotation, position + by),
            _ => (id, c, rotation, position),
        });
        // The layers drawn: all without a camera, which draws nothing anyway.
        let mask = camera.map_or(u32::MAX, |(_, c, ..)| c.culling_mask);
        // The camera's screen effects, their values after the materials'.
        if let Some((id, ..)) = camera {
            for effect in world.camera_effects(id) {
                let Some(shader) = compiled(world, effect.shader).filter(|s| s.effect) else { continue };
                let mut textures = [None; MAX_TEXTURES];
                for (i, declared) in shader.textures.iter().enumerate() {
                    if let Some(texture) = effect.textures[i] {
                        if uploaded.textures.insert((texture, declared.space)) {
                            let image =
                                world.texture(texture).expect("the world refuses an effect with a missing texture");
                            ops.push(ResourceOp::UploadTexture(
                                texture_key(texture),
                                declared.space,
                                Arc::clone(image),
                            ));
                        }
                        textures[i] = Some(texture_key(texture));
                    }
                }
                let values = scene.shader_values.len() as u32;
                push_values(&mut scene.shader_values, &effect.values);
                scene.effects.push(ScreenEffect { shader: shader_key(effect.shader), values, textures });
            }
        }
        scene.camera = camera.map(|(_, c, rotation, position)| CameraView {
            view: Mat4::from_rotation_translation(rotation, position).inverse(),
            position,
            fov_y: c.fov_y.to_radians(),
            orthographic: c.orthographic.then_some(c.size),
            near: c.near,
            far: c.far,
            clear_color: rgb(c.clear_color),
            sky: c.show_sky,
            bloom: c.bloom.then_some(Bloom { strength: c.bloom_strength, threshold: c.bloom_threshold }),
            ambient_occlusion: c.ambient_occlusion.then_some(AmbientOcclusion {
                intensity: c.ambient_occlusion_intensity,
                radius: c.ambient_occlusion_radius,
            }),
            pixels: (c.pixels > 0).then_some(c.pixels),
        });

        scene.quality = self.quality;
        scene.sky.clone_from(&self.sky);
        scene.fog = self.fog;
        let lights = &mut self.lights;
        lights.clear();
        world.for_each2::<Light, Transform>(|id, light, t| {
            if light.kind == LightKind::Ambient {
                return;
            }
            let Some((rotation, position)) = placed(id, t) else { return };
            let color = Vec3::from(rgb(light.color)) * light.intensity;
            let forward = rotation * Vec3::NEG_Z;
            lights.push((
                id,
                match light.kind {
                    LightKind::Point => RenderLight::point(position, light.range, color),
                    LightKind::Spot => RenderLight::spot(
                        position,
                        forward,
                        light.range,
                        light.spot_angle.to_radians(),
                        light.softness,
                        color,
                    ),
                    _ => RenderLight::sun(forward, color),
                }
                .with_shadows(light.shadows),
            ));
        });
        lights.sort_unstable_by_key(|&(id, _)| id);
        scene.lights.extend(lights.iter().map(|&(_, light)| light));
        world.for_each::<Light>(|id, light| {
            if light.kind == LightKind::Ambient && matrices.shown(world, id) {
                scene.ambient += Vec3::from(rgb(light.color)) * light.intensity;
            }
        });

        let materials = &self.materials;
        // Objects next to each other often share a material: the last one
        // found is kept.
        let mut last: Option<(MaterialId, PbrMaterial)> = None;
        let skins = &mut self.skins;
        skins.clear();
        let has_skins = world.has_skins();
        let has_submesh_materials = world.has_submesh_materials();
        world.for_each2::<MeshRenderer, Transform>(|id, r, t| {
            let (Some(mesh), Some(material)) = (r.mesh, r.material) else { return };
            if !world.in_mask(id, mask) {
                return;
            }
            let Some(model) = matrices.world(world, id, t) else { return };
            let material = match last {
                Some((at, m)) if at == material => m,
                _ => {
                    // The world refuses a renderer pointing at a missing
                    // material, so this always succeeds.
                    let Some(&m) = materials.get(&material) else { return };
                    last = Some((material, m));
                    m
                }
            };
            let mut material = material;
            if r.own_color {
                material.base_color = r.color.to_linear();
            }
            // A skinned mesh's joints, each skeleton's once: their world
            // matrices times their inverse binds.
            let skin = match if has_skins { world.skin(id) } else { None } {
                None => None,
                Some(skin) => *skins.entry(skin.key()).or_insert_with(|| {
                    let first = scene.joints.len();
                    for (&joint, &inverse_bind) in skin.joints().iter().zip(skin.inverse_binds()) {
                        let Some(m) = matrices.matrix(world, joint) else {
                            // A joint destroyed: drawn as bound.
                            scene.joints.truncate(first);
                            return None;
                        };
                        scene.joints.push(m * inverse_bind);
                    }
                    Some(first as u32)
                }),
            };
            let draw = Draw {
                skin,
                mesh: mesh_key(mesh),
                model,
                material,
                cast_shadows: r.cast_shadows,
                receive_shadows: r.receive_shadows,
                own: r.own,
                submesh: 0,
            };
            scene.draws.push(draw);
            // The submeshes after the first, each with its own material.
            if has_submesh_materials {
                for (n, id) in world.submesh_materials(id).iter().enumerate() {
                    let Some(&m) = materials.get(id) else { continue };
                    let mut material = m;
                    if r.own_color {
                        material.base_color = r.color.to_linear();
                    }
                    scene.draws.push(Draw { material, submesh: n as u32 + 1, ..draw });
                }
            }
        });

        // Room for as many particles and trail points as there can be, as
        // each system is a fixed pool: how many there are now goes up and
        // down, and a scene buffer grown only to the most it has seen grows
        // again on a busier frame, whenever the worker happens to fill it.
        // Reserved once, it grows only when the settings do.
        if world.has_particles() || world.has_trails() {
            let systems = world
                .particle_systems()
                .filter(|(_, s)| s.settings().material.is_some_and(|m| self.materials.contains_key(&m)))
                .map(|(_, s)| s.settings().max_particles as usize);
            let trails = world
                .trails()
                .filter(|(_, t)| t.settings().material.is_some_and(|m| self.materials.contains_key(&m)))
                .map(|_| MAX_TRAIL_POINTS);
            let most = systems.chain(trails).sum::<usize>();
            scene.particles.reserve(most);
        }

        // Particle systems with a material, each one draw of its particles.
        if world.has_particles() {
            for (id, system) in world.particle_systems() {
                let Some(&material) = system.settings().material.and_then(|m| self.materials.get(&m)) else {
                    continue;
                };
                if !matrices.shown(world, id) || !world.in_mask(id, mask) {
                    continue;
                }
                let Some(emitter) = matrices.matrix(world, id) else { continue };
                self.drawn.clear();
                let Some((lo, hi)) = system.drawn(emitter, &mut self.drawn) else { continue };
                let first = scene.particles.len() as u32;
                let mut largest = 0.0f32;
                scene.particles.extend(self.drawn.iter().map(|p| {
                    largest = largest.max(p.size);
                    Particle {
                        position: p.position,
                        size: p.size,
                        color: p.color,
                        rotation: p.rotation,
                        velocity: p.velocity,
                        frame: p.frame,
                        t: p.t,
                        random: p.random,
                    }
                }));
                let settings = system.settings();
                scene.particle_draws.push(ParticleDraw {
                    material,
                    mode: match settings.render_mode {
                        RenderMode::Billboard => ParticleMode::Billboard,
                        RenderMode::Stretched { length_scale, speed_scale } => {
                            ParticleMode::Stretched { length_scale, speed_scale }
                        }
                        RenderMode::Horizontal => ParticleMode::Horizontal,
                        RenderMode::Vertical => ParticleMode::Vertical,
                        RenderMode::Mesh(mesh) => ParticleMode::Mesh(mesh_key(mesh)),
                    },
                    sheet: settings.texture_sheet.as_ref().map_or([1, 1], |t| [t.columns, t.rows]),
                    lit: settings.lit,
                    softness: settings.softness,
                    largest,
                    first,
                    count: self.drawn.len() as u32,
                    center: (lo + hi) * 0.5,
                    radius: (hi - lo).length() * 0.5,
                });
            }
        }

        // Trails with a material, each one draw of its strip.
        if world.has_trails() {
            for (id, trail) in world.trails() {
                let Some(&material) = trail.settings().material.and_then(|m| self.materials.get(&m)) else {
                    continue;
                };
                if !matrices.shown(world, id) || !world.in_mask(id, mask) {
                    continue;
                }
                self.drawn.clear();
                let Some((lo, hi)) = trail.drawn(&mut self.drawn) else { continue };
                let first = scene.particles.len() as u32;
                scene.particles.extend(self.drawn.iter().map(|p| Particle {
                    position: p.position,
                    size: p.size,
                    color: p.color,
                    rotation: 0.0,
                    velocity: p.velocity,
                    frame: 0,
                    t: p.t,
                    random: 0.0,
                }));
                scene.particle_draws.push(ParticleDraw {
                    material,
                    mode: ParticleMode::Trail,
                    sheet: [1, 1],
                    lit: false,
                    softness: 0.0,
                    largest: 0.0,
                    first,
                    count: self.drawn.len() as u32,
                    center: (lo + hi) * 0.5,
                    radius: (hi - lo).length() * 0.5,
                });
            }
        }

        // Flat text: each font's letters one draw, over everything or not,
        // lit or not, each text placed as it faces.
        if world.has_texts()
            && let Some((_, _, rotation, eye)) = camera
        {
            let groups = &mut self.text_groups;
            groups.clear();
            let mut letters = 0;
            for (id, text) in world.text_renderers() {
                let s = text.settings();
                groups.push((s.font, s.on_top, s.lit));
                if matrices.shown(world, id) && world.in_mask(id, mask) {
                    letters += text.quads().len();
                }
            }
            groups.sort_unstable();
            groups.dedup();
            scene.glyphs.reserve(letters);
            for &(font, on_top, lit) in groups.iter() {
                let Ok(world_font) = world.font(font) else { continue };
                let (width, height) = (world_font.pixels().width as f32, world_font.pixels().height as f32);
                let first = scene.glyphs.len() as u32;
                let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
                for (id, text) in world.text_renderers() {
                    let s = text.settings();
                    if (s.font, s.on_top, s.lit) != (font, on_top, lit) || text.quads().is_empty() {
                        continue;
                    }
                    if !matrices.shown(world, id) || !world.in_mask(id, mask) {
                        continue;
                    }
                    let Some(model) = matrices.matrix(world, id) else { continue };
                    let basis = facing(model, s.facing, rotation, eye);
                    let (color, outline_color) = (s.color.to_linear(), s.outline_color.to_linear());
                    for q in text.quads() {
                        let [l, b, r, t] = q.rect;
                        let [x, y, w, h] = q.pixels.map(f32::from);
                        let center = basis.transform_point3(Vec3::new((l + r) * 0.5, (b + t) * 0.5, 0.0));
                        let right = basis.transform_vector3(Vec3::new((r - l) * 0.5, 0.0, 0.0));
                        let up = basis.transform_vector3(Vec3::new(0.0, (t - b) * 0.5, 0.0));
                        let reach = right.abs() + up.abs();
                        lo = lo.min(center - reach);
                        hi = hi.max(center + reach);
                        scene.glyphs.push(Glyph {
                            center,
                            right,
                            up,
                            uv: [w / width, h / height, x / width, y / height],
                            color,
                            outline_color,
                            outline: s.outline,
                            softness: s.softness,
                        });
                    }
                }
                let count = scene.glyphs.len() as u32 - first;
                if count == 0 {
                    continue;
                }
                let material = PbrMaterial {
                    blend: fynkel_render::Blend::Transparent,
                    textures: MaterialTextures { base_color: Some(atlas_key(font)), ..MaterialTextures::NONE },
                    ..PbrMaterial::DEFAULT
                };
                scene.particle_draws.push(ParticleDraw {
                    material,
                    mode: ParticleMode::Glyphs { on_top },
                    sheet: [1, 1],
                    lit,
                    softness: 0.0,
                    largest: 0.0,
                    first,
                    count,
                    center: (lo + hi) * 0.5,
                    radius: (hi - lo).length() * 0.5,
                });
            }
        }

        scene.lines.extend(world.debug_lines().iter().map(|l| Line { a: l.a, b: l.b, color: l.color.to_linear() }));
        Ok(())
    }
}

/// Where a text's letters go, from its object's world matrix `model`: as
/// the object turns, or turned to the camera (at `eye`, turned by
/// `camera`) as `facing` says, keeping the object's place and scale.
fn facing(model: Mat4, facing: Facing, camera: Quat, eye: Vec3) -> Mat4 {
    if facing == Facing::None {
        return model;
    }
    let (scale, own, at) = model.to_scale_rotation_translation();
    let turn = match facing {
        Facing::None | Facing::Camera => camera,
        Facing::Upright => {
            let to = eye - at;
            Quat::from_rotation_y(if to.x == 0.0 && to.z == 0.0 { 0.0 } else { to.x.atan2(to.z) })
        }
        // Flat on the ground, its top where the object's forward points.
        Facing::Up => {
            let forward = own * Vec3::NEG_Z;
            let yaw = if forward.x == 0.0 && forward.z == 0.0 { 0.0 } else { (-forward.x).atan2(-forward.z) };
            Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        }
    };
    Mat4::from_scale_rotation_translation(scale, turn, at)
}

/// A material as the renderer takes it: linear colours, emission times its
/// intensity.
fn pbr(m: &Material) -> PbrMaterial {
    PbrMaterial {
        base_color: m.base_color.to_linear(),
        metallic: m.metallic,
        roughness: m.roughness,
        emissive: (Vec3::from(rgb(m.emissive)) * m.emissive_intensity).into(),
        uv_scale: m.uv_scale.into(),
        uv_offset: m.uv_offset.into(),
        alpha_cutoff: m.alpha_cutoff,
        double_sided: m.double_sided,
        blend: match m.blend {
            fynkel_world::Blend::Opaque => fynkel_render::Blend::Opaque,
            fynkel_world::Blend::Transparent => fynkel_render::Blend::Transparent,
            fynkel_world::Blend::Additive => fynkel_render::Blend::Additive,
        },
        textures: texture_keys(m),
        shader: None,
        outline: (m.outline_width > 0.0).then(|| Outline { width: m.outline_width, color: rgb(m.outline_color) }),
    }
}

/// A material's values, laid out as its shader's `Values`, as the scene's
/// 48-byte blocks: the last one padded with zeros.
fn push_values(blocks: &mut Vec<[[f32; 4]; 3]>, bytes: &[u8]) {
    for chunk in bytes.chunks(48) {
        let mut block = [[0f32; 4]; 3];
        for (i, word) in chunk.chunks(4).enumerate() {
            let mut b = [0u8; 4];
            b[..word.len()].copy_from_slice(word);
            block[i / 4][i % 4] = f32::from_le_bytes(b);
        }
        blocks.push(block);
    }
}

/// Linear RGB, alpha dropped.
fn rgb(color: Color) -> [f32; 3] {
    let [r, g, b, _] = color.to_linear();
    [r, g, b]
}

#[cfg(test)]
mod tests;
