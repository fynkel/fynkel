//! `fynkel thumbnail`: the editor's Project-grid tiles, drawn the way
//! Unity's Project browser draws them — a prefab, a model or one of its
//! meshes at a three-quarter angle, a material on a sphere — staged by
//! `fynkel_preview` (shared with the editor's Inspector previews,
//! ADR-0030), so a tile and a preview agree, and drawn through the
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
//! and contact shadows off (no volume gives them an intensity anyway, and
//! a glow would bleed into the transparent background). Pixels left at exactly the clear
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

use fynkel_contracts::Uuid;
use fynkel_core::WorldConfig;
use fynkel_preview::{frame, stage, strip, subject, world_bounds, Refusal, Subject, VIEW_FROM};
use fynkel_render::{
    clear_rgba8, headless_device, render_offscreen, Assets, ExtractRenderer, Quality,
};

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

/// One tile's outcome.
enum Outcome {
    File(PathBuf),
    /// Nothing to draw for this kind: reported, not a failure.
    None(String),
    /// The asset could not be read or drawn.
    Failed(String),
}

impl From<Refusal> for Outcome {
    fn from(r: Refusal) -> Outcome {
        match r {
            Refusal::Nothing(why) => Outcome::None(why),
            Refusal::Failed(why) => Outcome::Failed(why),
        }
    }
}

/// Draws one subject into `path`.
fn draw(
    assets: &Assets,
    gpu: (&wgpu::Device, &wgpu::Queue),
    renderer: &mut ExtractRenderer,
    subject: Subject,
    path: &Path,
) -> Outcome {
    let staged = match stage(assets, &subject, WorldConfig::default()) {
        Ok(s) => s,
        Err(r) => return r.into(),
    };
    for d in &staged.diagnostics {
        eprintln!("{d}");
    }
    let mut extract = staged.world.extract();
    // Only the asset's own geometry: the tile's camera and light are the
    // renderer's fallbacks, set from the framing.
    strip(&mut extract);
    let Some(bounds) = world_bounds(assets, &extract) else {
        return Outcome::None("nothing to draw".to_string());
    };
    let f = frame(bounds, VIEW_FROM);
    renderer.camera = fynkel_render::Camera {
        eye: f.eye,
        target: f.target,
        up: glam::Vec3::Y,
        fov_y_radians: f.fov_y_radians,
        near: f.near,
        far: f.far,
    };
    renderer.light_dir = f.light_dir;
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
        contact_shadows: false,
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
            Err(r) => r.into(),
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
