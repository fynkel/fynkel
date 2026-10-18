//! Stage 10 phase 4b: packages/fynkel/wgsl/fynkel.wgsl, the reference to
//! what a shader can use, says what the engine has: each function with
//! the engine's signature, each struct with its fields, and every function
//! a shader may call is in it. And the helpers work in screen effects too.

use std::collections::BTreeMap;

use fynkel_render::CustomShader;

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The files a shader's own code is joined to.
fn engine() -> String {
    ["src/forward.wgsl", "src/library.wgsl", "src/helpers.wgsl", "src/effect.wgsl"].map(read).concat()
}

/// Each `fn` line's name and signature, up to its `{`.
fn functions(source: &str) -> BTreeMap<String, String> {
    source
        .lines()
        .filter_map(|l| l.strip_prefix("fn "))
        .map(|l| {
            let name = l.split('(').next().unwrap().to_string();
            (name, l.trim_end_matches('{').trim().to_string())
        })
        .collect()
}

/// Each struct's fields, as `name: type` without comments or spaces.
fn structs(source: &str) -> BTreeMap<String, Vec<String>> {
    let mut found = BTreeMap::new();
    let mut lines = source.lines();
    while let Some(line) = lines.next() {
        let Some(name) = line.strip_prefix("struct ").and_then(|l| l.strip_suffix(" {")) else { continue };
        let fields = lines
            .by_ref()
            .take_while(|l| !l.starts_with('}'))
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("//"))
            .map(|l| l.replace(' ', ""))
            .collect();
        found.insert(name.to_string(), fields);
    }
    found
}

#[test]
fn the_reference_says_what_the_engine_has() {
    let reference = read("../../packages/fynkel/wgsl/fynkel.wgsl");
    let engine = engine();
    let (theirs, ours) = (functions(&engine), functions(&reference));
    for (name, signature) in &ours {
        assert_eq!(theirs.get(name), Some(signature), "fynkel.wgsl's {name} isn't the engine's");
    }
    let (theirs, ours) = (structs(&engine), structs(&reference));
    for (name, fields) in &ours {
        assert_eq!(theirs.get(name), Some(fields), "fynkel.wgsl's struct {name} isn't the engine's");
    }
    // Every function a shader may call is in it: what the library, the
    // helpers and effect.wgsl have, but the engine's own.
    let callable = ["src/library.wgsl", "src/helpers.wgsl", "src/effect.wgsl"].map(read).concat();
    let ours = functions(&reference);
    for name in functions(&callable).keys().filter(|n| !n.starts_with("fy_") && !n.starts_with("vs_fy")) {
        assert!(ours.contains_key(name), "{name} isn't in packages/fynkel/wgsl/fynkel.wgsl");
    }
}

#[test]
fn an_effect_can_use_the_helpers() {
    let source = "fn effect(in: EffectInput) -> vec4<f32> {\n    \
                  let grain = remap(fbm(in.pixel) + noise3(vec3<f32>(in.uv, in.time)) + hash(in.pixel), 0.0, 3.0, 0.9, 1.0);\n    \
                  return vec4<f32>(picture(in.uv).rgb * grain, 1.0);\n}\n";
    let shader = CustomShader::new("grain.wgsl", source).unwrap_or_else(|e| panic!("{e}"));
    assert!(shader.effect);
}

/// Every ```wgsl block in docs/writing-a-game.md is a shader that compiles,
/// as its first line's file name says.
#[test]
fn the_guides_shaders_compile() {
    let guide = read("../../docs/writing-a-game.md");
    let blocks: Vec<&str> = guide.split("```wgsl\n").skip(1).map(|b| b.split("```").next().unwrap()).collect();
    assert!(blocks.len() >= 5, "{} shaders in the guide", blocks.len());
    for block in blocks {
        let name = block.lines().next().and_then(|l| l.strip_prefix("// ")).unwrap_or("guide.wgsl");
        CustomShader::new(name, block).unwrap_or_else(|e| panic!("the guide's {name}: {e}"));
    }
}
