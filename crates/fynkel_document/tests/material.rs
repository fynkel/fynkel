//! Material / shader corpus (`fixtures/material/*`): each document's
//! `*.expected.json` lists the diagnostics validation must report (code,
//! pointer, optional severity), in order. Materials are checked against the
//! built-in library plus every `*.shader` file in the directory (ADR-0031);
//! JSON manifests (the built-in's form) are checked on their own.

use std::path::{Path, PathBuf};

use fynkel_document::material::{
    AlphaMode, ColorSpace, ParameterType, ParameterValue, Pass, PBR_SHADER_ID,
};
use fynkel_document::{Document, DocumentKind, Material, SchemaSet, ShaderLibrary, ShaderManifest};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/material")
}

fn schemas() -> SchemaSet {
    SchemaSet::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")).unwrap()
}

fn library_with_fixture_shaders(_schemas: &SchemaSet) -> ShaderLibrary {
    let mut lib = ShaderLibrary::builtin();
    for path in std::fs::read_dir(fixtures_dir()).unwrap() {
        let path = path.unwrap().path();
        if path.extension().is_some_and(|e| e == "shader") {
            let meta = fynkel_document::asset::sidecar_path(&path);
            let id = Document::read(&meta, Some(DocumentKind::Meta))
                .unwrap()
                .envelope
                .id;
            let manifest =
                fynkel_document::material::read_shader_file(id, &path.display().to_string(), &path)
                    .unwrap_or_else(|d| panic!("{d:#?}"));
            lib.add(manifest);
        }
    }
    lib
}

fn check(path: &Path, diags: &[fynkel_document::Diagnostic]) {
    let expected: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(path.to_string_lossy().replace(".json", ".expected.json"))
            .unwrap(),
    )
    .unwrap();
    let shown = format!(
        "{}:\n{}",
        path.display(),
        diags.iter().map(|d| format!("  {d}\n")).collect::<String>()
    );
    assert_eq!(diags.len(), expected.len(), "{shown}");
    for (d, e) in diags.iter().zip(&expected) {
        assert_eq!(d.code.as_str(), e["code"].as_str().unwrap(), "{shown}");
        assert_eq!(d.pointer.as_deref(), e["pointer"].as_str(), "{shown}");
        assert_eq!(
            d.severity.as_str(),
            e.get("severity")
                .and_then(|s| s.as_str())
                .unwrap_or("error"),
            "{shown}"
        );
        assert_eq!(d.line, e["line"].as_u64().map(|v| v as u32), "{shown}");
        assert_eq!(d.column, e["column"].as_u64().map(|v| v as u32), "{shown}");
        assert!(d.document.is_some());
    }
}

#[test]
fn material_fixtures_report_exact_diagnostics() {
    let schemas = schemas();
    let lib = library_with_fixture_shaders(&schemas);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".material.json"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 4);
    for path in paths {
        let doc = Document::read(&path, Some(DocumentKind::Material)).unwrap();
        let material = Material::from_document(doc);
        check(&path, &material.validate(&schemas, &lib));
    }
}

#[test]
fn shader_fixtures_report_exact_diagnostics() {
    let schemas = schemas();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".shader.json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        let doc = Document::read(&path, Some(DocumentKind::Shader)).unwrap();
        let manifest = ShaderManifest::from_document(doc);
        let expected_path = path.to_string_lossy().replace(".json", ".expected.json");
        let diags = manifest.validate(&schemas);
        if Path::new(&expected_path).exists() {
            check(&path, &diags);
        } else {
            assert!(diags.is_empty(), "{}: {diags:#?}", path.display());
        }
    }
}

#[test]
fn builtin_pbr_manifest_is_clean_and_complete() {
    let lib = ShaderLibrary::builtin();
    assert_eq!(lib.ids(), vec![PBR_SHADER_ID]);
    let pbr = lib.get(PBR_SHADER_ID).unwrap();
    assert!(pbr.validate(&SchemaSet::embedded()).is_empty());
    assert!(pbr.has_pass(Pass::Opaque) && pbr.has_pass(Pass::Shadow));
    let base = pbr.parameter("baseColor").unwrap();
    assert_eq!(base.ty, ParameterType::Color4);
    assert_eq!(
        base.default,
        Some(ParameterValue::Vector(vec![1.0, 1.0, 1.0, 1.0]))
    );
    assert_eq!(pbr.parameter("roughness").unwrap().max, Some(1.0));
    assert_eq!(
        pbr.texture("baseColor").unwrap().color_space,
        ColorSpace::Srgb
    );
    assert_eq!(
        pbr.texture("normal").unwrap().color_space,
        ColorSpace::Linear
    );
    assert!(pbr.textures.iter().all(|t| !t.required));
}

#[test]
fn resolve_fills_defaults_and_keeps_authored_values() {
    let schemas = schemas();
    let lib = ShaderLibrary::builtin();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/valid/rusted_metal.material.json");
    let material = Material::from_document(Document::read(&path, None).unwrap());
    assert!(material.validate(&schemas, &lib).is_empty());
    let r = material.resolve(&lib).unwrap();
    assert_eq!(r.shader_id, PBR_SHADER_ID);
    assert_eq!(r.alpha_mode, AlphaMode::Mask);
    assert_eq!(r.pass, Pass::Mask);
    assert!((r.alpha_cutoff - 0.4).abs() < 1e-6);
    let pbr = r.pbr.as_ref().unwrap();
    assert_eq!((pbr.metallic, pbr.roughness), (1.0, 0.65));
    assert!(pbr.double_sided);
    assert!(pbr.occlusion_texture.is_none());
    assert_eq!(r.parameters.len(), 7);
    let metallic = r.parameter("metallic").unwrap();
    assert!(metallic.authored);
    assert_eq!(metallic.value, ParameterValue::Float(1.0));
    let emissive = r.parameter("emissive").unwrap();
    assert!(!emissive.authored);
    assert_eq!(emissive.value, ParameterValue::Vector(vec![0.0, 0.0, 0.0]));
    assert_eq!(r.textures.len(), 5);
    let base = r.texture("baseColor").unwrap();
    assert_eq!(base.color_space, ColorSpace::Srgb);
    assert_eq!(
        base.binding.as_ref().unwrap().texture.asset.to_string(),
        "3f8a2b1c-5d6e-4f70-8a9b-0c1d2e3f4a5b"
    );
    let mr = r.texture("metallicRoughness").unwrap();
    assert_eq!(mr.binding.as_ref().unwrap().texture.sub, "packed");
    let occlusion = r.texture("occlusion").unwrap();
    assert!(occlusion.binding.is_none());
    assert_eq!(occlusion.fallback.as_deref(), Some("white"));
    let json = r.to_json();
    assert_eq!(json["pass"], "mask");
    assert_eq!(json["parameters"][0]["name"], "baseColor");
    assert!(r.to_text().contains("roughness"));
}

#[test]
fn fresh_material_validates_against_the_builtin_shader() {
    let doc = Document::new(DocumentKind::Material, "Fresh");
    let material = Material::from_document(doc);
    let diags = material.validate(&SchemaSet::embedded(), &ShaderLibrary::builtin());
    assert!(diags.is_empty(), "{diags:#?}");
    let r = material.resolve(&ShaderLibrary::builtin()).unwrap();
    assert!(r.parameters.iter().all(|p| !p.authored));
}

#[test]
fn unknown_shader_still_resolves_to_nothing() {
    let text = r#"{
  "formatVersion": 1,
  "id": "70d7ba8b-f7fb-4eba-920d-ec9e6b968330",
  "name": "Nope",
  "shader": "studio.toon@9"
}"#;
    let material = Material::parse(None, text).unwrap();
    assert!(material.resolve(&ShaderLibrary::builtin()).is_none());
    let diags = material.validate_semantics(&ShaderLibrary::builtin());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code.as_str(), "FYN-D0300");
}
