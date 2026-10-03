//! `fynkel_preview` — the preview stage's scene (ADR-0030 §3), shared by
//! `fynkel thumbnail` (the Project's tiles) and the editor's Inspector
//! previews, so a tile and a preview always show an asset alike.
//!
//! An asset is put alone into a small world: a prefab or a model's node
//! hierarchy as it is, a mesh with the material its model pairs it with,
//! a material on a shape (the built-in primitives' Sphere by default,
//! ADR-0023; a stand-in sphere made here when the cache has none). What
//! is not the asset's own is taken out of what gets drawn
//! ([`strip`]): its cameras, lights, environment and volumes. The drawn
//! meshes' world bounds are measured ([`world_bounds`]) and framed
//! ([`frame`]) from (1, 0.8, 1) with a 30° lens and a sun from the upper
//! left front of that view — every asset lit alike, whatever it authored.
//!
//! No GPU here: the caller draws the world's extract with its own
//! renderer (the CLI offscreen, the editor through a viewport).

use fynkel_assets::{Assets, Residency};
use fynkel_contracts::{AssetRef, Uuid};
use fynkel_cooked::payload::{MeshPayload, Vertex};
use fynkel_core::{
    ComponentValue, DirectionalLight, Entity, EntityDescription, Extract, MeshRenderer,
    SceneDescription, Transform, World, WorldConfig,
};
use glam::{Mat4, Quat, Vec3};

/// The built-in primitives asset (ADR-0023, `fynkel_authoring::PRIMITIVES_ID`).
pub const PRIMITIVES_ID: Uuid =
    Uuid::from_bytes(u128::to_be_bytes(0xf7a1e5d0_0000_4000_8000_000000000023));
/// The stand-in sphere's reference: an id no cache has.
const STAND_IN_SPHERE: Uuid =
    Uuid::from_bytes(u128::to_be_bytes(0x7b0e0000_0000_4000_8000_00000000a5f1));
/// The world the asset is placed in, and the entity carrying a mesh or
/// material on its own.
const STAGE_SCENE: Uuid =
    Uuid::from_bytes(u128::to_be_bytes(0x7b0e0000_0000_4000_8000_000000000001));
const STAGE_ENTITY: Uuid =
    Uuid::from_bytes(u128::to_be_bytes(0x7b0e0000_0000_4000_8000_000000000002));
/// The stage's own sun, when the world is drawn as it is.
const STAGE_SUN: Uuid = Uuid::from_bytes(u128::to_be_bytes(0x7b0e0000_0000_4000_8000_000000000003));

/// Where the camera looks from, relative to the bounds' centre.
pub const VIEW_FROM: Vec3 = Vec3::new(1.0, 0.8, 1.0);
pub const FOV_Y_DEGREES: f32 = 30.0;
/// Room around the bounding sphere (1 = touching the picture's edges).
pub const MARGIN: f32 = 1.1;

/// What a material is shown on (Unity's preview shapes), or any mesh the
/// user drops in.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Sphere,
    Cube,
    Cylinder,
    Capsule,
    Plane,
    Mesh(AssetRef),
}

impl Shape {
    /// `sphere`, `cube`, `cylinder`, `capsule`, `plane`; `None` for
    /// anything else.
    pub fn parse(name: &str) -> Option<Shape> {
        Some(match name {
            "sphere" => Shape::Sphere,
            "cube" => Shape::Cube,
            "cylinder" => Shape::Cylinder,
            "capsule" => Shape::Capsule,
            "plane" => Shape::Plane,
            _ => return None,
        })
    }

    /// The primitives' sub-asset of a built-in shape.
    fn primitive(&self) -> Option<&'static str> {
        Some(match self {
            Shape::Sphere => "mesh/Sphere",
            Shape::Cube => "mesh/Cube",
            Shape::Cylinder => "mesh/Cylinder",
            Shape::Capsule => "mesh/Capsule",
            Shape::Plane => "mesh/Plane",
            Shape::Mesh(_) => return None,
        })
    }
}

/// What one asset is shown as.
#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    /// A prefab document, or a model's (or rig's) node hierarchy.
    Hierarchy(Uuid),
    /// One mesh with a material (nil: the renderer's default).
    Mesh { mesh: AssetRef, material: AssetRef },
    /// A material on a shape.
    Material { material: AssetRef, shape: Shape },
}

impl Subject {
    /// The asset whose closure must be resident while it is shown.
    pub fn owner(&self) -> Uuid {
        match self {
            Subject::Hierarchy(id) => *id,
            Subject::Mesh { mesh, .. } => mesh.asset,
            Subject::Material { material, .. } => material.asset,
        }
    }
}

/// Why an asset has no subject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing to show for this kind (a texture, a sound…): not a failure.
    Nothing(String),
    /// The asset could not be read.
    Failed(String),
}

impl Refusal {
    pub fn message(&self) -> &str {
        match self {
            Refusal::Nothing(m) | Refusal::Failed(m) => m,
        }
    }
}

fn article(word: &str) -> &'static str {
    match word.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

fn nothing_for(kind: &str) -> Refusal {
    Refusal::Nothing(format!("no thumbnail for {} {kind}", article(kind)))
}

/// The editor's word for an index kind (`document/<kind>` or an importer).
pub fn kind_name(kind: &str) -> &str {
    match kind {
        "fynkel.image" => "texture",
        "fynkel.audio" => "sound",
        "fynkel.gltf" | "fynkel.fbx" => "model",
        "fynkel.file" => "file",
        k => k.strip_prefix("document/").unwrap_or(k),
    }
}

/// How `id#sub` is shown, from the cache's index, or why it is not.
pub fn subject(assets: &Assets, id: Uuid, sub: Option<&str>) -> Result<Subject, Refusal> {
    let Some(entry) = assets.entry(id) else {
        return Err(Refusal::Failed(format!(
            "{id} is not in the project's cache"
        )));
    };
    if entry.file.is_none() {
        return Err(Refusal::Failed(format!("{}: its cook failed", entry.path)));
    }
    let kind = kind_name(&entry.kind);
    let hierarchy = matches!(kind, "prefab" | "model" | "rig");
    match sub {
        None | Some("prefab") if hierarchy => Ok(Subject::Hierarchy(id)),
        None if kind == "material" => Ok(Subject::Material {
            material: AssetRef::new(id),
            shape: Shape::Sphere,
        }),
        None => Err(nothing_for(kind)),
        Some(s) if s.starts_with("mesh/") => {
            let mesh = AssetRef::with_sub(id, s);
            let material = default_material(assets, &mesh);
            Ok(Subject::Mesh { mesh, material })
        }
        Some(s) if s.starts_with("material/") => Ok(Subject::Material {
            material: AssetRef::with_sub(id, s),
            shape: Shape::Sphere,
        }),
        Some(s) => Err(nothing_for(match s.split('/').next().unwrap_or(s) {
            "image" => "texture",
            "clip" => "clip",
            other => other,
        })),
    }
}

/// The material a model draws `mesh` with: the one its own hierarchy
/// pairs it with (the model's `prefab`), nil — the renderer's default —
/// when it has none or the mesh is not in it.
pub fn default_material(assets: &Assets, mesh: &AssetRef) -> AssetRef {
    let Ok(prefab) = assets.prefab(mesh.asset) else {
        return AssetRef::default();
    };
    prefab
        .description
        .entities
        .iter()
        .flat_map(|e| &e.components)
        .find_map(|c| match c {
            ComponentValue::MeshRenderer(r) if r.mesh == *mesh => Some(r.material.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// The mesh `shape` is: the project's primitive, a dropped-in mesh as it
/// is, or — when the cache has no primitives — a stand-in UV sphere of
/// the Sphere's size made resident in the store (never written to the
/// cache).
pub fn shape_mesh(assets: &Assets, shape: &Shape) -> Result<AssetRef, Refusal> {
    if let Shape::Mesh(mesh) = shape {
        return Ok(mesh.clone());
    }
    let sub = shape.primitive().unwrap_or("mesh/Sphere");
    let primitive = AssetRef::with_sub(PRIMITIVES_ID, sub);
    if assets.entry(PRIMITIVES_ID).is_some() && assets.mesh(&primitive).is_ok() {
        return Ok(primitive);
    }
    let stand_in = AssetRef::with_sub(STAND_IN_SPHERE, "mesh/Sphere");
    if assets.mesh(&stand_in).is_err() {
        assets
            .insert_mesh(&stand_in, uv_sphere(0.5, 32, 16))
            .map_err(|d| Refusal::Failed(d.message))?;
    }
    Ok(stand_in)
}

/// A UV sphere of `radius` with `segments` around and `rings` from pole
/// to pole, counter-clockwise seen from outside (the cooked winding),
/// with normals, tangents along the longitude and UVs.
pub fn uv_sphere(radius: f32, segments: u32, rings: u32) -> MeshPayload {
    use std::f32::consts::{PI, TAU};
    let mut vertices = Vec::with_capacity(((segments + 1) * (rings + 1)) as usize);
    for i in 0..=rings {
        let theta = PI * i as f32 / rings as f32;
        for j in 0..=segments {
            let phi = TAU * j as f32 / segments as f32;
            let n = [
                theta.sin() * phi.cos(),
                theta.cos(),
                theta.sin() * phi.sin(),
            ];
            vertices.push(Vertex {
                position: n.map(|c| c * radius),
                normal: n,
                tangent: [-phi.sin(), 0.0, phi.cos(), 1.0],
                uv0: [j as f32 / segments as f32, i as f32 / rings as f32],
                ..Vertex::default()
            });
        }
    }
    let mut indices = Vec::with_capacity((segments * rings * 6) as usize);
    let row = segments + 1;
    for i in 0..rings {
        for j in 0..segments {
            let a = i * row + j;
            let (b, c, d) = (a + row, a + row + 1, a + 1);
            indices.extend_from_slice(&[a, d, c, a, c, b]);
        }
    }
    MeshPayload {
        bounds_min: [-radius; 3],
        bounds_max: [radius; 3],
        vertices,
        indices,
        skin: Vec::new(),
    }
}

/// One entity drawing `mesh` with `material` at the origin.
pub fn single(mesh: AssetRef, material: AssetRef) -> SceneDescription {
    SceneDescription {
        id: STAGE_SCENE,
        name: "Preview".to_string(),
        entities: vec![EntityDescription {
            registered: Vec::new(),
            id: STAGE_ENTITY,
            name: "Preview".to_string(),
            active: true,
            parent: None,
            children: Vec::new(),
            components: vec![ComponentValue::MeshRenderer(MeshRenderer {
                mesh,
                material,
                ..MeshRenderer::default()
            })],
        }],
    }
}

/// A subject in its world, stepped once, its assets held resident.
pub struct Staged {
    pub world: World,
    /// Keeps the subject's closure resident while the world is shown.
    pub residency: Residency,
    /// What loading the hierarchy's prefabs and animation reported.
    pub diagnostics: Vec<String>,
}

/// Puts `subject` alone into a world made with `config`, and steps it
/// once (see [`stage_into`]).
pub fn stage(assets: &Assets, subject: &Subject, config: WorldConfig) -> Result<Staged, Refusal> {
    let mut world = World::new(config);
    let stood = stage_into(assets, subject, &mut world, None)?;
    world.begin_frame(1.0 / 60.0);
    world.step();
    Ok(Staged {
        world,
        residency: stood.residency,
        diagnostics: stood.diagnostics,
    })
}

/// What [`stage_into`] put in a world.
pub struct Stood {
    /// Keeps the subject's closure resident while the world is shown.
    pub residency: Residency,
    /// What loading the hierarchy's prefabs and animation reported.
    pub diagnostics: Vec<String>,
    /// The stage's sun, when it has its own.
    pub sun: Option<Entity>,
    /// The subject's top entity (a clip's animator goes here).
    pub root: Option<Entity>,
    /// The joints of the hierarchy's skins, each skin counted once.
    pub bones: usize,
}

/// The description `subject` loads as: a hierarchy's own, or one entity
/// with the mesh (a material on its shape).
pub fn description(assets: &Assets, subject: &Subject) -> Result<SceneDescription, Refusal> {
    Ok(match subject {
        Subject::Hierarchy(id) => match assets.prefab(*id) {
            Ok(p) => p.description,
            Err(diags) => {
                return Err(Refusal::Failed(
                    diags
                        .first()
                        .map(|d| d.message.clone())
                        .unwrap_or_else(|| format!("{id}: not a prefab")),
                ))
            }
        },
        Subject::Mesh { mesh, material } => {
            if let Err(d) = assets.mesh(mesh) {
                return Err(Refusal::Failed(d.message));
            }
            single(mesh.clone(), material.clone())
        }
        Subject::Material { material, shape } => {
            if let Err(d) = assets.material(material) {
                return Err(Refusal::Failed(d.message));
            }
            let mesh = shape_mesh(assets, shape)?;
            if let Err(d) = assets.mesh(&mesh) {
                return Err(Refusal::Failed(d.message));
            }
            single(mesh, material.clone())
        }
    })
}

/// Loads `subject` into `world` (replacing what it held) and holds its
/// assets resident. A hierarchy also gets what its components reference
/// from the cache: the skins a skinned model deforms with, the prefabs
/// its emitters would spawn. With a `sun` (toward the light, as
/// [`Framing::light_dir`]), the asset's cameras, lights, environment,
/// volumes and audio listeners are left out of the world itself
/// ([`strip_description`]) and the stage's own sun is put in, as the
/// renderer's fallback one (10 000 lux, shadows) — for a world drawn as
/// it is, where [`strip`] on an extract is not possible. The world is not
/// stepped.
pub fn stage_into(
    assets: &Assets,
    subject: &Subject,
    world: &mut World,
    sun: Option<Vec3>,
) -> Result<Stood, Refusal> {
    let mut description = description(assets, subject)?;
    if let Some(dir) = sun {
        strip_description(&mut description);
        description.entities.push(EntityDescription {
            registered: Vec::new(),
            id: STAGE_SUN,
            name: "Preview Sun".to_string(),
            active: true,
            parent: None,
            children: Vec::new(),
            components: vec![
                ComponentValue::Transform(sun_transform(dir)),
                ComponentValue::DirectionalLight(DirectionalLight::default()),
            ],
        });
    }
    let mut closure = assets.closure(subject.owner());
    if let Subject::Material {
        shape: Shape::Mesh(mesh),
        ..
    } = subject
    {
        closure.extend(assets.closure(mesh.asset));
    }
    let residency = assets.retain(&closure);
    let (resources, report) =
        fynkel_assets::PreparedGameplayResources::for_scene(assets, world, &description);
    let loaded = world
        .load_scene_prepared(
            &description,
            fynkel_core::LoadMode::Replace,
            Some(Box::new(resources)),
        )
        .map_err(|e| Refusal::Failed(format!("{e:?}")))?;
    let diagnostics = report.diagnostics.iter().map(ToString::to_string).collect();
    // Spawn order is parents first: the first entity that is not the sun
    // is the subject's top.
    let root = loaded
        .entities()
        .iter()
        .find(|(id, _)| *id != STAGE_SUN)
        .map(|(_, e)| *e);
    let mut skins: Vec<&fynkel_contracts::AssetRef> = Vec::new();
    for c in description.entities.iter().flat_map(|e| &e.components) {
        if let fynkel_core::components::ComponentValue::Skin(k) = c {
            if !skins.contains(&&k.skin) {
                skins.push(&k.skin);
            }
        }
    }
    let bones = skins
        .iter()
        .filter_map(|r| assets.skin(r).ok())
        .map(|s| s.joints.len())
        .sum();
    Ok(Stood {
        residency,
        diagnostics,
        sun: sun.and_then(|_| loaded.handle_of(STAGE_SUN)),
        root,
        bones,
    })
}

/// The stage's sun shining along `-toward` (a light shines down its -Z).
pub fn sun_transform(toward: Vec3) -> Transform {
    let along = -toward.try_normalize().unwrap_or(Vec3::Y);
    Transform {
        rotation: Quat::from_rotation_arc(Vec3::NEG_Z, along),
        ..Transform::default()
    }
}

/// Turns the stage's `sun` to shine from `toward` (applied now).
pub fn set_sun(world: &mut World, sun: Entity, toward: Vec3) -> fynkel_core::EditResult<()> {
    world.edit_gameplay(|edit| edit.set_transform(sun, sun_transform(toward)))?;
    Ok(())
}

/// Takes the components that are not the asset's own look out of a
/// description: cameras (and their rigs), lights, the environment,
/// volumes and audio listeners.
pub fn strip_description(description: &mut SceneDescription) {
    for e in &mut description.entities {
        e.components.retain(|c| {
            !matches!(
                c,
                ComponentValue::Camera(_)
                    | ComponentValue::ThirdPersonCamera(_)
                    | ComponentValue::LookAtCamera(_)
                    | ComponentValue::DirectionalLight(_)
                    | ComponentValue::PointLight(_)
                    | ComponentValue::SpotLight(_)
                    | ComponentValue::Environment(_)
                    | ComponentValue::Volume(_)
                    | ComponentValue::AudioListener(_)
            )
        });
    }
}

/// Only the asset's own geometry: its cameras, lights, environment and
/// volumes are taken out; the stage's own take their place.
pub fn strip(extract: &mut Extract) {
    extract.camera = None;
    extract.directional_lights.clear();
    extract.point_lights.clear();
    extract.spot_lights.clear();
    extract.environment = None;
    extract.volumes.clear();
}

/// The drawn meshes' world bounds (min, max), from their cooked bounds;
/// `None` when nothing measurable is drawn.
pub fn world_bounds(assets: &Assets, extract: &Extract) -> Option<(Vec3, Vec3)> {
    let mut bounds: Option<(Vec3, Vec3)> = None;
    for m in &extract.meshes {
        let Ok(payload) = assets.mesh(&m.mesh) else {
            continue;
        };
        let world: Mat4 = extract.records[m.record as usize].world;
        let (lo, hi) = (
            Vec3::from(payload.bounds_min),
            Vec3::from(payload.bounds_max),
        );
        for k in 0..8 {
            let corner = Vec3::new(
                if k & 1 == 0 { lo.x } else { hi.x },
                if k & 2 == 0 { lo.y } else { hi.y },
                if k & 4 == 0 { lo.z } else { hi.z },
            );
            let p = world.transform_point3(corner);
            bounds = Some(match bounds {
                Some((a, b)) => (a.min(p), b.max(p)),
                None => (p, p),
            });
        }
    }
    bounds
}

/// What the drawn meshes add up to (the info line under a 3D preview).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeshStats {
    pub meshes: usize,
    pub vertices: usize,
    pub triangles: usize,
    /// Of them, the ones a skin deforms.
    pub skinned: usize,
}

/// Counts the extract's meshes from their cooked payloads.
pub fn mesh_stats(assets: &Assets, extract: &Extract) -> MeshStats {
    let mut stats = MeshStats::default();
    for m in &extract.meshes {
        let Ok(payload) = assets.mesh(&m.mesh) else {
            continue;
        };
        stats.meshes += 1;
        stats.vertices += payload.vertices.len();
        stats.triangles += payload.indices.len() / 3;
        if !payload.skin.is_empty() {
            stats.skinned += 1;
        }
    }
    stats
}

/// A camera fitting some bounds, and the sun that lights them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov_y_radians: f32,
    pub near: f32,
    pub far: f32,
    /// Toward the light (the renderer's `light_dir`).
    pub light_dir: Vec3,
    /// The bounding sphere's radius (what an orbit keeps its distance by).
    pub radius: f32,
}

impl Framing {
    /// The camera's world matrix (it looks down its -Z).
    pub fn camera_world(&self) -> Mat4 {
        glam::camera::rh::view::look_at_mat4(self.eye, self.target, Vec3::Y).inverse()
    }
}

/// The camera fitting the bounds' sphere from `view_from` (by default
/// [`VIEW_FROM`]), and the sun from the upper left front of that view.
pub fn frame((lo, hi): (Vec3, Vec3), view_from: Vec3) -> Framing {
    let centre = (lo + hi) * 0.5;
    let radius = match (hi - lo).length() * 0.5 {
        r if r > 1e-4 && r.is_finite() => r,
        _ => 0.5,
    };
    let fov = FOV_Y_DEGREES.to_radians();
    let distance = radius * MARGIN / (fov * 0.5).sin();
    let toward = view_from.normalize();
    // The view's left: across the view direction, level with the ground
    // ((-1, 0, 1) for an eye on (1, ·, 1)).
    let left = match Vec3::new(-view_from.z, 0.0, view_from.x).try_normalize() {
        Some(l) => l,
        None => Vec3::new(-1.0, 0.0, 1.0).normalize(),
    };
    Framing {
        eye: centre + toward * distance,
        target: centre,
        fov_y_radians: fov,
        near: (distance - radius * 1.5).max(distance * 0.01),
        far: distance + radius * 1.5,
        light_dir: (toward + left * 0.7 + Vec3::Y * 0.8).normalize(),
        radius,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_fits_the_sphere_from_the_three_quarter_view() {
        let f = frame((Vec3::splat(-0.5), Vec3::splat(0.5)), VIEW_FROM);
        assert_eq!(f.target, Vec3::ZERO);
        let d = (f.eye - f.target).length();
        let r = (3.0f32).sqrt() * 0.5;
        assert!((d - r * MARGIN / (15f32.to_radians()).sin()).abs() < 1e-4);
        assert!((f.eye.normalize() - VIEW_FROM.normalize()).length() < 1e-5);
        // The thumbnail's sun: toward + left·0.7 + up·0.8, left = (-1, 0, 1).
        let toward = VIEW_FROM.normalize();
        let want =
            (toward + Vec3::new(-1.0, 0.0, 1.0).normalize() * 0.7 + Vec3::Y * 0.8).normalize();
        assert!((f.light_dir - want).length() < 1e-5);
        assert!(f.near > 0.0 && f.far > d);
        // Its world matrix looks from the eye to the target.
        let fwd = f.camera_world().transform_vector3(-Vec3::Z);
        assert!((fwd - (f.target - f.eye).normalize()).length() < 1e-5);
    }

    #[test]
    fn empty_bounds_frame_a_unit_sphere() {
        assert_eq!(frame((Vec3::ONE, Vec3::ONE), VIEW_FROM).radius, 0.5);
    }

    #[test]
    fn shapes_by_name() {
        assert_eq!(Shape::parse("cube"), Some(Shape::Cube));
        assert_eq!(Shape::parse("torus"), None);
        assert_eq!(Shape::Capsule.primitive(), Some("mesh/Capsule"));
    }

    #[test]
    fn strip_leaves_only_geometry() {
        let mut e = Extract {
            camera: Some(fynkel_core::ExtractCamera {
                record: 0,
                world: Mat4::IDENTITY,
                camera: Default::default(),
            }),
            ..Extract::default()
        };
        strip(&mut e);
        assert!(e.camera.is_none());
    }

    #[test]
    fn the_sun_shines_away_from_toward() {
        let toward = Vec3::new(0.3, 1.0, 0.2).normalize();
        let t = sun_transform(toward);
        assert!((t.rotation * Vec3::NEG_Z + toward).length() < 1e-5);
    }

    #[test]
    fn sun_edit_publishes_without_a_tick_and_rejects_stale_identity() {
        let mut world = World::new(Default::default());
        let commit = world
            .edit_gameplay(|edit| edit.spawn("sun", Transform::IDENTITY, None))
            .unwrap();
        let sun = commit.resolve(commit.value).unwrap();
        let tick = world.tick();
        let toward = Vec3::new(0.3, 1.0, 0.2);
        set_sun(&mut world, sun, toward).unwrap();
        let expected = sun_transform(toward);
        assert_eq!(world.ecs().copy::<Transform>(sun).unwrap(), expected);
        assert_eq!(world.tick(), tick);
        world.edit_gameplay(|edit| edit.destroy(sun)).unwrap();
        assert!(
            matches!(set_sun(&mut world, sun, Vec3::X), Err(fynkel_core::EditError::StaleEntity(e)) if e == sun)
        );
        assert_eq!(world.tick(), tick);
        assert_eq!(world.step().status, fynkel_contracts::Status::Ok);
        assert_eq!(world.live_entities(), 0);
    }

    #[test]
    fn the_uv_sphere_is_closed_and_sized() {
        let s = uv_sphere(0.5, 8, 4);
        assert_eq!(s.vertices.len(), 9 * 5);
        assert_eq!(s.indices.len(), 8 * 4 * 6);
        assert!(s
            .vertices
            .iter()
            .all(|v| (Vec3::from(v.position).length() - 0.5).abs() < 1e-5));
    }
}
