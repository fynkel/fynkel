//! A model whose accessors claim more values than the file could hold
//! (stage 14, WO1): a sparse accessor needs no bytes for the values it
//! leaves at 0, so a file of a few hundred bytes can claim 2^44 of them.
//! Reading one allocated that many and aborted the process. Each is
//! refused with an error instead.

use fynkel_load::ModelData;

/// A `.glb` of `json` and `bin`.
fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut j = json.as_bytes().to_vec();
    j.resize(j.len().div_ceil(4) * 4, b' ');
    let mut b = bin.to_vec();
    b.resize(b.len().div_ceil(4) * 4, 0);
    let total = 12 + 8 + j.len() + 8 + b.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&j);
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&b);
    out
}

/// One u8 sparse index (0), one f32 value, then 16 floats: a vec3 and a
/// matrix's worth.
fn bin() -> Vec<u8> {
    let mut b = vec![0u8; 4];
    b.extend_from_slice(&0.5f32.to_le_bytes());
    for i in 0..16 {
        b.extend_from_slice(&(i as f32).to_le_bytes());
    }
    b
}

const VIEWS: &str = r#""buffers":[{"byteLength":72}],
"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":1},{"buffer":0,"byteOffset":4,"byteLength":4},
 {"buffer":0,"byteOffset":8,"byteLength":12},{"buffer":0,"byteOffset":8,"byteLength":64}]"#;

/// An accessor of `count` `kind`s: sparse, or with no buffer view at all.
fn huge(count: u64, kind: &str, sparse: bool) -> String {
    let sparse = if sparse {
        r#","sparse":{"count":1,"indices":{"bufferView":0,"componentType":5121},"values":{"bufferView":1}}"#
    } else {
        ""
    };
    let bounds = if kind == "SCALAR" { r#","min":[0.0],"max":[1.0]"# } else { "" };
    format!(r#"{{"componentType":5126,"count":{count},"type":"{kind}"{bounds}{sparse}}}"#)
}

fn animation(count: u64, sparse: bool) -> Vec<u8> {
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"name":"n"}}],{VIEWS},
"accessors":[{},{{"bufferView":2,"componentType":5126,"count":1,"type":"VEC3"}}],
"animations":[{{"channels":[{{"sampler":0,"target":{{"node":0,"path":"translation"}}}}],
 "samplers":[{{"input":0,"output":1}}]}}]}}"#,
        huge(count, "SCALAR", sparse)
    );
    glb(&json, &bin())
}

fn skin(count: u64) -> Vec<u8> {
    let json = format!(
        r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0,1]}}],
"nodes":[{{"name":"bone"}},{{"name":"body","mesh":0,"skin":0}}],{VIEWS},
"accessors":[{},{{"bufferView":2,"componentType":5126,"count":1,"type":"VEC3","min":[0,1,2],"max":[0,1,2]}}],
"meshes":[{{"primitives":[{{"attributes":{{"POSITION":1}},"mode":0}}]}}],
"skins":[{{"joints":[0],"inverseBindMatrices":0}}]}}"#,
        huge(count, "MAT4", true)
    );
    glb(&json, &bin())
}

#[test]
fn a_small_file_claiming_billions_of_animation_keys_is_refused() {
    // The same file with one key loads: the file itself is sound.
    assert!(ModelData::from_glb(&animation(1, true)).is_ok());
    let error = ModelData::from_glb(&animation(1 << 44, true)).expect_err("refused");
    assert!(error.to_string().contains("more than the file has bytes"), "{error}");
    // With no buffer view and not sparse, glTF itself says it's broken.
    assert!(ModelData::from_glb(&animation(1 << 44, false)).is_err());
}

#[test]
fn a_small_file_claiming_billions_of_inverse_bind_matrices_is_refused() {
    let error = ModelData::from_glb(&skin(1 << 44)).expect_err("refused");
    assert!(error.to_string().contains("more than the file has bytes"), "{error}");
}
