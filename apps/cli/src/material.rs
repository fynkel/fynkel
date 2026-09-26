//! `fynkel material …` and `fynkel shader …` (§14.1): `validate` and
//! `inspect` for material documents and shader manifests. Materials are
//! checked against the built-in shaders plus any manifests passed with
//! `--shader <path>` (repeatable). Same exit codes and `--json` shape as
//! `fynkel document`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fynkel_document::material::manifest_json;
use fynkel_document::{
    diagnostics_json, has_errors, Diagnostic, Document, DocumentKind, Material, SchemaSet,
    ShaderLibrary, ShaderManifest,
};

pub fn usage(cmd: &str) -> ExitCode {
    if cmd == "shader" {
        eprintln!(
            "usage:\n  fynkel shader inspect <path> [--json] [--schemas <dir>]      parameters, texture slots and passes of a manifest\n  fynkel shader validate <path>... [--json] [--schemas <dir>]  schema + manifest rules\n  fynkel shader inspect --builtin <shaderId>                   the same for a built-in shader (fynkel.pbr@1)"
        );
    } else {
        eprintln!(
            "usage:\n  fynkel material inspect <path> [--json] [--schemas <dir>] [--shader <manifest>]...\n                                               resolved parameters and texture bindings\n  fynkel material validate <path>... [--json] [--schemas <dir>] [--shader <manifest>]...\n                                               schema + validation against the shader manifest"
        );
    }
    ExitCode::from(2)
}

struct Opts {
    paths: Vec<PathBuf>,
    json: bool,
    schemas_dir: Option<PathBuf>,
    shaders: Vec<PathBuf>,
    builtin: Option<String>,
}

fn parse(cmd: &str, args: &[String]) -> Option<Opts> {
    let mut o = Opts {
        paths: Vec::new(),
        json: false,
        schemas_dir: None,
        shaders: Vec::new(),
        builtin: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--schemas" => {
                i += 1;
                o.schemas_dir = args.get(i).map(PathBuf::from);
            }
            "--shader" => {
                i += 1;
                o.shaders.push(PathBuf::from(args.get(i)?));
            }
            "--builtin" => {
                i += 1;
                o.builtin = Some(args.get(i)?.clone());
            }
            "--root" => i += 1,
            other if other.starts_with("--") => {
                eprintln!("fynkel {cmd}: unknown argument {other}");
                return None;
            }
            p => o.paths.push(PathBuf::from(p)),
        }
        i += 1;
    }
    Some(o)
}

fn report(diags: &[Diagnostic], json: bool) -> ExitCode {
    if json {
        println!("{}", diagnostics_json(diags));
    } else {
        for d in diags {
            eprintln!("{d}");
        }
    }
    ExitCode::from(1)
}

/// `cmd` is `material` or `shader`.
pub fn run(root: Option<&Path>, cmd: &str, args: &[String]) -> ExitCode {
    let Some(sub) = args.first() else {
        return usage(cmd);
    };
    let Some(opts) = parse(cmd, &args[1..]) else {
        return usage(cmd);
    };
    if opts.paths.is_empty() && opts.builtin.is_none() {
        return usage(cmd);
    }
    let schemas = match crate::schemas::load(root, opts.schemas_dir.as_deref()) {
        Ok(s) => s,
        Err(d) => return report(&[d], opts.json),
    };
    match (cmd, sub.as_str()) {
        ("shader", "inspect") => shader_inspect(&opts, &schemas),
        ("shader", "validate") => shader_validate(&opts, &schemas),
        ("material", "inspect") => material_inspect(&opts, &schemas),
        ("material", "validate") => material_validate(&opts, &schemas),
        _ => usage(cmd),
    }
}

/// A `.shader` file (ADR-0031), its asset id from the sidecar beside it
/// (nil without one). A `.shader.json` manifest is refused with the
/// header that says the same.
fn read_manifest(path: &Path) -> Result<ShaderManifest, Vec<Diagnostic>> {
    let shown = path.display().to_string();
    if shown.ends_with(".shader.json") {
        let m = Document::read(path, Some(DocumentKind::Shader)).map(ShaderManifest::from_document)?;
        return Err(vec![fynkel_document::material::legacy_manifest(&m)]);
    }
    let sidecar = fynkel_document::asset::sidecar_path(path);
    let id = Document::read(&sidecar, Some(DocumentKind::Meta))
        .map(|d| d.envelope.id)
        .unwrap_or(uuid::Uuid::nil());
    fynkel_document::material::read_shader_file(id, &shown, path)
}

/// Built-ins plus `--shader` manifests. A manifest with errors is reported
/// and left out, so materials naming it get `FYN-D0300` rather than being
/// checked against a broken interface.
fn library(opts: &Opts, schemas: &SchemaSet, diags: &mut Vec<Diagnostic>) -> ShaderLibrary {
    let mut lib = ShaderLibrary::builtin();
    for path in &opts.shaders {
        match read_manifest(path) {
            Ok(m) => {
                let d = m.validate(schemas);
                let broken = has_errors(&d);
                diags.extend(d);
                if !broken {
                    lib.add(m);
                }
            }
            Err(d) => diags.extend(d),
        }
    }
    lib
}

fn exit(diags: &[Diagnostic]) -> ExitCode {
    if has_errors(diags) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn shader_inspect(opts: &Opts, schemas: &SchemaSet) -> ExitCode {
    let manifest = match &opts.builtin {
        Some(id) => match ShaderLibrary::builtin().get(id) {
            Some(m) => m.clone(),
            None => {
                return report(
                    &[Diagnostic::error(
                        fynkel_document::Code::MATERIAL_UNKNOWN_SHADER,
                        format!("no built-in shader {id:?}"),
                    )
                    .with_suggestion(format!(
                        "built-ins: {}",
                        ShaderLibrary::builtin().ids().join(", ")
                    ))],
                    opts.json,
                )
            }
        },
        None => match read_manifest(&opts.paths[0]) {
            Ok(m) => m,
            Err(d) => return report(&d, opts.json),
        },
    };
    let diags = manifest.validate(schemas);
    if opts.json {
        let mut v = manifest_json(&manifest);
        v["ok"] = serde_json::Value::Bool(!has_errors(&diags));
        v["diagnostics"] = diagnostics_json(&diags)["diagnostics"].clone();
        println!("{v}");
    } else {
        print!("{}", manifest_text(&manifest));
        for d in &diags {
            eprintln!("{d}");
        }
    }
    exit(&diags)
}

fn manifest_text(m: &ShaderManifest) -> String {
    let lighting = m.shader.as_ref().map_or("lit", |s| s.interface.lighting.name());
    let mut s = format!(
        "{} ({})\n  lighting {lighting}\n  passes {}\n  parameters\n",
        m.name,
        m.shader_id,
        m.passes
            .iter()
            .map(|p| p.name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    for p in &m.parameters {
        let range = match (p.min, p.max) {
            (Some(a), Some(b)) => format!("  [{a}, {b}]"),
            (Some(a), None) => format!("  >= {a}"),
            (None, Some(b)) => format!("  <= {b}"),
            (None, None) => String::new(),
        };
        s.push_str(&format!(
            "    {:<20} {:<7} default {}{}\n",
            p.name,
            p.ty.name(),
            p.default
                .as_ref()
                .map(|d| d.to_json().to_string())
                .unwrap_or_else(|| "?".into()),
            range
        ));
    }
    s.push_str("  textures\n");
    for t in &m.textures {
        s.push_str(&format!(
            "    {:<20} {:<7}{}{}\n",
            t.name,
            t.color_space.name(),
            if t.required { "  required" } else { "" },
            t.default
                .as_ref()
                .map(|d| format!("  fallback {d}"))
                .unwrap_or_default()
        ));
    }
    s
}

fn shader_validate(opts: &Opts, schemas: &SchemaSet) -> ExitCode {
    let mut diags = Vec::new();
    for path in &opts.paths {
        match read_manifest(path) {
            Ok(m) => diags.extend(m.validate(schemas)),
            Err(d) => diags.extend(d),
        }
    }
    if opts.json {
        println!("{}", diagnostics_json(&diags));
    } else {
        for d in &diags {
            eprintln!("{d}");
        }
        if !has_errors(&diags) {
            eprintln!(
                "fynkel shader validate: {} shader(s) ok",
                opts.paths.len()
            );
        }
    }
    exit(&diags)
}

fn material_inspect(opts: &Opts, schemas: &SchemaSet) -> ExitCode {
    let mut diags = Vec::new();
    let lib = library(opts, schemas, &mut diags);
    let material = match Document::read(&opts.paths[0], Some(DocumentKind::Material)) {
        Ok(doc) => Material::from_document(doc),
        Err(d) => {
            diags.extend(d);
            return report(&diags, opts.json);
        }
    };
    diags.extend(material.validate(schemas, &lib));
    let resolved = material.resolve(&lib);
    if opts.json {
        let mut v = match &resolved {
            Some(r) => r.to_json(),
            None => serde_json::json!({
                "kind": "material",
                "id": material.id().to_string(),
                "name": material.document.envelope.name,
                "shader": material.shader,
            }),
        };
        v["ok"] = serde_json::Value::Bool(!has_errors(&diags));
        v["diagnostics"] = diagnostics_json(&diags)["diagnostics"].clone();
        println!("{v}");
    } else {
        match &resolved {
            Some(r) => print!("{}", r.to_text()),
            None => println!(
                "{} ({})\n  shader {} (unknown)",
                material.document.envelope.name,
                material.id(),
                material.shader
            ),
        }
        for d in &diags {
            eprintln!("{d}");
        }
    }
    exit(&diags)
}

fn material_validate(opts: &Opts, schemas: &SchemaSet) -> ExitCode {
    let mut diags = Vec::new();
    let lib = library(opts, schemas, &mut diags);
    for path in &opts.paths {
        match Document::read(path, Some(DocumentKind::Material)) {
            Ok(doc) => diags.extend(Material::from_document(doc).validate(schemas, &lib)),
            Err(d) => diags.extend(d),
        }
    }
    if opts.json {
        println!("{}", diagnostics_json(&diags));
    } else {
        for d in &diags {
            eprintln!("{d}");
        }
        if !has_errors(&diags) {
            eprintln!(
                "fynkel material validate: {} material(s) ok",
                opts.paths.len()
            );
        }
    }
    exit(&diags)
}
