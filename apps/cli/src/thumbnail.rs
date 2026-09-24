//! `fynkel thumbnail`: the editor's Project-grid tiles, drawn the way
//! Unity's Project browser draws them — a prefab, a model or one of its
//! meshes at a three-quarter angle, a material on a sphere — through the
//! same path as `fynkel render` (`cook::prepare` → `Assets` → a world →
//! one step → extract → `ExtractRenderer` → readback), so a tile is what
//! the scene would show and never a second renderer.
//!
//! What is not the asset's own is taken out of the picture: the extract
//! loses any camera, light, environment and volume a prefab brings, the
//! renderer frames the drawn meshes' world bounds from (1, 0.8, 1) with a
//! 30° lens and lights them with its fallback sun from the upper left
//! front over the default ambient — every tile lit alike, whatever the
//! asset authored. The project's default quality profile applies with
//! MSAA forced on (a tile is small; its edges are most of it) and bloom
//! off (no volume gives it an intensity anyway, and a glow would bleed
//! into the transparent background). Pixels left at exactly the clear
//! colour become transparent, so the tile's own background shows.
//!
//! A material is drawn on the project's built-in Sphere (ADR-0023) when
//! the project has the primitives asset; otherwise on a stand-in sphere
//! made here and handed to the store (`Assets::insert_mesh`), never
//! written to the cache.
//!
//! Exit 0 when every asset got a file or was reported as having no
//! thumbnail (textures, scenes, sounds…), 1 when the project or cook
//! failed or an asset could not be read, 2 on usage or when no GPU
//! adapter is available.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_contracts::{AssetRef, Uuid};
use fynkel_cooked::payload::{MeshPayload, Vertex};
use fynkel_core::{
    ComponentValue, EntityDescription, MeshRenderer, SceneDescription, World, WorldConfig,
};
use fynkel_render::{
    clear_rgba8, headless_device, render_offscreen, Assets, ExtractRenderer, Quality,
};
use glam::{Mat4, Vec3};

/// The built-in primitives asset (ADR-0023, `fynkel_authoring::PRIMITIVES_ID`).
const PRIMITIVES_ID: Uuid =
    Uuid::from_bytes(u128::to_be_bytes(0xf7a1e5d0_0000_4000_8000_000000000023));
/// Its sphere, 1 m across.
const PRIMITIVES_SPHERE: &str = "mesh/Sphere";
/// The stand-in sphere's reference: an id no cache has.
const STAND_IN_SPHERE: Uuid =
    Uuid::from_bytes(u128::to_be_bytes(0x7b0e0000_0000_4000_8000_00000000a5f1));
/// The world the asset is placed in, and the entity carrying a mesh or
/// material on its own.
const THUMBNAIL_SCENE: Uuid =
    Uuid::from_bytes(u128::to_be_bytes(0x7b0e0000_0000_4000_8000_000000000001));
const THUMBNAIL_ENTITY: Uuid =
    Uuid::from_bytes(u128::to_be_bytes(0x7b0e0000_0000_4000_8000_000000000002));

/// Where the camera looks from, relative to the bounds' centre.
const VIEW_FROM: Vec3 = Vec3::new(1.0, 0.8, 1.0);
const FOV_Y_DEGREES: f32 = 30.0;
/// Room around the bounding sphere (1 = touching the tile's edges).
const MARGIN: f32 = 1.1;

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel thumbnail [--project <dir>] [--cache <dir>] [--no-cook] [--size <px>] --out-dir <dir> [--json] <asset>...\n                                               cook the project, then draw each <assetId> or <assetId>#<sub> (a prefab, a model or its mesh/<name>,\n                                               a material or a model's material/<name> on a sphere) to <out-dir>/<assetId>[__<sub, '/' as '_'>].png,\n                                               <px> square (16..=1024, default 128), transparent where nothing was drawn"
    );
    ExitCode::from(2)
}

struct Opts {
    target: crate::cook::Target,
    out_dir: PathBuf,
    size: u32,
    json: bool,
    assets: Vec<(Uuid, Option<String>)>,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        target: crate::cook::Target::default(),
        out_dir: PathBuf::new(),
        size: 128,
        json: false,
        assets: Vec::new(),
    };
    let mut out_dir = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--no-cook" => o.target.no_cook = true,
            "--project" => {
                i += 1;
                o.target.project = Some(PathBuf::from(args.get(i)?));
            }
            "--cache" => {
                i += 1;
                o.target.cache = Some(PathBuf::from(args.get(i)?));
            }
            "--out-dir" => {
                i += 1;
                out_dir = Some(PathBuf::from(args.get(i)?));
            }
            "--size" => {
                i += 1;
                o.size = args
                    .get(i)?
                    .parse()
                    .ok()
                    .filter(|s| (16..=1024).contains(s))?;
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel thumbnail: unknown argument {other}");
                return None;
            }
            asset => {
                let (id, sub) = match asset.split_once('#') {
                    Some((id, sub)) if !sub.is_empty() => (id, Some(sub.to_string())),
                    Some(_) => return None,
                    None => (asset, None),
                };
                let Ok(id) = id.parse() else {
                    eprintln!(
                        "fynkel thumbnail: {asset}: not an asset id (<uuid> or <uuid>#<sub>)"
                    );
                    return None;
                };
                o.assets.push((id, sub));
            }
        }
        i += 1;
    }
    o.out_dir = out_dir?;
    if o.assets.is_empty() {
        return None;
    }
    Some(o)
}

/// `<assetId>.png`, or `<assetId>__<sub with '/' as '_'>.png`.
pub fn file_name(id: Uuid, sub: Option<&str>) -> String {
    match sub {
        Some(sub) => format!("{id}__{}.png", sub.replace('/', "_")),
        None => format!("{id}.png"),
    }
}

/// What one requested asset is drawn as.
enum Subject {
    /// A prefab document, or a model's (or rig's) node hierarchy.
    Hierarchy(Uuid),
    /// One mesh with a material (nil: the renderer's default).
    Mesh(AssetRef, AssetRef),
    /// A material on the sphere.
    Material(AssetRef),
}

/// One tile's outcome.
enum Outcome {
    File(PathBuf),
    /// Nothing to draw for this kind: reported, not a failure.
    None(String),
    /// The asset could not be read or drawn.
    Failed(String),
}

fn article(word: &str) -> &'static str {
    match word.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

fn no_thumbnail(kind: &str) -> Outcome {
    Outcome::None(format!("no thumbnail for {} {kind}", article(kind)))
}

/// The editor's word for an index kind (`document/<kind>` or an importer).
fn kind_name(kind: &str) -> &str {
    match kind {
        "fynkel.image" => "texture",
        "fynkel.audio" => "sound",
        "fynkel.gltf" | "fynkel.fbx" => "model",
        "fynkel.file" => "file",
        k => k.strip_prefix("document/").unwrap_or(k),
    }
}

/// Decides how `id#sub` is drawn from the index, or why it is not.
fn subject(assets: &Assets, id: Uuid, sub: Option<&str>) -> Result<Subject, Outcome> {
    let Some(entry) = assets.entry(id) else {
        return Err(Outcome::Failed(format!(
            "{id} is not in the project's cache"
        )));
    };
    if entry.file.is_none() {
        return Err(Outcome::Failed(format!("{}: its cook failed", entry.path)));
    }
    let kind = kind_name(&entry.kind);
    let hierarchy = matches!(kind, "prefab" | "model" | "rig");
    match sub {
        None | Some("prefab") if hierarchy => Ok(Subject::Hierarchy(id)),
        None if kind == "material" => Ok(Subject::Material(AssetRef::new(id))),
        None => Err(no_thumbnail(kind)),
        Some(s) if s.starts_with("mesh/") => {
            let mesh = AssetRef::with_sub(id, s);
            Ok(Subject::Mesh(mesh.clone(), default_material(assets, &mesh)))
        }
        Some(s) if s.starts_with("material/") => Ok(Subject::Material(AssetRef::with_sub(id, s))),
        Some(s) => Err(no_thumbnail(match s.split('/').next().unwrap_or(s) {
            "image" => "texture",
            "clip" => "clip",
            other => other,
        })),
    }
}

/// The material a model draws `mesh` with: the one its own hierarchy
/// pairs it with (the model's `prefab`), nil — the renderer's default —
/// when it has none or the mesh is not in it.
fn default_material(assets: &Assets, mesh: &AssetRef) -> AssetRef {
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

/// The sphere a material is drawn on: the project's primitives Sphere,
/// or a stand-in UV sphere of the same size made resident in the store.
fn sphere(assets: &Assets) -> AssetRef {
    let primitive = AssetRef::with_sub(PRIMITIVES_ID, PRIMITIVES_SPHERE);
    if assets.entry(PRIMITIVES_ID).is_some() && assets.mesh(&primitive).is_ok() {
        return primitive;
    }
    let stand_in = AssetRef::with_sub(STAND_IN_SPHERE, PRIMITIVES_SPHERE);
    if assets.mesh(&stand_in).is_err() {
        assets.insert_mesh(&stand_in, uv_sphere(0.5, 32, 16));
    }
    stand_in
}

/// A UV sphere of `radius` with `segments` around and `rings` from pole
/// to pole, counter-clockwise seen from outside (the cooked winding),
/// with normals, tangents along the longitude and UVs.
fn uv_sphere(radius: f32, segments: u32, rings: u32) -> MeshPayload {
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
fn single(mesh: AssetRef, material: AssetRef) -> SceneDescription {
    SceneDescription {
        id: THUMBNAIL_SCENE,
        name: "Thumbnail".to_string(),
        entities: vec![EntityDescription {
            id: THUMBNAIL_ENTITY,
            name: "Thumbnail".to_string(),
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

/// The drawn meshes' world bounds (min, max), from their cooked bounds;
/// `None` when nothing measurable is drawn.
fn world_bounds(assets: &Assets, extract: &fynkel_core::Extract) -> Option<(Vec3, Vec3)> {
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

/// The camera fitting the bounds' sphere from [`VIEW_FROM`], and the
/// fallback sun from the upper left front of that view.
fn frame(renderer: &mut ExtractRenderer, (lo, hi): (Vec3, Vec3)) {
    let centre = (lo + hi) * 0.5;
    let radius = match (hi - lo).length() * 0.5 {
        r if r > 1e-4 && r.is_finite() => r,
        _ => 0.5,
    };
    let fov = FOV_Y_DEGREES.to_radians();
    let distance = radius * MARGIN / (fov * 0.5).sin();
    let toward = VIEW_FROM.normalize();
    renderer.camera = fynkel_render::Camera {
        eye: centre + toward * distance,
        target: centre,
        up: Vec3::Y,
        fov_y_radians: fov,
        near: (distance - radius * 1.5).max(distance * 0.01),
        far: distance + radius * 1.5,
    };
    // The view's left is (-1, 0, 1) for an eye on (1, ·, 1).
    let left = Vec3::new(-1.0, 0.0, 1.0).normalize();
    renderer.light_dir = (toward + left * 0.7 + Vec3::Y * 0.8).normalize();
}

/// Draws one subject into `path`.
fn draw(
    assets: &Assets,
    gpu: (&wgpu::Device, &wgpu::Queue),
    renderer: &mut ExtractRenderer,
    subject: Subject,
    path: &Path,
) -> Outcome {
    let (description, owner, hierarchy) = match subject {
        Subject::Hierarchy(id) => match assets.prefab(id) {
            Ok(p) => (p.description, id, true),
            Err(diags) => {
                return Outcome::Failed(
                    diags
                        .first()
                        .map(|d| d.message.clone())
                        .unwrap_or_else(|| format!("{id}: not a prefab")),
                )
            }
        },
        Subject::Mesh(mesh, material) => {
            if let Err(d) = assets.mesh(&mesh) {
                return Outcome::Failed(d.message);
            }
            let id = mesh.asset;
            (single(mesh, material), id, false)
        }
        Subject::Material(material) => {
            if let Err(d) = assets.material(&material) {
                return Outcome::Failed(d.message);
            }
            let id = material.asset;
            (single(sphere(assets), material), id, false)
        }
    };
    let _residency = assets.retain(&assets.closure(owner));
    let mut world = World::new(WorldConfig::default());
    if let Err(e) = world.load_scene(&description) {
        return Outcome::Failed(format!("{e:?}"));
    }
    if hierarchy {
        // What the hierarchy's components reference from the cache: the
        // skins a skinned model deforms with (drawn at the first step's
        // pose), the prefabs its emitters would spawn.
        let (_, prefab_diags) = assets.register_prefabs(&description, &mut world);
        let (_, _, anim_diags) = assets.register_animation(&description, &mut world);
        for d in prefab_diags.iter().chain(&anim_diags) {
            eprintln!("{d}");
        }
    }
    world.begin_frame(1.0 / 60.0);
    world.step();
    let mut extract = world.extract();
    // Only the asset's own geometry: the tile's camera and light are the
    // renderer's fallbacks, set by `frame`.
    extract.camera = None;
    extract.directional_lights.clear();
    extract.point_lights.clear();
    extract.spot_lights.clear();
    extract.environment = None;
    extract.volumes.clear();
    let Some(bounds) = world_bounds(assets, &extract) else {
        return Outcome::None("nothing to draw".to_string());
    };
    frame(renderer, bounds);
    let seen = renderer.asset_errors().len();
    let pixels = render_offscreen(gpu.0, gpu.1, renderer, &extract);
    for e in renderer.asset_errors().iter().skip(seen) {
        eprintln!("fynkel thumbnail: asset error: {e}");
    }
    let clear = clear_rgba8();
    let pixels: Vec<[u8; 4]> = pixels
        .into_iter()
        .map(|p| if p == clear { [0; 4] } else { p })
        .collect();
    let (w, h) = renderer.size();
    match crate::render::save_png(path, &pixels, w, h) {
        Ok(()) => Outcome::File(path.to_path_buf()),
        Err(e) => Outcome::Failed(format!("{}: {e}", path.display())),
    }
}

pub fn run(args: &[String]) -> ExitCode {
    let Some(opts) = parse(args) else {
        return usage();
    };
    let prepared = match crate::cook::prepare("fynkel thumbnail", &opts.target) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let assets = match Assets::open(&prepared.cache) {
        Ok(a) => a,
        Err(d) => {
            eprintln!("{d}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&opts.out_dir) {
        eprintln!("fynkel thumbnail: {}: {e}", opts.out_dir.display());
        return ExitCode::from(1);
    }
    let out_dir = opts
        .out_dir
        .canonicalize()
        .unwrap_or_else(|_| opts.out_dir.clone());

    let Some((device, queue)) = headless_device() else {
        eprintln!("fynkel thumbnail: no GPU adapter available");
        return ExitCode::from(2);
    };
    let mut renderer = ExtractRenderer::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        opts.size,
        opts.size,
    );
    renderer.placeholders = false;
    let base = assets
        .project()
        .and_then(|p| p.quality)
        .and_then(|q| q.default_profile().map(Quality::from))
        .unwrap_or_default();
    renderer.set_quality(Quality {
        msaa: 4,
        bloom: false,
        ..base
    });
    renderer.set_assets(Some(assets.clone()));

    let mut failed = false;
    let mut reports = Vec::new();
    for (id, sub) in &opts.assets {
        let outcome = match subject(&assets, *id, sub.as_deref()) {
            Ok(s) => {
                let path = out_dir.join(file_name(*id, sub.as_deref()));
                draw(&assets, (&device, &queue), &mut renderer, s, &path)
            }
            Err(o) => o,
        };
        let shown = match sub {
            Some(s) => format!("{id}#{s}"),
            None => id.to_string(),
        };
        let mut report = serde_json::json!({ "asset": id.to_string(), "sub": sub });
        match &outcome {
            Outcome::File(path) => {
                report["file"] = path.display().to_string().into();
                if !opts.json {
                    println!("{shown}: {}", path.display());
                }
            }
            Outcome::None(why) | Outcome::Failed(why) => {
                report["error"] = why.clone().into();
                if !opts.json {
                    println!("{shown}: {why}");
                }
            }
        }
        failed |= matches!(outcome, Outcome::Failed(_));
        reports.push(report);
    }
    if opts.json {
        println!("{}", serde_json::json!({ "thumbnails": reports }));
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
