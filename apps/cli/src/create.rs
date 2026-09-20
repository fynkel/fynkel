//! `fynkel create <dir>` (blueprint §13.2/§13.3, ADR-0017, §19.2c item 17):
//! a new game project — a normal Flutter app on `package:fynkel` with
//! `fynkel.project.json`, its lock, an `assets/` tree holding a starter
//! scene (camera, sun, a spinning cube), material, model and texture with
//! sidecars, one behaviour (`game.Spinner`) with its generated registration
//! and schema, a headless test, and `AGENTS.md`. Every file comes from
//! `templates/create/` (embedded at build time); `{{placeholders}}` are the
//! package name, title, SDK paths and fresh ids. Sources are imported once
//! so their sidecars carry sub-asset ids (`mesh/0`), exactly as `fynkel
//! import` would leave them.
//!
//! With `flutter` on `PATH` the platform runners come from `flutter create`
//! first (macOS + iOS, the app sandbox switched off so a dev build can read
//! the project directory); `--no-flutter` skips that. Fresh output passes
//! `fynkel validate`, `fynkel test --headless` and `flutter test`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fynkel_document::asset::{sidecar_path, Meta};
use fynkel_import::Limits;
use uuid::Uuid;

pub fn usage() -> ExitCode {
    eprintln!(
        "usage:\n  fynkel create <dir> [--name <package>] [--title <text>] [--org <reverse.domain>] [--no-flutter] [--force] [--json]\n                                               create a new game project (Flutter app + fynkel.project.json + assets + AGENTS.md)\n                                               package name: the directory's name, lower_snake_case; --root <repo> when not run from the Fynkel checkout"
    );
    ExitCode::from(2)
}

/// A template file: `.tmpl` sources are substituted, the rest copied.
struct TemplateFile {
    /// Path inside the new project.
    path: &'static str,
    body: Body,
}

enum Body {
    Text(&'static str),
    Binary(&'static [u8]),
}

macro_rules! text {
    ($path:literal, $src:literal) => {
        TemplateFile {
            path: $path,
            body: Body::Text(include_str!(concat!("../templates/create/", $src))),
        }
    };
}
macro_rules! bin {
    ($path:literal, $src:literal) => {
        TemplateFile {
            path: $path,
            body: Body::Binary(include_bytes!(concat!("../templates/create/", $src))),
        }
    };
}

const FILES: &[TemplateFile] = &[
    text!("pubspec.yaml", "pubspec.yaml.tmpl"),
    text!("analysis_options.yaml", "analysis_options.yaml"),
    text!(".gitignore", "gitignore"),
    text!("README.md", "README.md.tmpl"),
    TemplateFile {
        path: "fynkel_cache/README.md",
        body: Body::Text(crate::bundle::KEEP_TEXT),
    },
    text!("AGENTS.md", "AGENTS.md.tmpl"),
    text!("fynkel.project.json", "fynkel.project.json.tmpl"),
    text!("lib/main.dart", "lib/main.dart.tmpl"),
    text!("lib/game/game.dart", "lib/game/game.dart.tmpl"),
    text!("lib/game/spinner.dart", "lib/game/spinner.dart"),
    text!("lib/game/mover.dart", "lib/game/mover.dart"),
    text!(
        "lib/generated/behaviours.g.dart",
        "lib/generated/behaviours.g.dart.tmpl"
    ),
    text!(
        "lib/generated/behaviours.g.json",
        "lib/generated/behaviours.g.json.tmpl"
    ),
    text!(
        "schemas/behaviours/game.Spinner.schema.json",
        "schemas/behaviours/game.Spinner.schema.json.tmpl"
    ),
    text!(
        "schemas/behaviours/game.Mover.schema.json",
        "schemas/behaviours/game.Mover.schema.json.tmpl"
    ),
    text!(
        "assets/scenes/main.scene.json",
        "assets/scenes/main.scene.json.tmpl"
    ),
    text!(
        "assets/materials/starter.material.json",
        "assets/materials/starter.material.json.tmpl"
    ),
    bin!("assets/models/cube.glb", "assets/models/cube.glb"),
    bin!("assets/textures/swatch.png", "assets/textures/swatch.png"),
    text!("assets/prefabs/.gitkeep", "assets/prefabs/.gitkeep"),
    text!("assets/audio/.gitkeep", "assets/audio/.gitkeep"),
    text!("test/game_test.dart", "test/game_test.dart.tmpl"),
    text!("test/walk.input.json", "test/walk.input.json"),
];

/// Source files that get an imported sidecar, in `FILES` order.
const SOURCES: &[(&str, &str)] = &[
    ("assets/models/cube.glb", "cube_id"),
    ("assets/textures/swatch.png", "swatch_id"),
];

struct Opts {
    dir: PathBuf,
    name: Option<String>,
    title: Option<String>,
    org: String,
    flutter: bool,
    force: bool,
    json: bool,
}

fn parse(args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        dir: PathBuf::new(),
        name: None,
        title: None,
        org: "com.example".to_string(),
        flutter: true,
        force: false,
        json: false,
    };
    let mut dir = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--force" => o.force = true,
            "--no-flutter" => o.flutter = false,
            "--name" => {
                i += 1;
                o.name = Some(args.get(i)?.clone());
            }
            "--title" => {
                i += 1;
                o.title = Some(args.get(i)?.clone());
            }
            "--org" => {
                i += 1;
                o.org = args.get(i)?.clone();
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel create: unknown argument {other}");
                return None;
            }
            p => {
                if dir.replace(PathBuf::from(p)).is_some() {
                    return None;
                }
            }
        }
        i += 1;
    }
    o.dir = dir?;
    Some(o)
}

/// What a run produced (`--json`).
pub struct Created {
    pub dir: PathBuf,
    pub package: String,
    pub title: String,
    pub files: Vec<String>,
    pub flutter: bool,
}

pub fn run(root: Option<&Path>, args: &[String]) -> ExitCode {
    let Some(opts) = parse(args) else {
        return usage();
    };
    let Some(root) = root else {
        eprintln!(
            "fynkel create: the Fynkel checkout was not found (needed for the package:fynkel path dependency); pass --root <repo>"
        );
        return ExitCode::from(2);
    };
    match create(root, &opts) {
        Ok(c) => {
            if opts.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": true,
                        "dir": c.dir.display().to_string(),
                        "package": c.package,
                        "title": c.title,
                        "flutter": c.flutter,
                        "files": c.files,
                    }))
                    .unwrap()
                );
            } else {
                println!(
                    "fynkel create: {} ({}) — {} files in {}{}",
                    c.title,
                    c.package,
                    c.files.len(),
                    c.dir.display(),
                    if c.flutter {
                        ""
                    } else {
                        "; no platform runners (run `flutter create --platforms macos,ios .` inside it)"
                    }
                );
                println!(
                    "next:\n  cd {}\n  fynkel validate\n  fynkel test --headless\n  flutter test\n  fynkel run",
                    opts.dir.display()
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            if opts.json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": false, "diagnostics": [{ "code": "FYN-P0001", "severity": "error", "message": e }] })
                );
            } else {
                eprintln!("fynkel create: {e}");
            }
            ExitCode::from(1)
        }
    }
}

fn create(root: &Path, opts: &Opts) -> Result<Created, String> {
    let dir = &opts.dir;
    let package = match &opts.name {
        Some(n) => n.clone(),
        None => dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
            .ok_or_else(|| format!("{} has no directory name; pass --name", dir.display()))?,
    };
    check_package_name(&package)?;
    let title = opts.title.clone().unwrap_or_else(|| title_of(&package));

    if dir.exists() {
        if !dir.is_dir() {
            return Err(format!("{} exists and is not a directory", dir.display()));
        }
        let empty = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .next()
            .is_none();
        if !empty && !opts.force {
            return Err(format!(
                "{} is not empty (pass --force to write into it anyway)",
                dir.display()
            ));
        }
    }
    let fynkel_pkg = root.join("packages/fynkel");
    let flutter_pkg = root.join("packages/fynkel_flutter");
    let generator_pkg = root.join("packages/fynkel_generator");
    for p in [&fynkel_pkg, &flutter_pkg, &generator_pkg] {
        if !p.join("pubspec.yaml").is_file() {
            return Err(format!(
                "{} is not a Fynkel checkout ({} missing); pass --root <repo>",
                root.display(),
                p.display()
            ));
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    // Platform runners first: `flutter create` writes a whole app, then the
    // Fynkel files replace its pubspec, main.dart, README, lints and
    // .gitignore.
    let mut used_flutter = false;
    if opts.flutter {
        if let Some(flutter) = find_flutter() {
            flutter_create(&flutter, dir, &package, &opts.org)?;
            used_flutter = true;
        }
    }

    // Placeholders: identity, SDK paths and fresh ids.
    let mut vars: Vec<(String, String)> = vec![
        ("package".into(), package.clone()),
        ("title".into(), title.clone()),
        ("fynkel".into(), dependency_path(dir, &fynkel_pkg, root)),
        (
            "fynkel_flutter".into(),
            dependency_path(dir, &flutter_pkg, root),
        ),
        (
            "generator".into(),
            dependency_path(dir, &generator_pkg, root),
        ),
    ];
    for key in [
        "project_id",
        "scene_id",
        "material_id",
        "cube_id",
        "swatch_id",
        "e_camera",
        "c_camera_transform",
        "c_camera",
        "e_sun",
        "c_sun_transform",
        "c_sun",
        "e_cube",
        "c_cube_transform",
        "c_cube_mesh",
        "c_cube_spinner",
        "c_cube_mover",
        "c_cube_body",
        "c_cube_collider",
        "e_floor",
        "c_floor_transform",
        "c_floor_mesh",
        "c_floor_collider",
        "e_crate",
        "c_crate_transform",
        "c_crate_mesh",
        "c_crate_body",
        "c_crate_collider",
    ] {
        vars.push((key.into(), Uuid::new_v4().to_string()));
    }

    let mut files = Vec::new();
    for f in FILES {
        let target = dir.join(f.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        match f.body {
            Body::Text(text) => {
                let out = substitute(text, &vars)?;
                std::fs::write(&target, out).map_err(|e| format!("{}: {e}", target.display()))?;
            }
            Body::Binary(bytes) => {
                std::fs::write(&target, bytes).map_err(|e| format!("{}: {e}", target.display()))?;
            }
        }
        files.push(f.path.to_string());
    }

    // Sidecars with the ids the scene and material reference, sub-assets
    // filled by a real import.
    let limits = Limits::default();
    for (rel, id_key) in SOURCES {
        let source = dir.join(rel);
        let id: Uuid = vars
            .iter()
            .find(|(k, _)| k == id_key)
            .map(|(_, v)| v.parse().unwrap())
            .unwrap();
        let mut meta = Meta::new(&source);
        meta.document
            .root
            .set("id", fynkel_document::Node::string(id.to_string()));
        let imported = fynkel_import::import(&source, &meta, &limits);
        if imported.intermediate.is_none() {
            return Err(format!(
                "{} did not import:\n{}",
                rel,
                imported
                    .diagnostics
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        meta.sub_assets = imported.sub_assets;
        let sidecar = sidecar_path(&source);
        let text = meta.to_document().write().map_err(|d| d.to_string())?;
        std::fs::write(&sidecar, text).map_err(|e| format!("{}: {e}", sidecar.display()))?;
        files.push(format!("{rel}.meta.json"));
    }

    // The lock for this build.
    let lock = fynkel_import::expected_lock();
    let lock_path = dir.join(fynkel_document::project::LOCK_FILE);
    std::fs::write(&lock_path, lock.to_text())
        .map_err(|e| format!("{}: {e}", lock_path.display()))?;
    files.push(fynkel_document::project::LOCK_FILE.to_string());

    if used_flutter {
        relax_sandbox(dir)?;
    }

    Ok(Created {
        dir: dir.clone(),
        package,
        title,
        files,
        flutter: used_flutter,
    })
}

/// Dart package names: lower-case letters, digits and underscores, starting
/// with a letter, not a reserved word.
fn check_package_name(name: &str) -> Result<(), String> {
    let ok = name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    const RESERVED: &[&str] = &[
        "assert", "break", "case", "catch", "class", "const", "continue", "default", "do", "else",
        "enum", "extends", "false", "final", "finally", "for", "if", "in", "is", "new", "null",
        "rethrow", "return", "super", "switch", "this", "throw", "true", "try", "var", "void",
        "while", "with", "flutter", "fynkel", "test",
    ];
    if !ok || RESERVED.contains(&name) {
        return Err(format!(
            "\"{name}\" is not a valid package name (lower_snake_case starting with a letter, not a Dart keyword); pass --name"
        ));
    }
    Ok(())
}

/// `my_game` → `My Game`.
fn title_of(package: &str) -> String {
    package
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The pubspec `path:` for an SDK package: relative when the project lives
/// inside the checkout (portable within the repository), absolute otherwise.
fn dependency_path(project: &Path, package: &Path, root: &Path) -> String {
    let abs_project = absolute(project);
    let abs_package = absolute(package);
    let abs_root = absolute(root);
    if abs_project.starts_with(&abs_root) {
        let mut rel = PathBuf::new();
        for _ in abs_project.strip_prefix(&abs_root).unwrap().components() {
            rel.push("..");
        }
        if let Ok(rest) = abs_package.strip_prefix(&abs_root) {
            return rel.join(rest).display().to_string();
        }
    }
    abs_package.display().to_string()
}

fn absolute(p: &Path) -> PathBuf {
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    std::fs::canonicalize(&joined).unwrap_or_else(|_| {
        // Not created yet: normalise `.`/`..` textually.
        let mut out = PathBuf::new();
        for c in joined.components() {
            match c {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other),
            }
        }
        out
    })
}

fn substitute(text: &str, vars: &[(String, String)]) -> Result<String, String> {
    let mut out = text.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    if let Some(i) = out.find("{{") {
        let rest = &out[i..];
        let end = rest.find("}}").map(|e| e + 2).unwrap_or(rest.len().min(40));
        return Err(format!(
            "template placeholder without a value: {}",
            &rest[..end]
        ));
    }
    Ok(out)
}

/// `flutter` on `PATH`.
pub fn find_flutter() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("flutter"))
        .find(|p| p.is_file())
}

fn flutter_create(flutter: &Path, dir: &Path, package: &str, org: &str) -> Result<(), String> {
    let output = Command::new(flutter)
        .args([
            "create",
            "--empty",
            "--platforms",
            "macos,ios",
            "--project-name",
            package,
            "--org",
            org,
        ])
        .arg(dir)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", flutter.display()))?;
    if !output.status.success() {
        return Err(format!(
            "`flutter create` failed ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// A dev build reads the project directory from disk (`World.loadProject`),
/// which the macOS app sandbox forbids; the Fynkel Flutter host runs the
/// same way. Release packaging bundles cooked assets instead (M3+).
fn relax_sandbox(dir: &Path) -> Result<(), String> {
    for name in ["DebugProfile.entitlements", "Release.entitlements"] {
        let path = dir.join("macos/Runner").join(name);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let relaxed = text.replace(
            "<key>com.apple.security.app-sandbox</key>\n\t<true/>",
            "<key>com.apple.security.app-sandbox</key>\n\t<false/>",
        );
        if relaxed != text {
            std::fs::write(&path, relaxed).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}
