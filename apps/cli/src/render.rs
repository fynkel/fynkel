//! `fynkel render`: a project's startup scene through the real runtime and
//! renderer to a PNG, headless. The proof that a scene file reaches pixels
//! without a window: `load_project` → `World::load_scene` → one step →
//! extract → `ExtractRenderer` over the cooked cache → readback. Exit 0
//! when the image was written, 1 when the project, the scene or an asset
//! failed, 2 on usage or when no GPU adapter is available.

use std::path::PathBuf;
use std::process::ExitCode;

use fynkel_core::{World, WorldConfig};
use fynkel_document::{diagnostics_json, load_project};
use fynkel_render::{headless_device, render_offscreen, CookedAssets, ExtractRenderer};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel render [--project <dir>] [--out <file.png>] [--size <W>x<H>] [--cache <dir>] [--placeholders] [--json]\n                                               render the project's startup scene headlessly to a PNG (run `fynkel cook` first)"
    );
    ExitCode::from(2)
}

struct Opts {
    project: PathBuf,
    out: PathBuf,
    width: u32,
    height: u32,
    cache: Option<PathBuf>,
    placeholders: bool,
    json: bool,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        project: PathBuf::from("."),
        out: PathBuf::from("render.png"),
        width: 1280,
        height: 720,
        cache: None,
        placeholders: false,
        json: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--placeholders" => o.placeholders = true,
            "--project" => {
                i += 1;
                o.project = PathBuf::from(args.get(i)?);
            }
            "--out" => {
                i += 1;
                o.out = PathBuf::from(args.get(i)?);
            }
            "--cache" => {
                i += 1;
                o.cache = Some(PathBuf::from(args.get(i)?));
            }
            "--size" => {
                i += 1;
                let (w, h) = args.get(i)?.split_once('x')?;
                o.width = w.parse().ok().filter(|w| (1..=16384).contains(w))?;
                o.height = h.parse().ok().filter(|h| (1..=16384).contains(h))?;
            }
            "--root" => i += 1,
            _ => return None,
        }
        i += 1;
    }
    Some(o)
}

pub fn run(args: &[String]) -> ExitCode {
    let Some(opts) = parse(args) else {
        return usage();
    };
    let loaded = match load_project(&opts.project) {
        Ok(l) => l,
        Err(diags) => {
            if opts.json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": false, "diagnostics": diagnostics_json(&diags) })
                );
            } else {
                for d in &diags {
                    eprintln!("{d}");
                }
            }
            return ExitCode::from(1);
        }
    };
    for d in &loaded.warnings {
        eprintln!("{d}");
    }
    let cache = opts.cache.clone().unwrap_or_else(|| loaded.cache.clone());
    let assets = match CookedAssets::open(&cache) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("fynkel render: {e}");
            return ExitCode::from(1);
        }
    };

    let mut world = World::new(WorldConfig::default());
    let scene = match world.load_scene(&loaded.scene) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fynkel render: {e}");
            return ExitCode::from(1);
        }
    };
    world.begin_frame(1.0 / 60.0);
    world.step();
    let extract = world.extract();

    let Some((device, queue)) = headless_device() else {
        eprintln!("fynkel render: no GPU adapter available");
        return ExitCode::from(2);
    };
    let mut renderer = ExtractRenderer::new(
        &device,
        wgpu::TextureFormat::Rgba8Unorm,
        opts.width,
        opts.height,
    );
    renderer.placeholders = opts.placeholders;
    renderer.set_assets(Some(assets));
    let pixels = render_offscreen(&device, &queue, &mut renderer, &extract);
    let stats = renderer.stats();
    let errors = renderer.asset_errors().to_vec();

    let mut rgba = Vec::with_capacity(pixels.len() * 4);
    for p in &pixels {
        rgba.extend_from_slice(p);
    }
    if let Err(e) = image::save_buffer(
        &opts.out,
        &rgba,
        opts.width,
        opts.height,
        image::ColorType::Rgba8,
    ) {
        eprintln!("fynkel render: {}: {e}", opts.out.display());
        return ExitCode::from(1);
    }

    if opts.json {
        println!(
            "{}",
            serde_json::json!({
                "ok": errors.is_empty(),
                "out": opts.out.display().to_string(),
                "size": [opts.width, opts.height],
                "project": loaded.project_name,
                "scene": { "id": scene.id.to_string(), "name": scene.name, "entities": scene.entities().len(), "rejectedComponents": scene.rejected_components },
                "cache": cache.display().to_string(),
                "instances": stats.instances,
                "meshes": stats.meshes,
                "placeholders": stats.placeholders,
                "missing": stats.missing,
                "batches": stats.batches,
                "sceneCamera": stats.scene_camera,
                "sceneLight": stats.scene_light,
                "assetErrors": errors,
                "diagnostics": diagnostics_json(&loaded.warnings),
            })
        );
    } else {
        println!(
            "{}: {}x{} — {} \"{}\": {} entities, {} instances ({} meshes, {} placeholders, {} missing), camera {}, light {}",
            opts.out.display(),
            opts.width,
            opts.height,
            loaded.project_name,
            scene.name,
            scene.entities().len(),
            stats.instances,
            stats.meshes,
            stats.placeholders,
            stats.missing,
            if stats.scene_camera { "scene" } else { "default" },
            if stats.scene_light { "scene" } else { "default" },
        );
        for e in &errors {
            eprintln!("asset error: {e}");
        }
    }
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
