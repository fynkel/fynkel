//! The FBX importer against the glTF importer on the same content
//! (blueprint §19.2d item 19b): `rig.fbx` and `robot.fbx` are `rig.glb`
//! and `robot.glb` round-tripped through Blender (`fixtures/blender_fbx.py`),
//! so every number below is checked against what the glTF path gives —
//! joints, weights, bind matrices, clips, materials, UVs, the prefab.

use std::path::{Path, PathBuf};

use fynkel_contracts::{has_errors, Code, Severity};
use fynkel_cooked::payload::{
    AnimationChannel, ANIM_INTERPOLATION_LINEAR, ANIM_PROPERTY_ROTATION, ANIM_PROPERTY_TRANSLATION,
};
use fynkel_document::asset::{sidecar_path, Meta};
use fynkel_document::{Document, DocumentKind, Material, Prefab, SchemaSet, ShaderLibrary};
use fynkel_import::{derive_sub_id, import, ColorSpace, Imported, Intermediate, Limits};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn meta_for(source: &Path) -> Meta {
    let sidecar = sidecar_path(source);
    if sidecar.exists() {
        Meta::from_document(Document::read(&sidecar, None).unwrap())
    } else {
        Meta::new(source)
    }
}

fn import_fixture(name: &str) -> (Imported, Meta) {
    let path = fixture(name);
    let meta = meta_for(&path);
    let r = import(&path, &meta, &Limits::default());
    (r, meta)
}

fn codes(r: &Imported) -> Vec<Code> {
    r.diagnostics.iter().map(|d| d.code).collect()
}

fn close(a: &[f32], b: &[f32], eps: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= eps)
}

/// The key of a channel at (or nearest to) `t`, as a slice of its floats.
fn key_at(channel: &AnimationChannel, t: f32) -> &[f32] {
    let n = channel.key_floats();
    let (k, _) = channel
        .times
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - t).abs().total_cmp(&(b.1 - t).abs()))
        .unwrap();
    &channel.values[k * n..(k + 1) * n]
}

/// `a * b` as `[x, y, z, w]` quaternions.
fn qmul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// `v` rotated by the `[x, y, z, w]` quaternion `q`.
fn qrot(q: [f32; 4], v: &[f32]) -> [f32; 3] {
    let p = [v[0], v[1], v[2], 0.0];
    let conj = [-q[0], -q[1], -q[2], q[3]];
    let r = qmul(qmul(q, p), conj);
    [r[0], r[1], r[2]]
}

/// The rotation of the named prefab entity's transform.
fn entity_rotation(prefab: &serde_json::Value, name: &str) -> [f32; 4] {
    let entity = prefab["entities"]
        .as_object()
        .unwrap()
        .values()
        .find(|e| e["name"] == name)
        .unwrap();
    let transform = entity["components"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(transform["type"], "fynkel.Transform");
    let q: Vec<f32> = transform["data"]["rotation"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    [q[0], q[1], q[2], q[3]]
}

fn channel<'a>(
    i: &'a Intermediate,
    clip: &str,
    target: &str,
    property: u32,
) -> &'a AnimationChannel {
    i.animations
        .iter()
        .find(|a| a.name == clip)
        .unwrap_or_else(|| panic!("no clip {clip}"))
        .payload
        .channels
        .iter()
        .find(|c| c.target_name == target && c.property == property)
        .unwrap_or_else(|| panic!("{clip}: no {property} channel for {target}"))
}

#[test]
fn the_new_extension_maps_to_the_importer() {
    assert_eq!(
        fynkel_document::asset::importer_for(Path::new("x/hero.FBX")),
        "fynkel.fbx"
    );
    assert!(fynkel_import::IMPORTERS
        .iter()
        .any(|(name, version)| *name == "fynkel.fbx" && *version == 5));
}

#[test]
fn rig_fbx_matches_the_gltf_rig() {
    let (r, meta) = import_fixture("rig.fbx");
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    assert!(
        r.diagnostics.is_empty(),
        "a clean rig imports silently: {:#?}",
        r.diagnostics
    );
    let i = r.intermediate.as_ref().unwrap();
    assert_eq!(i.importer, "fynkel.fbx");
    assert_eq!(i.importer_version, 5);
    assert_eq!(i.source, "rig.fbx");
    let (g, _) = import_fixture("rig.glb");
    let g = g.intermediate.unwrap();

    // The column: Blender's round trip keeps its 16 vertices (the
    // welding rebuilds the indexed mesh from the file's corners), its
    // bounds and, per position, its weights.
    let body = i.meshes.iter().find(|m| m.name == "Body").unwrap();
    let gbody = g.meshes.iter().find(|m| m.name == "Body").unwrap();
    assert_eq!(body.sub_id, "mesh/Body");
    assert_eq!(body.vertices.len(), 16);
    assert_eq!(body.triangle_count(), 24);
    assert!(close(&body.bounds_min, &gbody.bounds_min, 1e-5));
    assert!(close(&body.bounds_max, &gbody.bounds_max, 1e-5));
    assert!(body.is_skinned());
    assert_eq!(body.skin.len(), body.vertices.len());
    for (v, w) in body.vertices.iter().zip(&body.skin) {
        let (gv, gw) = gbody
            .vertices
            .iter()
            .zip(&gbody.skin)
            .find(|(x, _)| close(&x.position, &v.position, 1e-5))
            .unwrap_or_else(|| panic!("no glTF vertex at {:?}", v.position));
        assert!(
            close(&gv.normal, &v.normal, 1e-4),
            "{:?} vs {:?}",
            v.normal,
            gv.normal
        );
        // The same joints with the same weights (the file lists them by
        // weight, so a 0.5/0.5 split may come in either order).
        let mut mine: Vec<(u16, f32)> = w
            .joints
            .iter()
            .copied()
            .zip(w.weights)
            .filter(|(_, w)| *w > 0.0)
            .collect();
        let mut theirs: Vec<(u16, f32)> = gw
            .joints
            .iter()
            .copied()
            .zip(gw.weights)
            .filter(|(_, w)| *w > 0.0)
            .collect();
        mine.sort_by(|a, b| a.0.cmp(&b.0));
        theirs.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(mine.len(), theirs.len(), "{:?}", v.position);
        for (a, b) in mine.iter().zip(&theirs) {
            assert_eq!(a.0, b.0);
            assert!((a.1 - b.1).abs() < 1e-5);
        }
    }
    let prop = i.meshes.iter().find(|m| m.name == "Prop").unwrap();
    assert_eq!(prop.sub_id, "mesh/Prop");
    assert_eq!(prop.vertices.len(), 24);
    assert!(!prop.is_skinned());

    // The skin: the same joints in the same order, the same parents,
    // rest pose and inverse bind matrices, hips as the skeleton root.
    assert_eq!(i.skins.len(), 1);
    let skin = &i.skins[0];
    let gskin = &g.skins[0];
    assert_eq!(skin.sub_id, "skin/Rig");
    assert_eq!(skin.payload.skeleton, skin.payload.joints[0].binding);
    let names: Vec<&str> = skin
        .payload
        .joints
        .iter()
        .map(|j| j.name.as_str())
        .collect();
    assert_eq!(names, ["hips", "spine", "head"]);
    // Blender writes its armature convention as a -90° X turn on the
    // armature node and +90° X on the root bone (world poses unchanged),
    // so the root's rest is compared in the armature's space.
    let rig_rotation = entity_rotation(i.prefab.as_ref().unwrap(), "Rig");
    for (j, gj) in skin.payload.joints.iter().zip(&gskin.payload.joints) {
        assert_eq!(j.parent, gj.parent, "{}", j.name);
        assert!(
            close(&j.inverse_bind, &gj.inverse_bind, 1e-4),
            "{}: {:?} vs {:?}",
            j.name,
            j.inverse_bind,
            gj.inverse_bind
        );
        assert!(
            close(&j.rest_translation, &gj.rest_translation, 1e-4),
            "{}",
            j.name
        );
        let rotation = if j.parent.is_none() {
            qmul(rig_rotation, j.rest_rotation)
        } else {
            j.rest_rotation
        };
        assert!(
            close(&rotation, &gj.rest_rotation, 1e-4),
            "{}: {:?} vs {:?}",
            j.name,
            rotation,
            gj.rest_rotation
        );
        assert!(close(&j.rest_scale, &gj.rest_scale, 1e-4), "{}", j.name);
    }

    // The clips: one per stack, named after it, in the file's order, with
    // the same length; their curves baked at the file's 24 fps into
    // linear keys that hit the glTF keys' values.
    let clips: Vec<(&str, f32)> = i
        .animations
        .iter()
        .map(|a| (a.name.as_str(), a.payload.duration))
        .collect();
    assert_eq!(clips, [("bob", 2.0), ("wave", 1.0)]);
    assert_eq!(i.animations[0].sub_id, "anim/bob");
    assert_eq!(i.animations[1].sub_id, "anim/wave");
    let spine = channel(i, "wave", "spine", ANIM_PROPERTY_ROTATION);
    assert_eq!(spine.interpolation, ANIM_INTERPOLATION_LINEAR);
    assert_eq!(spine.times[0], 0.0);
    assert!(spine.times.contains(&0.5), "{:?}", spine.times);
    let half = std::f32::consts::FRAC_PI_8;
    assert!(close(key_at(spine, 0.0), &[0.0, 0.0, 0.0, 1.0], 1e-4));
    assert!(
        close(
            key_at(spine, 0.5),
            &[0.0, 0.0, half.sin(), half.cos()],
            1e-3
        ),
        "{:?}",
        key_at(spine, 0.5)
    );
    assert!(close(key_at(spine, 1.0), &[0.0, 0.0, 0.0, 1.0], 1e-4));
    let head = channel(i, "wave", "head", ANIM_PROPERTY_ROTATION);
    let quarter = std::f32::consts::FRAC_PI_4;
    assert!(close(key_at(head, 0.0), &[0.0, 0.0, 0.0, 1.0], 1e-4));
    let end = key_at(head, 1.0);
    assert!(
        close(end, &[0.0, quarter.sin(), 0.0, quarter.cos()], 1e-3)
            || close(end, &[0.0, -quarter.sin(), 0.0, -quarter.cos()], 1e-3),
        "{end:?}"
    );
    // The hips move in the armature's space (see the rest pose above).
    let hips = channel(i, "wave", "hips", ANIM_PROPERTY_TRANSLATION);
    assert!(close(key_at(hips, 0.0), &[0.0, 0.0, 0.0], 1e-4));
    let lifted = qrot(rig_rotation, key_at(hips, 1.0));
    assert!(close(&lifted, &[0.0, 0.25, 0.0], 1e-3), "{lifted:?}");
    let bob = channel(i, "bob", "Prop", ANIM_PROPERTY_TRANSLATION);
    assert!(close(
        &qrot(rig_rotation, key_at(bob, 0.0)),
        &[1.0, 0.0, 0.0],
        1e-4
    ));
    let lifted = qrot(rig_rotation, key_at(bob, 1.0));
    assert!(close(&lifted, &[1.0, 0.3, 0.0], 1e-3), "{lifted:?}");
    assert!(close(
        &qrot(rig_rotation, key_at(bob, 2.0)),
        &[1.0, 0.0, 0.0],
        1e-3
    ));

    // Sub-assets and derived ids.
    let subs: Vec<&str> = i.sub_assets.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        subs,
        [
            "mesh/Body",
            "mesh/Prop",
            "skin/Rig",
            "anim/bob",
            "anim/wave",
            "prefab"
        ]
    );
    assert_eq!(i.sub_assets[2].1, derive_sub_id(meta.id(), "skin/Rig"));

    // The prefab: the same entities, `fynkel.Skin` next to the Body's
    // renderer naming the skin and the hips, none on the Prop; it validates.
    let p = i.prefab.as_ref().unwrap();
    let entities = p["entities"].as_object().unwrap();
    // Blender's export puts Body next to the armature, so the two
    // top-level nodes get a root named after the file (ADR-0022).
    assert_eq!(entities.len(), 7);
    assert_eq!(entities[p["root"].as_str().unwrap()]["name"], "rig");
    let mut names: Vec<&str> = entities
        .values()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["Body", "Prop", "Rig", "head", "hips", "rig", "spine"]
    );
    let body = entities.values().find(|e| e["name"] == "Body").unwrap();
    let components: Vec<&serde_json::Value> =
        body["components"].as_object().unwrap().values().collect();
    assert_eq!(components.len(), 4);
    assert_eq!(components[1]["type"], "fynkel.MeshRenderer");
    assert_eq!(components[1]["data"]["mesh"]["sub"], "mesh/Body");
    assert_eq!(components[2]["type"], "fynkel.Skin");
    assert_eq!(
        components[2]["data"]["skin"]["asset"],
        meta.id().to_string()
    );
    assert_eq!(components[2]["data"]["skin"]["sub"], "skin/Rig");
    // The skeleton root is a reference to the hips' entity (ADR-0035).
    let hips = entities
        .iter()
        .find(|(_, e)| e["name"] == "hips")
        .unwrap()
        .0;
    assert_eq!(
        components[2]["data"]["skeleton"],
        serde_json::json!({ "entity": hips })
    );
    let prop = entities.values().find(|e| e["name"] == "Prop").unwrap();
    assert_eq!(prop["components"].as_object().unwrap().len(), 3);
    let spine = entities.values().find(|e| e["name"] == "spine").unwrap();
    let t = &spine["components"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap()["data"];
    assert!(
        (t["position"][1].as_f64().unwrap() - 0.5).abs() < 1e-6,
        "{t}"
    );
    let schemas = SchemaSet::embedded();
    let text = serde_json::to_string_pretty(p).unwrap();
    let doc = Document::parse(DocumentKind::Prefab, Some("prefab"), &text)
        .unwrap_or_else(|d| panic!("{d:#?}"));
    let diags = Prefab::from_document(doc).validate(&schemas);
    assert!(!has_errors(&diags), "{diags:#?}");

    // Deterministic bytes that round-trip.
    let again = import(&fixture("rig.fbx"), &meta, &Limits::default())
        .intermediate
        .unwrap();
    assert_eq!(&again, i);
    let back = Intermediate::from_bytes(&i.to_bytes()).unwrap();
    assert_eq!(&back, i);
}

#[test]
fn robot_fbx_imports_meshes_materials_and_the_embedded_texture() {
    let (r, meta) = import_fixture("robot.fbx");
    assert!(!r.has_errors(), "{:#?}", r.diagnostics);
    let i = r.intermediate.as_ref().unwrap();
    let (g, _) = import_fixture("robot.glb");
    let g = g.intermediate.unwrap();

    // Meshes: the two-material mesh becomes one part per material slot
    // (Blender gives the quad a default material), the cube its own.
    let subs: Vec<&str> = i.meshes.iter().map(|m| m.sub_id.as_str()).collect();
    assert_eq!(
        subs,
        [
            "mesh/PyramidAndQuad/0",
            "mesh/PyramidAndQuad/1",
            "mesh/Cube"
        ]
    );
    let cube = i.meshes.iter().find(|m| m.name == "Cube").unwrap();
    let gcube = g.meshes.iter().find(|m| m.name == "Cube").unwrap();
    assert_eq!(cube.vertices.len(), 24, "welded back to the glTF's 24");
    assert_eq!(cube.triangle_count(), 12);
    assert_eq!(cube.bounds_min, gcube.bounds_min);
    assert_eq!(cube.bounds_max, gcube.bounds_max);
    // Per corner (position + normal) the same UVs: the V flip lands on
    // the glTF convention.
    for v in &cube.vertices {
        let gv = gcube
            .vertices
            .iter()
            .find(|x| close(&x.position, &v.position, 1e-5) && close(&x.normal, &v.normal, 1e-4))
            .unwrap_or_else(|| panic!("no glTF corner at {:?}/{:?}", v.position, v.normal));
        assert!(
            close(&gv.uv0, &v.uv0, 1e-5),
            "uv {:?} vs {:?} at {:?}",
            v.uv0,
            gv.uv0,
            v.position
        );
    }
    let materials: Vec<(&str, &str)> = i
        .materials
        .iter()
        .map(|m| (m.sub_id.as_str(), m.name.as_str()))
        .collect();
    assert_eq!(
        materials,
        [
            ("material/Glass", "Glass"),
            ("material/DefaultMaterial", "DefaultMaterial"),
            ("material/Painted", "Painted")
        ]
    );
    assert_eq!(cube.material.as_deref(), Some("material/Painted"));
    let pyramid = &i.meshes[0];
    assert_eq!(pyramid.name, "PyramidAndQuad[0]");
    assert_eq!(pyramid.triangle_count(), 4);
    assert_eq!(pyramid.material.as_deref(), Some("material/Glass"));
    let quad = &i.meshes[1];
    assert_eq!(quad.name, "PyramidAndQuad[1]");
    assert_eq!(quad.triangle_count(), 2);
    assert_eq!(
        quad.vertices.len(),
        4,
        "the quad's corners share their colour"
    );
    let gquad = g
        .meshes
        .iter()
        .find(|m| m.sub_id == "mesh/PyramidAndQuad/1")
        .unwrap();
    for v in &quad.vertices {
        let gv = gquad
            .vertices
            .iter()
            .find(|x| close(&x.position, &v.position, 1e-5))
            .unwrap();
        assert!(
            close(&gv.color, &v.color, 1e-3),
            "colour {:?} vs {:?}",
            v.color,
            gv.color
        );
    }

    // Materials: Blender's exporter keeps the factors it has FBX properties
    // for and binds the image where it can (the normal map); the documents
    // validate against the PBR manifest.
    let glass = &i.materials[0].document;
    assert_eq!(glass["shader"], "fynkel.pbr@1");
    assert_eq!(
        glass["id"],
        derive_sub_id(meta.id(), "material/Glass").to_string()
    );
    assert_eq!(glass["base_color"][3], 1.0);
    assert_eq!(glass["metallic"], 0.0);
    assert!(glass["roughness"].as_f64().unwrap() < 0.1);
    assert_eq!(glass["emissive"], serde_json::json!([0.0, 0.0, 0.0]));
    let painted = &i.materials[2].document;
    assert_eq!(painted["normal_texture"]["asset"], meta.id().to_string());
    assert_eq!(painted["normal_texture"]["sub"], "image/paint");
    assert!(painted.get("base_color_texture").is_none());
    let schemas = SchemaSet::embedded();
    let shaders = ShaderLibrary::builtin();
    for m in &i.materials {
        let text = serde_json::to_string_pretty(&m.document).unwrap();
        let doc = Document::parse(DocumentKind::Material, Some(&m.sub_id), &text)
            .unwrap_or_else(|d| panic!("{d:#?}"));
        let diags = Material::from_document(doc).validate(&schemas, &shaders);
        assert!(!has_errors(&diags), "{}: {diags:#?}", m.sub_id);
    }

    // The embedded image: the same pixels as the glTF's, bound as data
    // only, so it is linear.
    assert_eq!(i.images.len(), 1);
    let image = &i.images[0];
    assert_eq!(image.sub_id, "image/paint");
    assert_eq!(image.name, "paint");
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(image.color_space, ColorSpace::Linear);
    let mine = image::load_from_memory(&image.bytes).unwrap().into_rgba8();
    let theirs = image::load_from_memory(&g.images[0].bytes)
        .unwrap()
        .into_rgba8();
    assert_eq!(mine.dimensions(), theirs.dimensions());
    assert_eq!(mine.as_raw(), theirs.as_raw());

    // The prefab mirrors the glTF's hierarchy (robot > Ground, Robot >
    // Antenna, Body, Head: two top-level nodes get a root named after the
    // file) with the two-part mesh as two children.
    let p = i.prefab.as_ref().unwrap();
    let entities = p["entities"].as_object().unwrap();
    let names: Vec<&str> = entities
        .values()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "robot",
            "Ground",
            "PyramidAndQuad[0]",
            "PyramidAndQuad[1]",
            "Robot",
            "Antenna",
            "PyramidAndQuad[0]",
            "PyramidAndQuad[1]",
            "Body",
            "Head"
        ]
    );
    let root = p["root"].as_str().unwrap();
    assert_eq!(p["entities"][root]["children"].as_array().unwrap().len(), 2);

    // Warnings only: the texture's clamped wrap is not kept.
    assert_eq!(codes(&r), [Code::IMPORT_SKIPPED]);
    assert!(r.diagnostics[0].message.contains("clamped"));
    let subs: Vec<&str> = i.sub_assets.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        subs,
        [
            "mesh/PyramidAndQuad/0",
            "mesh/PyramidAndQuad/1",
            "mesh/Cube",
            "material/Glass",
            "material/DefaultMaterial",
            "material/Painted",
            "image/paint",
            "prefab"
        ]
    );
}

#[test]
fn external_textures_are_refused_unless_the_sidecar_allows_them() {
    let path = fixture("robot_external.fbx");
    let denied = import(&path, &Meta::new(&path), &Limits::default());
    assert!(denied.intermediate.is_none());
    let errors: Vec<&fynkel_contracts::Diagnostic> = denied
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, Code::IMPORT_EXTERNAL_URI);
    assert!(
        errors[0].message.contains("paint.png"),
        "{}",
        errors[0].message
    );
    assert!(errors[0]
        .suggestion
        .as_deref()
        .unwrap()
        .contains("allowExternalFiles"));

    let mut meta = Meta::new(&path);
    meta.settings =
        fynkel_document::json::Node::from_serde(&serde_json::json!({ "allowExternalFiles": true }));
    let allowed = import(&path, &meta, &Limits::default());
    assert!(!allowed.has_errors(), "{:#?}", allowed.diagnostics);
    let i = allowed.intermediate.unwrap();
    assert_eq!(i.images.len(), 1);
    assert!(i.images[0].bytes.starts_with(b"\x89PNG"));
    assert_eq!(i.images[0].name, "paint");

    // The cook keys the cache on the files the FBX may read: the
    // external texture here, nothing for the embedded export.
    assert_eq!(
        fynkel_import::fbx::external_files(&std::fs::read(&path).unwrap()),
        ["paint.png"]
    );
    assert!(
        fynkel_import::fbx::external_files(&std::fs::read(fixture("robot.fbx")).unwrap())
            .is_empty()
    );
    assert!(fynkel_import::fbx::external_files(b"not fbx").is_empty());
}

#[test]
fn unreadable_files_and_limits() {
    let not_fbx = fixture("rig.glb");
    let mut meta = Meta::new(&not_fbx);
    meta.importer = "fynkel.fbx".to_string();
    let r = import(&not_fbx, &meta, &Limits::default());
    assert_eq!(codes(&r), [Code::IMPORT_UNREADABLE]);
    assert!(r.diagnostics[0].message.starts_with("not an FBX file"));

    let path = fixture("rig.fbx");
    let meta = meta_for(&path);
    let two = Limits {
        max_joints: 2,
        ..Limits::default()
    };
    assert_eq!(codes(&import(&path, &meta, &two)), [Code::IMPORT_LIMIT]);
    let few = Limits {
        max_vertices: 8,
        ..Limits::default()
    };
    assert_eq!(codes(&import(&path, &meta, &few)), [Code::IMPORT_LIMIT]);
    let shallow = Limits {
        max_node_depth: 2,
        ..Limits::default()
    };
    assert_eq!(codes(&import(&path, &meta, &shallow)), [Code::IMPORT_LIMIT]);
}
