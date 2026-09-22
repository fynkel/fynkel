//! `fynkel render`: a project's startup scene through the real runtime and
//! renderer to a PNG, headless. The proof that a scene reaches pixels
//! without a window — and from the cache alone: `cook::prepare` (cook the
//! project) → `Assets::scene` → `World::load_scene` (prefabs, clips and
//! skins registered from the cache) → one step, or `--steps` of them, so
//! an animated scene is drawn posed → extract → `ExtractRenderer` over the
//! same asset store → readback. Exit 0 when the
//! image was written, 1 when the project, the scene or an asset failed, 2
//! on usage or when no GPU adapter is available.

use std::path::PathBuf;
use std::process::ExitCode;

use fynkel_core::{World, WorldConfig};
use fynkel_document::diagnostics_json;
use fynkel_render::{headless_device, render_offscreen, Assets, ExtractRenderer};

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel render [--project <dir>] [--scene <path|id>] [--out <file.png>] [--size <W>x<H>] [--quality <profile>] [--cache <dir>] [--no-cook] [--placeholders] [--steps <n>] [--json]\n                                               cook the project, then render its startup scene (or --scene) headlessly from the cache to a PNG after n ticks (default 1; --quality: one of the project's profiles; its default otherwise)"
    );
    ExitCode::from(2)
}

struct Opts {
    target: crate::cook::Target,
    out: PathBuf,
    width: u32,
    height: u32,
    placeholders: bool,
    quality: Option<String>,
    steps: u32,
    json: bool,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        target: crate::cook::Target::default(),
        out: PathBuf::from("render.png"),
        width: 1280,
        height: 720,
        placeholders: false,
        quality: None,
        steps: 1,
        json: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--placeholders" => o.placeholders = true,
            "--no-cook" => o.target.no_cook = true,
            "--project" => {
                i += 1;
                o.target.project = Some(PathBuf::from(args.get(i)?));
            }
            "--scene" => {
                i += 1;
                o.target.scene = Some(args.get(i)?.clone());
            }
            "--out" => {
                i += 1;
                o.out = PathBuf::from(args.get(i)?);
            }
            "--steps" => {
                i += 1;
                o.steps = args.get(i)?.parse().ok().filter(|n| *n >= 1)?;
            }
            "--cache" => {
                i += 1;
                o.target.cache = Some(PathBuf::from(args.get(i)?));
            }
            "--quality" => {
                i += 1;
                o.quality = Some(args.get(i)?.clone());
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
    let prepared = match crate::cook::prepare("fynkel render", &opts.target) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let fail = |diags: Vec<fynkel_document::Diagnostic>| {
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
        ExitCode::from(1)
    };
    let cache = prepared.cache.clone();
    let assets = match Assets::open(&cache) {
        Ok(a) => a,
        Err(d) => return fail(vec![d]),
    };
    let scene_id = match prepared.scene {
        Some(id) => id,
        None => match assets.startup_scene() {
            Ok(id) => id,
            Err(d) => return fail(vec![d]),
        },
    };
    let cooked = match assets.scene(scene_id) {
        Ok(s) => s,
        Err(diags) => return fail(diags),
    };
    for d in &cooked.diagnostics {
        eprintln!("{d}");
    }
    let project_name = assets
        .project()
        .map(|p| p.name)
        .unwrap_or_else(|| "(no project)".to_string());

    let mut world = World::new(WorldConfig::default());
    let scene = match world.load_scene(&cooked.description) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fynkel render: {e}");
            return ExitCode::from(1);
        }
    };
    let _residency = assets.retain(&cooked.closure);
    // What the scene's components reference from the cache: prefabs
    // (Emitter), clips and skins (Animator, Skin — M3-19).
    let (_, prefab_diags) = assets.register_prefabs(&cooked.description, &mut world);
    let (_, _, anim_diags) = assets.register_animation(&cooked.description, &mut world);
    for d in prefab_diags.iter().chain(&anim_diags) {
        eprintln!("{d}");
    }
    for _ in 0..opts.steps {
        world.begin_frame(1.0 / 60.0);
        world.step();
    }
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
    if !renderer.set_quality_profile(opts.quality.as_deref()) {
        eprintln!(
            "fynkel render: --quality {}: not a profile of {} (profiles: {})",
            opts.quality.unwrap_or_default(),
            project_name,
            renderer.quality_profiles().join(", ")
        );
        return ExitCode::from(2);
    }
    let pixels = render_offscreen(&device, &queue, &mut renderer, &extract);
    let stats = renderer.stats();
    let quality = renderer.quality();
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
                "project": project_name,
                "scene": { "id": scene.id.to_string(), "name": scene.name, "key": cooked.key, "entities": scene.entities().len(), "rejectedComponents": scene.rejected_components },
                "cache": cache.display().to_string(),
                "steps": opts.steps,
                "instances": stats.instances,
                "culled": stats.culled,
                "draws": stats.draws,
                "skinned": stats.skinned,
                "joints": stats.joints,
                "shadowDraws": stats.shadow_draws,
                "meshes": stats.meshes,
                "placeholders": stats.placeholders,
                "missing": stats.missing,
                "batches": stats.batches,
                "sceneCamera": stats.scene_camera,
                "sceneLight": stats.scene_light,
                "lights": stats.lights,
                "lightsDropped": stats.lights_dropped,
                "lightsMaxPerObject": stats.lights_max_per_object,
                "lightsOverflow": stats.lights_overflow,
                "sky": stats.sky,
                "ibl": stats.ibl,
                "ev100": renderer.ev100(),
                "transparent": stats.transparent,
                "shadowCasters": stats.shadow_casters,
                "shadowMap": stats.shadow_map,
                "shadowCascades": stats.shadow_cascades,
                "ambientOcclusion": stats.ambient_occlusion,
                "bloom": stats.bloom,
                "volumes": stats.volumes,
                "post": {
                    "aoRadius": renderer.post_settings().ao_radius,
                    "aoIntensity": renderer.post_settings().ao_intensity,
                    "bloomIntensity": renderer.post_settings().bloom_intensity,
                    "bloomThreshold": renderer.post_settings().bloom_threshold,
                    "bloomScatter": renderer.post_settings().bloom_scatter,
                    "bloomTint": renderer.post_settings().bloom_tint.to_array(),
                },
                "quality": {
                    "profile": quality.profile,
                    "shadows": quality.settings.shadows,
                    "shadowResolution": quality.settings.shadow_resolution,
                    "ambientOcclusion": quality.settings.ambient_occlusion,
                    "bloom": quality.settings.bloom,
                    "msaa": quality.settings.msaa,
                    "renderScale": quality.settings.render_scale,
                    "anisotropy": quality.settings.anisotropy,
                    "renderSize": [quality.internal_size.0, quality.internal_size.1],
                },
                "assetErrors": errors,
                "diagnostics": diagnostics_json(&cooked.diagnostics),
            })
        );
    } else {
        println!(
            "{}: {}x{} — {} \"{}\": {} entities, {} instances in {} draw(s) ({} meshes, {} placeholders, {} missing, {} transparent, {} skinned over {} joints, {} culled), camera {}, sun {}, {} light(s){}, EV100 {:.2}{}, quality {} (msaa {}, {}, {}, {}x{})",
            opts.out.display(),
            opts.width,
            opts.height,
            project_name,
            scene.name,
            scene.entities().len(),
            stats.instances,
            stats.draws,
            stats.meshes,
            stats.placeholders,
            stats.missing,
            stats.transparent,
            stats.skinned,
            stats.joints,
            stats.culled,
            if stats.scene_camera { "scene" } else { "default" },
            if stats.scene_light { "scene" } else { "default" },
            stats.lights,
            match (stats.lights_dropped, stats.lights_overflow) {
                (0, 0) => String::new(),
                (0, o) => format!(" (≤ {} each, {o} object(s) clipped)", stats.lights_max_per_object),
                (d, 0) => format!(" ({d} dropped)"),
                (d, o) => format!(" ({d} dropped, {o} object(s) clipped)"),
            },
            renderer.ev100(),
            match (stats.sky, stats.ibl) {
                (true, true) => ", sky + ibl",
                (true, false) => ", sky",
                _ => "",
            },
            quality.profile,
            quality.settings.msaa,
            if stats.shadow_map > 0 {
                format!(
                    "{} caster(s) in {} draw(s) into {} cascades of a {}² shadow map",
                    stats.shadow_casters,
                    stats.shadow_draws,
                    stats.shadow_cascades,
                    stats.shadow_map
                )
            } else {
                "no shadow".to_string()
            },
            {
                let post = renderer.post_settings();
                let mut look = if stats.ambient_occlusion {
                    format!("ambient occlusion {:.2} m ^{:.2}", post.ao_radius, post.ao_intensity)
                } else {
                    "no ambient occlusion".to_string()
                };
                if stats.bloom {
                    look += &format!(", bloom {:.2} >{:.2}", post.bloom_intensity, post.bloom_threshold);
                }
                if stats.volumes > 0 {
                    look += &format!(", {} volume(s)", stats.volumes);
                }
                look
            },
            quality.internal_size.0,
            quality.internal_size.1,
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
