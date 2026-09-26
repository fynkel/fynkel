//! Shader assets in the authoring service (ADR-0031 § 7): the `shaders`
//! query the material Inspector's Shader dropdown and a shader's page
//! read, and the Inspector's live edit of a shader asset's material.

use std::path::{Path, PathBuf};

use fynkel_assets::Assets;
use fynkel_authoring::{AuthoringSession, Preview};
use serde_json::{json, Value};
use uuid::Uuid;

const TINT: &str = "c0000000-0000-4000-8000-000000000701";
const BROKEN: &str = "c0000000-0000-4000-8000-000000000702";
const TINTED: &str = "c0000000-0000-4000-8000-000000000703";

const TINT_SHADER: &str = "shader \"Tests/Tint\" {\n    lighting unlit\n    alpha opaque, mask\n    shadows off\n    properties {\n        tint color3 = (1, 0.5, 0) \"The colour\"\n        power float = 1 range(0, 4)\n        glow texture = black linear\n    }\n}\n\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    s.base_color = material.tint * material.power + sample_glow(in.uv0).rgb;\n    return s;\n}\n";

const BROKEN_SHADER: &str = "shader \"Tests/Broken\" {\n    properties {\n        amount float = 1\n    }\n}\n\nfn surface(in: SurfaceInput) -> Surface {\n    var s = default_surface();\n    s.base_color = material.missing;\n    return s;\n}\n";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
}

/// A private copy of the fixture project with two shaders and a material
/// (removed on drop).
struct Copy {
    dir: PathBuf,
}

impl Copy {
    fn new(name: &str) -> Copy {
        let dir =
            std::env::temp_dir().join(format!("fynkel-shaders-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture(), &dir);
        let shaders = dir.join("assets/shaders");
        std::fs::create_dir_all(&shaders).unwrap();
        for (file, id, text) in [
            ("tint.shader", TINT, TINT_SHADER),
            ("broken.shader", BROKEN, BROKEN_SHADER),
        ] {
            std::fs::write(shaders.join(file), text).unwrap();
            std::fs::write(
                shaders.join(format!("{file}.meta.json")),
                format!(r#"{{"$schema":"https://fynkel.dev/schemas/meta.schema.json","formatVersion":2,"id":"{id}","name":"{file}","importer":"fynkel.shader","importerVersion":1,"settings":{{}},"subAssets":{{}}}}"#),
            )
            .unwrap();
        }
        std::fs::write(
            dir.join("assets/materials/tinted.material.json"),
            format!(r#"{{"$schema":"https://fynkel.dev/schemas/material.schema.json","formatVersion":2,"id":"{TINTED}","name":"Tinted","shader":{{"asset":"{TINT}"}},"parameters":{{"tint":[0,0,1]}}}}"#),
        )
        .unwrap();
        Copy {
            dir: dir.canonicalize().unwrap(),
        }
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

fn open(copy: &Copy) -> AuthoringSession {
    let mut s = AuthoringSession::new();
    s.open_project(&copy.dir)
        .map_err(|d| format!("{d:?}"))
        .unwrap();
    s
}

fn by_shader<'a>(list: &'a Value, shader: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|v| v["shader"] == shader)
        .unwrap_or_else(|| panic!("{shader} in {list:#}"))
}

#[test]
fn the_shaders_query_lists_the_built_in_and_the_projects_shaders() {
    let copy = Copy::new("query");
    let s = open(&copy);
    let list = s.query("shaders", &json!({})).unwrap();

    // The built-in first, as Unity's Lit.
    let lit = &list[0];
    assert_eq!(lit["shader"], "fynkel.pbr@1");
    assert_eq!(lit["name"], "Lit");
    assert_eq!(lit["builtin"], true);
    let names: Vec<&str> = lit["properties"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"baseColor") && names.contains(&"metallic"),
        "{names:?}"
    );

    let tint = by_shader(&list, TINT);
    assert_eq!(tint["name"], "Tests/Tint", "{tint:#}");
    assert_eq!(tint["path"], "shaders/tint.shader");
    assert_eq!(tint["ok"], true);
    assert_eq!(tint["lighting"], "unlit");
    assert_eq!(tint["alpha"], json!(["opaque", "mask"]));
    assert_eq!(tint["shadows"], false);
    assert_eq!(tint["vertex"], false);
    assert_eq!(
        tint["properties"],
        json!([
            { "name": "tint", "label": "Tint", "line": 6, "tooltip": "The colour",
              "type": "color3", "default": [1.0, 0.5, 0.0] },
            { "name": "power", "label": "Power", "line": 7, "type": "float", "default": 1.0,
              "min": 0.0, "max": 4.0 },
            { "name": "glow", "label": "Glow", "line": 8, "type": "texture", "default": "black",
              "colorSpace": "linear", "required": false },
        ])
    );
    assert!(tint["file"]
        .as_str()
        .unwrap()
        .ends_with("assets/shaders/tint.shader"));

    // A shader that does not compile: its errors, with the line.
    let broken = by_shader(&list, BROKEN);
    assert_eq!(broken["ok"], false);
    assert_eq!(broken["name"], "broken.shader");
    let d = &broken["diagnostics"][0];
    assert_eq!(d["severity"], "error");
    assert_eq!(d["line"], 9, "{d:#}");
    assert_eq!(broken["properties"], json!([]));
}

#[test]
fn the_inspectors_live_edit_of_a_shader_material_is_checked_and_drawn() {
    let copy = Copy::new("live");
    let s = open(&copy);
    let report = s.project().unwrap().cook();
    // The broken shader fails the cook; the rest is cooked.
    assert!(report.has_errors());
    let assets = Assets::open(&s.project().unwrap().cache_dir()).unwrap();
    let tinted = Uuid::parse_str(TINTED).unwrap();
    let r = fynkel_contracts::AssetRef::new(fynkel_contracts::Uuid(*tinted.as_bytes()));
    let block = |a: &Assets| a.custom_material(&r).unwrap().unwrap().block.clone();
    let floats = |b: &[u8], i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    assert_eq!(floats(&block(&assets), 2), 1.0);

    let fields = json!({
        "shader": { "asset": TINT },
        "parameters": { "tint": [1, 0, 0], "power": 3 },
    });
    Preview::set_material(&assets, tinted, &fields).unwrap();
    let b = block(&assets);
    assert_eq!(
        (floats(&b, 0), floats(&b, 2), floats(&b, 4)),
        (1.0, 0.0, 3.0)
    );

    // Checked as the cook checks: out of range is refused.
    let wrong = json!({ "shader": { "asset": TINT }, "parameters": { "power": 9 } });
    let refused = Preview::set_material(&assets, tinted, &wrong).unwrap_err();
    assert!(refused.message.contains("power"), "{}", refused.message);

    // The built-in's fields: drawn as a PBR material.
    Preview::set_material(&assets, tinted, &json!({ "shader": "fynkel.pbr@1" })).unwrap();
    assert!(assets.custom_material(&r).unwrap().is_none());

    Preview::restore_material(&assets, tinted);
    assert_eq!(floats(&block(&assets), 2), 1.0);
}
