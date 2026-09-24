//! The edit-preview world over a copy of the fixture project (blueprint
//! §19.2e item 6): every fixture scene loads; after every transaction
//! kind the incrementally synced world dumps exactly like a fresh load
//! (the incremental sync rule); a ray through each entity's centre picks
//! it; the marquee; the outlines; 60 s of steps move nothing; the
//! transform preview; hidden entities; and the edit-preview world's
//! picture equals a plain game world's from the same camera, pixel for
//! pixel, when a GPU is there.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fynkel_assets::Assets;
use fynkel_authoring::preview::{ICON_RADIUS, UNSELECTED_CAMERA_ALPHA};
use fynkel_authoring::{AuthoringSession, Handle, Preview, Ray, View};
use fynkel_core::components::{Camera, Projection};
use fynkel_core::{SceneDescription, World, WorldConfig};
use fynkel_scene::Transform;
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use serde_json::{json, Value};
use uuid::Uuid;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fynkel_document/fixtures/project")
        .canonicalize()
        .unwrap()
}

struct Copy {
    dir: PathBuf,
}

impl Copy {
    fn new(name: &str) -> Copy {
        let dir =
            std::env::temp_dir().join(format!("fynkel-preview-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixture(), &dir);
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

/// A session over a cooked copy with its asset store.
fn open(copy: &Copy) -> (AuthoringSession, Assets) {
    let mut s = AuthoringSession::new();
    s.open_project(&copy.dir)
        .map_err(|d| format!("{d:?}"))
        .unwrap();
    let report = s.project().unwrap().cook();
    assert!(!report.has_errors(), "{:?}", report.diagnostics);
    let assets = Assets::open(&s.project().unwrap().cache_dir()).unwrap();
    (s, assets)
}

fn edit_world() -> World {
    World::new(WorldConfig {
        edit_mode: true,
        ..WorldConfig::default()
    })
}

fn describe(s: &AuthoringSession, h: Handle) -> SceneDescription {
    let d = s.describe(h).unwrap();
    assert!(!d.has_errors(), "{:?}", d.diagnostics);
    d.description
}

/// The dump of a world loaded fresh from `described`.
fn fresh_dump(assets: &Assets, document: Uuid, described: &SceneDescription) -> Value {
    let mut w = edit_world();
    let p = Preview::load(document, &mut w, Some(assets), described);
    assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
    p.dump(&w)
}

fn apply(s: &mut AuthoringSession, h: Handle, commands: Value) -> Value {
    let applied = s.apply_json(h, &commands).unwrap();
    assert_eq!(applied["ok"], true, "{applied}");
    applied
}

const SCENES: [&str; 5] = [
    "assets/scenes/main.scene.json",
    "assets/scenes/playground.scene.json",
    "assets/scenes/catalogue.scene.json",
    "assets/scenes/results.scene.json",
    "assets/scenes/rig.scene.json",
];

const MAIN: &str = SCENES[0];
const CAMERA: &str = "c0000000-0000-4000-8000-0000000000e0";
const ROBOT: &str = "c0000000-0000-4000-8000-0000000000e1";
const CRATE_A: &str = "c0000000-0000-4000-8000-0000000000e2";
const SUN: &str = "c0000000-0000-4000-8000-0000000000e3";
const FLOOR: &str = "c0000000-0000-4000-8000-0000000000e4";
const CRATE_B: &str = "c0000000-0000-4000-8000-0000000000e5";
const ROBOT_MESH: &str = "c0000000-0000-4000-8000-0000000000c1";
const FLOOR_TRANSFORM: &str = "c0000000-0000-4000-8000-0000000000d0";
const CRATE_PREFAB: &str = "c0000000-0000-4000-8000-000000000040";
const CRATE_PREFAB_ENTITY: &str = "c0000000-0000-4000-8000-0000000000b1";
const CRATE_PREFAB_MESH: &str = "c0000000-0000-4000-8000-0000000000c3";

#[test]
fn every_fixture_scene_loads_into_the_edit_world() {
    let copy = Copy::new("load");
    let (mut s, assets) = open(&copy);
    for scene in SCENES {
        let h = s.open_document(scene).unwrap();
        let described = describe(&s, h);
        let mut w = edit_world();
        let p = Preview::load(
            s.document(h).unwrap().editor.document().envelope.id,
            &mut w,
            Some(&assets),
            &described,
        );
        assert!(p.diagnostics.is_empty(), "{scene}: {:?}", p.diagnostics);
        assert_eq!(p.len(), described.entities.len(), "{scene}");
        assert_eq!(p.loads, 1);
        // Every described entity is live, mapped both ways, and named.
        for e in &described.entities {
            let id = Uuid::from_bytes(*e.id.as_bytes());
            let entity = p
                .entity(id)
                .unwrap_or_else(|| panic!("{scene}: {} unmapped", e.name));
            assert_eq!(p.id_of(entity), Some(id));
            assert!(w.ecs().contains(entity));
        }
        let dump = p.dump(&w);
        assert_eq!(dump["entityCount"], described.entities.len());
        // Meshes reach the extract (the picture draws them).
        let extract = w.extract();
        let meshes = described
            .entities
            .iter()
            .filter(|e| {
                e.components
                    .iter()
                    .any(|c| matches!(c, fynkel_core::components::ComponentValue::MeshRenderer(m) if m.visible))
            })
            .count();
        assert_eq!(extract.meshes.len(), meshes, "{scene}");
        assert!(
            extract.camera.is_some(),
            "{scene}: the scene camera is reported"
        );
        s.close_document(h).unwrap();
    }
}

#[test]
fn incremental_sync_matches_a_fresh_load_after_every_transaction_kind() {
    let copy = Copy::new("sync");
    let (mut s, assets) = open(&copy);
    let h = s.open_document(MAIN).unwrap();
    let document = s.document(h).unwrap().editor.document().envelope.id;
    let mut w = edit_world();
    let mut p = Preview::load(document, &mut w, Some(&assets), &describe(&s, h));
    assert_eq!(p.dump(&w), fresh_dump(&assets, document, &describe(&s, h)));

    // Every transaction is followed by one sync; the synced world must
    // dump exactly like a world loaded fresh from the document now, and
    // the incremental path must hold (no fallback reload).
    let check = |s: &mut AuthoringSession, w: &mut World, p: &mut Preview, what: &str| {
        let described = describe(s, h);
        let report = p.sync(w, Some(&assets), &described);
        assert!(
            !report.reloaded,
            "{what}: fell back to a reload: {report:?}"
        );
        assert!(
            report.diagnostics.is_empty(),
            "{what}: {:?}",
            report.diagnostics
        );
        assert_eq!(
            p.dump(w),
            fresh_dump(&assets, document, &described),
            "{what}"
        );
        report
    };

    // setField on a transform: one SetTransform, one step.
    apply(
        &mut s,
        h,
        json!([{ "command": "setField", "entity": FLOOR, "component": FLOOR_TRANSFORM, "field": "position", "value": [0, -1.5, 0] }]),
    );
    let r = check(&mut s, &mut w, &mut p, "setField transform");
    assert_eq!((r.updated, r.spawned, r.destroyed, r.steps), (1, 0, 0, 1));

    // setField on another component: one SetComponent.
    apply(
        &mut s,
        h,
        json!([{ "command": "setField", "entity": ROBOT, "component": ROBOT_MESH, "field": "cast_shadows", "value": false }]),
    );
    let r = check(&mut s, &mut w, &mut p, "setField component");
    assert_eq!((r.updated, r.steps), (1, 1));

    // createObject: a root and a child under it in one transaction — the
    // child waits a round for its parent's handle.
    let created = apply(
        &mut s,
        h,
        json!([
            { "command": "createObject", "name": "Lamp post", "components": [
                { "type": "fynkel.Transform", "data": { "position": [3, 0, -2], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] } }
            ] },
        ]),
    );
    let post = created["mutations"][0]["affectedIds"][0]
        .as_str()
        .unwrap()
        .to_string();
    apply(
        &mut s,
        h,
        json!([
            { "command": "createObject", "name": "Bulb", "parent": post, "components": [
                { "type": "fynkel.Transform", "data": { "position": [0, 2.5, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] } },
                { "type": "fynkel.PointLight", "data": { "color": [1, 0.9, 0.7], "intensity": 800, "range": 6, "culling_mask": 4294967295u32 } }
            ] },
            { "command": "createObject", "name": "Shade", "parent": post },
        ]),
    );
    let r = check(&mut s, &mut w, &mut p, "createObject");
    assert_eq!(
        r.spawned, 3,
        "the post from the first transaction, then its two children"
    );
    assert!(
        r.steps >= 2,
        "the light is set after its entity exists: {r:?}"
    );
    let bulb_id = describe(&s, h)
        .entities
        .iter()
        .find(|e| e.name == "Bulb")
        .map(|e| Uuid::from_bytes(*e.id.as_bytes()))
        .unwrap();
    assert!(w
        .ecs()
        .has::<fynkel_core::components::PointLight>(p.entity(bulb_id).unwrap()));

    // renameObject.
    apply(
        &mut s,
        h,
        json!([{ "command": "renameObject", "id": ROBOT, "name": "Bot" }]),
    );
    let r = check(&mut s, &mut w, &mut p, "renameObject");
    assert_eq!((r.updated, r.steps), (1, 1));

    // setObjectActive: one SetActive; the post's subtree goes with it and
    // is never picked.
    let post_id = Uuid::parse_str(&post).unwrap();
    apply(
        &mut s,
        h,
        json!([{ "command": "setObjectActive", "id": post, "active": false }]),
    );
    let r = check(&mut s, &mut w, &mut p, "setObjectActive");
    assert_eq!((r.updated, r.steps), (1, 1));
    let bulb = p.entity(bulb_id).unwrap();
    assert!(!fynkel_scene::is_active_in_hierarchy(w.ecs(), bulb));
    assert!(fynkel_scene::is_active_self(w.ecs(), bulb));
    let at = p
        .world_matrix(&w, post_id)
        .unwrap()
        .transform_point3(Vec3::ZERO);
    let ray = Ray {
        origin: at + Vec3::Z * 10.0,
        dir: Vec3::NEG_Z,
    };
    assert!(p.pick(&w, None, &ray).iter().all(|h| h.id != post_id));
    apply(
        &mut s,
        h,
        json!([{ "command": "setObjectActive", "id": post, "active": true }]),
    );
    check(&mut s, &mut w, &mut p, "setObjectActive true");
    assert!(fynkel_scene::is_active_in_hierarchy(w.ecs(), bulb));
    assert!(p.pick(&w, None, &ray).iter().any(|h| h.id == post_id));

    // reparentObject with an index: the Robot under the Floor, first.
    apply(
        &mut s,
        h,
        json!([{ "command": "reparentObject", "id": ROBOT, "parent": FLOOR, "index": 0 }]),
    );
    let r = check(&mut s, &mut w, &mut p, "reparentObject");
    assert_eq!(r.updated, 1);
    // A second child, then the order flipped by reparenting to index 0
    // again: no command, only the sibling order.
    apply(
        &mut s,
        h,
        json!([{ "command": "reparentObject", "id": CRATE_B, "parent": FLOOR, "index": 0 }]),
    );
    check(&mut s, &mut w, &mut p, "reparentObject index");
    apply(
        &mut s,
        h,
        json!([{ "command": "reparentObject", "id": ROBOT, "parent": FLOOR, "index": 0 }]),
    );
    let r = check(&mut s, &mut w, &mut p, "sibling order");
    assert_eq!(r.updated, 0);
    assert!(r.reordered > 0);

    // addComponent / removeComponent.
    let added = apply(
        &mut s,
        h,
        json!([{ "command": "addComponent", "entity": FLOOR, "component": { "type": "fynkel.PointLight", "data": { "color": [1, 1, 1], "intensity": 100, "range": 3, "culling_mask": 4294967295u32 } } }]),
    );
    let light_component = added["mutations"][0]["affectedIds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .find(|id| id != FLOOR)
        .unwrap();
    let r = check(&mut s, &mut w, &mut p, "addComponent");
    assert_eq!((r.updated, r.steps), (1, 1));
    apply(
        &mut s,
        h,
        json!([{ "command": "removeComponent", "entity": FLOOR, "component": light_component }]),
    );
    let r = check(&mut s, &mut w, &mut p, "removeComponent");
    assert_eq!((r.updated, r.steps), (1, 1));

    // duplicateObject: the Floor with its two children.
    apply(
        &mut s,
        h,
        json!([{ "command": "duplicateObject", "id": FLOOR, "name": "Floor 2" }]),
    );
    let r = check(&mut s, &mut w, &mut p, "duplicateObject");
    assert_eq!(r.spawned, 3);

    // instantiatePrefab and a prefab override on an existing instance.
    apply(
        &mut s,
        h,
        json!([{ "command": "instantiatePrefab", "name": "Crate C", "prefab": { "asset": CRATE_PREFAB }, "components": [
            { "type": "fynkel.Transform", "data": { "position": [-4, 0, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] } }
        ] }]),
    );
    let r = check(&mut s, &mut w, &mut p, "instantiatePrefab");
    assert_eq!(r.spawned, 1, "the instance is the prefab's one entity");
    apply(
        &mut s,
        h,
        json!([{ "command": "setPrefabOverride", "entity": CRATE_A, "override": {
            "op": "setField", "entity": CRATE_PREFAB_ENTITY, "component": CRATE_PREFAB_MESH, "field": "visible", "value": false
        } }]),
    );
    let r = check(&mut s, &mut w, &mut p, "setPrefabOverride");
    assert_eq!((r.updated, r.spawned), (1, 0));
    apply(
        &mut s,
        h,
        json!([{ "command": "revertPrefabOverride", "entity": CRATE_A }]),
    );
    let r = check(&mut s, &mut w, &mut p, "revertPrefabOverride");
    assert_eq!(
        r.updated, 2,
        "cast_shadows back to the prefab's and visible back on, and Crate A back at the prefab's position: two SetComponents"
    );

    // deleteObject: a parent with children goes as one destroy.
    apply(
        &mut s,
        h,
        json!([{ "command": "deleteObject", "id": FLOOR }]),
    );
    let r = check(&mut s, &mut w, &mut p, "deleteObject");
    assert_eq!((r.destroyed, r.steps), (1, 1));
    assert!(
        p.entity(Uuid::parse_str(ROBOT).unwrap()).is_none(),
        "the child went with it"
    );

    // setDocumentField (the scene's name) changes no entity: nothing to do.
    apply(
        &mut s,
        h,
        json!([{ "command": "setDocumentField", "pointer": "/name", "value": "Main 2" }]),
    );
    let r = check(&mut s, &mut w, &mut p, "setDocumentField");
    assert_eq!(r.steps, 0);

    // Undo all the way back, then redo all the way forward — every step
    // through the same diff.
    let history = s.state_json(h).unwrap()["history"].as_u64().unwrap();
    for i in 0..history {
        s.undo(h).unwrap();
        check(&mut s, &mut w, &mut p, &format!("undo {i}"));
    }
    assert_eq!(
        p.dump(&w)["entityCount"],
        6,
        "back to the fixture's six (Crate A is the crate prefab's root)"
    );
    for i in 0..history {
        s.redo(h).unwrap();
        check(&mut s, &mut w, &mut p, &format!("redo {i}"));
    }
    assert_eq!(p.syncs as u64, 17 + 2 * history);
    assert_eq!(p.loads, 1, "never a fallback reload");
    let _ = (CAMERA, SUN);
}

#[test]
fn a_prefab_open_in_another_tab_previews_live_and_a_material_is_refused() {
    let copy = Copy::new("prefab-live");
    let (mut s, assets) = open(&copy);
    let scene = s.open_document(MAIN).unwrap();
    let prefab = s.open_document("assets/prefabs/crate.prefab.json").unwrap();
    let document = s.document(scene).unwrap().editor.document().envelope.id;
    let mut w = edit_world();
    let mut p = Preview::load(document, &mut w, Some(&assets), &describe(&s, scene));
    // Edit the prefab (unsaved): scale its root and give it a Lid, a
    // child with the root's mesh. The scene's instances follow on the next
    // sync, and the synced world equals a fresh load.
    let root_components = s.document_json(prefab).unwrap()["root"]["entities"][CRATE_PREFAB_ENTITY]
        ["components"]
        .clone();
    let component_of = |ty: &str| {
        root_components
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, c)| c["type"] == ty)
            .map(|(id, c)| (id.clone(), c["data"].clone()))
            .unwrap()
    };
    let (transform, _) = component_of("fynkel.Transform");
    let (_, renderer) = component_of("fynkel.MeshRenderer");
    apply(
        &mut s,
        prefab,
        json!([
            { "command": "setField", "entity": CRATE_PREFAB_ENTITY, "component": transform, "field": "scale", "value": [2, 2, 2] },
            { "command": "createObject", "name": "Lid", "parent": CRATE_PREFAB_ENTITY, "components": [
                { "type": "fynkel.Transform", "data": { "position": [0, 2, 0], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] } },
                { "type": "fynkel.MeshRenderer", "data": renderer }
            ] }
        ]),
    );
    let described = describe(&s, scene);
    let report = p.sync(&mut w, Some(&assets), &described);
    assert!(!report.reloaded, "{report:?}");
    assert_eq!(p.dump(&w), fresh_dump(&assets, document, &described));
    // Crate A is the prefab's root: scaled itself, the Lid under it.
    let crate_a = Uuid::parse_str(CRATE_A).unwrap();
    let subtree = p.subtree(crate_a);
    assert_eq!(subtree.len(), 2);
    assert_eq!(subtree[0], crate_a);
    let lid = subtree[1];
    let scale = w
        .ecs()
        .copy::<Transform>(p.entity(crate_a).unwrap())
        .unwrap()
        .scale;
    assert_eq!(scale, Vec3::splat(2.0));
    // The anchor map: the Scene view works in document entities, so a
    // pick, a marquee, an outline and a hidden flag inside the instance
    // all speak of Crate A.
    let (_, anchors) = s.describe_with_anchors(scene).unwrap();
    assert_eq!(anchors.get(&lid), Some(&crate_a));
    p.set_anchors(anchors);
    let inner = p
        .world_matrix(&w, lid)
        .unwrap()
        .transform_point3(Vec3::ZERO);
    let ray = Ray {
        origin: inner + Vec3::Y * 10.0,
        dir: Vec3::NEG_Y,
    };
    let hits = p.pick(&w, Some(&assets), &ray);
    assert!(
        hits.iter().any(|h| h.id == crate_a),
        "{:?}",
        hits.iter().map(|h| h.id).collect::<Vec<_>>()
    );
    assert!(
        !hits.iter().any(|h| h.id == lid),
        "the inner entity answers as its instance"
    );
    let outline_instance = p.outline_lines(&w, Some(&assets), &[crate_a], Vec4::ONE);
    let outline_inner = p.outline_lines(&w, Some(&assets), &[lid], Vec4::ONE);
    assert!(
        outline_instance.len() > outline_inner.len(),
        "the root's box and the Lid's"
    );
    assert!(outline_inner.iter().all(|l| outline_instance.contains(l)));
    p.set_hidden(&mut w, &[crate_a]);
    assert_eq!(
        w.edit_hidden().count(),
        2,
        "the Lid hides with its instance"
    );
    assert!(p
        .pick(&w, Some(&assets), &ray)
        .iter()
        .all(|h| h.id != crate_a));
    p.set_hidden(&mut w, &[]);
    p.set_anchors(Default::default());
    // The prefab document itself previews as a scene.
    let pd = s.describe(prefab).unwrap();
    assert_eq!(pd.description.entities.len(), 2);
    // A material has nothing to preview.
    let material = s
        .open_document("assets/materials/rusted.material.json")
        .unwrap();
    let err = s.describe(material).unwrap_err();
    assert_eq!(err.code.0, "FYN-T0003");
}

/// The fixture's cameras all look at their scene from +Z-ish; the test
/// camera sits where the scene camera is.
fn scene_camera(described: &SceneDescription, w: &World, p: &Preview) -> (Mat4, Camera) {
    let cam = described
        .entities
        .iter()
        .find(|e| {
            e.components
                .iter()
                .any(|c| matches!(c, fynkel_core::components::ComponentValue::Camera(_)))
        })
        .unwrap();
    let camera = cam
        .components
        .iter()
        .find_map(|c| match c {
            fynkel_core::components::ComponentValue::Camera(c) => Some(*c),
            _ => None,
        })
        .unwrap();
    let m = p
        .world_matrix(w, Uuid::from_bytes(*cam.id.as_bytes()))
        .unwrap();
    (m, camera)
}

#[test]
fn a_ray_through_every_entitys_centre_picks_it() {
    let copy = Copy::new("pick");
    let (mut s, assets) = open(&copy);
    for scene in SCENES {
        let h = s.open_document(scene).unwrap();
        let described = describe(&s, h);
        let document = s.document(h).unwrap().editor.document().envelope.id;
        let mut w = edit_world();
        let mut p = Preview::load(document, &mut w, Some(&assets), &described);
        // Through the scene's own camera — the picture the user sees; the
        // fixture's rig body is an open column, so a ray from straight
        // above would pass through it without touching a triangle, as a
        // click on the picture never does.
        let (camera_world, camera) = scene_camera(&described, &w, &p);
        let view = View {
            world: camera_world,
            camera,
            aspect: 16.0 / 9.0,
        };
        for e in &described.entities {
            let id = Uuid::from_bytes(*e.id.as_bytes());
            let m = p.world_matrix(&w, id).unwrap();
            let centre = match mesh_bounds(&assets, e) {
                Some((min, max)) => m.transform_point3((min + max) * 0.5),
                None => m.transform_point3(Vec3::ZERO),
            };
            // Aim at the centre through the camera's own projection (the
            // pixel → ray path the Scene view uses); a centre outside the
            // frame still gives a ray.
            let ray = if let Some(ndc) = view.project(centre) {
                view.ray(ndc)
            } else {
                // The scene camera itself (or something beside it): no
                // pixel looks at it from where it stands — a ray from
                // above, as an orbit would give.
                Ray {
                    origin: centre + Vec3::Y * 10.0,
                    dir: Vec3::NEG_Y,
                }
            };
            let hits = p.pick(&w, Some(&assets), &ray);
            assert!(
                hits.iter().any(|hit| hit.id == id),
                "{scene}: {} ({id}) not among {:?}",
                e.name,
                hits.iter().map(|h| h.id).collect::<Vec<_>>()
            );
            let hit = hits.iter().find(|hit| hit.id == id).unwrap();
            assert_eq!(
                hit.mesh,
                mesh_bounds(&assets, e).is_some(),
                "{scene}: {}",
                e.name
            );
            assert!((hit.point - centre).length() < 50.0 && hit.distance > 0.0);
            assert_eq!(hit.object, id, "no anchors: the object is the id");
            // Inside a prefab instance the hit answers with the anchor, and
            // still names the object the ray met.
            let anchor = Uuid::from_u128(0xa11c0);
            p.set_anchors(HashMap::from([(id, anchor)]));
            let hits = p.pick(&w, Some(&assets), &ray);
            let hit = hits.iter().find(|hit| hit.id == anchor).unwrap();
            assert_eq!(hit.object, id);
            p.set_anchors(HashMap::new());
        }
        // The stacked-hits rule the Scene view cycles through: the ray
        // through the floor's centre from above hits the floor and only
        // the things standing on that exact spot.
        let hits = p.pick(
            &w,
            Some(&assets),
            &Ray {
                origin: Vec3::new(0.0, 40.0, 0.0),
                dir: Vec3::NEG_Y,
            },
        );
        for pair in hits.windows(2) {
            assert!(
                pair[0].distance <= pair[1].distance,
                "{scene}: nearest first"
            );
        }
        // Hidden entities never hit.
        let all: Vec<Uuid> = p.ids().to_vec();
        p.set_hidden(&mut w, &all);
        assert!(p
            .pick(
                &w,
                Some(&assets),
                &Ray {
                    origin: Vec3::new(0.0, 40.0, 0.0),
                    dir: Vec3::NEG_Y,
                },
            )
            .is_empty());
        p.set_hidden(&mut w, &[]);
        s.close_document(h).unwrap();
    }
}

fn mesh_bounds(assets: &Assets, e: &fynkel_core::EntityDescription) -> Option<(Vec3, Vec3)> {
    e.components.iter().find_map(|c| match c {
        fynkel_core::components::ComponentValue::MeshRenderer(m)
            if m.visible && !m.mesh.asset.is_nil() =>
        {
            let mesh = assets.mesh(&m.mesh).ok()?;
            Some((Vec3::from(mesh.bounds_min), Vec3::from(mesh.bounds_max)))
        }
        _ => None,
    })
}

#[test]
fn the_marquee_the_outlines_and_the_icons() {
    let copy = Copy::new("overlays");
    let (mut s, assets) = open(&copy);
    let h = s.open_document(MAIN).unwrap();
    let described = describe(&s, h);
    let document = s.document(h).unwrap().editor.document().envelope.id;
    let mut w = edit_world();
    let p = Preview::load(document, &mut w, Some(&assets), &described);
    let view = View {
        world: Mat4::from_rotation_translation(
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            Vec3::new(0.0, 40.0, 0.0),
        ),
        camera: Camera {
            far: 200.0,
            ..Camera::default()
        },
        aspect: 16.0 / 9.0,
    };
    // The whole view: everything (the camera entity and the sun included:
    // their origins are inside).
    let all = p.pick_rect(
        &w,
        Some(&assets),
        &view,
        Vec2::splat(-1.0),
        Vec2::splat(1.0),
    );
    assert_eq!(all.len(), described.entities.len(), "{all:?}");
    // A rectangle around the Robot's centre alone (the floor under it
    // overlaps every rectangle — the marquee is an overlap test).
    let robot = Uuid::parse_str(ROBOT).unwrap();
    let ndc = view
        .project(
            p.world_matrix(&w, robot)
                .unwrap()
                .transform_point3(Vec3::new(0.0, 0.5, 0.0)),
        )
        .unwrap();
    let small = p.pick_rect(
        &w,
        Some(&assets),
        &view,
        ndc - Vec2::splat(0.01),
        ndc + Vec2::splat(0.01),
    );
    assert!(small.contains(&robot), "{small:?}");
    assert!(small.contains(&Uuid::parse_str(FLOOR).unwrap()));
    assert!(
        !small.contains(&Uuid::parse_str(CRATE_A).unwrap()),
        "{small:?}"
    );
    // An empty rectangle far off: nothing.
    assert!(p
        .pick_rect(
            &w,
            Some(&assets),
            &view,
            Vec2::new(0.95, 0.95),
            Vec2::new(0.99, 0.99)
        )
        .is_empty());

    // Outlines: a mesh's box is twelve edges, an empty's cross three.
    let sun = Uuid::parse_str(SUN).unwrap();
    let lines = p.outline_lines(&w, Some(&assets), &[robot, sun], Vec4::ONE);
    assert_eq!(lines.len(), 15);
    let sun_pos = p
        .world_matrix(&w, sun)
        .unwrap()
        .transform_point3(Vec3::ZERO);
    assert!(lines[12..]
        .iter()
        .all(|l| (l.a - sun_pos).length() <= ICON_RADIUS + 1e-4));
    assert_eq!(lines[0].json().as_array().unwrap().len(), 10);
    // Icons: the camera's frustum and the sun's arrows are there; hidden
    // entities are left out.
    let camera = Uuid::parse_str(CAMERA).unwrap();
    let icons = p.icon_lines(&w, Some(&assets), 16.0 / 9.0, &[]);
    assert!(icons.len() > 20, "{}", icons.len());
    // A camera that is not selected draws its frustum faint; selected, at
    // full alpha.
    let faint = icons
        .iter()
        .filter(|l| l.color.w == UNSELECTED_CAMERA_ALPHA)
        .count();
    assert!(faint > 0);
    let chosen = p.icon_lines(&w, Some(&assets), 16.0 / 9.0, &[camera]);
    assert_eq!(chosen.len(), icons.len());
    assert!(chosen.iter().all(|l| l.color.w != UNSELECTED_CAMERA_ALPHA));
    // Colliders only for the selection: the floor's box appears when the
    // floor is selected.
    let floor = Uuid::parse_str(FLOOR).unwrap();
    let with_floor = p.icon_lines(&w, Some(&assets), 16.0 / 9.0, &[floor]);
    assert_eq!(with_floor.len(), icons.len() + 12);
    p.set_hidden(&mut w, &[camera, sun]);
    let fewer = p.icon_lines(&w, Some(&assets), 16.0 / 9.0, &[]);
    assert!(fewer.len() < icons.len());
    let stats = p.stats(&w);
    assert_eq!(stats["hidden"], 2);
    assert_eq!(stats["entities"], described.entities.len());
}

#[test]
fn nothing_ticks_and_the_transform_preview_is_not_an_edit() {
    let copy = Copy::new("still");
    let (mut s, assets) = open(&copy);
    // The playground: a player with a character controller and a body, a
    // floor, a platform.
    let h = s.open_document(SCENES[1]).unwrap();
    let described = describe(&s, h);
    let document = s.document(h).unwrap().editor.document().envelope.id;
    let mut w = edit_world();
    let p = Preview::load(document, &mut w, Some(&assets), &described);
    let before = p.dump(&w);
    let steps = (60.0 / fynkel_contracts::DEFAULT_FIXED_DT).round() as u32;
    for _ in 0..steps {
        w.begin_frame(fynkel_contracts::DEFAULT_FIXED_DT as f32);
        w.step();
    }
    assert_eq!(p.dump(&w), before, "60 s of steps moved something");

    // The gizmo's preview moves the picture, not the document; restoring
    // brings the picture back; a sync afterwards has nothing to do.
    let player = Uuid::from_bytes(*described.entities[1].id.as_bytes());
    let moved = Transform::from_position(Vec3::new(5.0, 1.0, 5.0));
    p.set_transforms(&mut w, &[(player, moved)]).unwrap();
    assert_eq!(
        w.ecs()
            .copy::<Transform>(p.entity(player).unwrap())
            .unwrap(),
        moved
    );
    assert_eq!(s.state_json(h).unwrap()["dirty"], false);
    p.restore_transforms(&mut w, &[player]).unwrap();
    assert_eq!(p.dump(&w), before);
    let unknown = p
        .set_transforms(&mut w, &[(Uuid::nil(), moved)])
        .unwrap_err();
    assert_eq!(unknown.code.0, "FYN-T0003");
    // The editor camera replaces the scene camera in the extract.
    let (cam_world, cam) = scene_camera(&described, &w, &p);
    assert_ne!(w.extract().camera.unwrap().record, u32::MAX);
    w.set_edit_camera(Some((cam_world, cam)));
    assert_eq!(w.extract().camera.unwrap().record, u32::MAX);
    assert_eq!(w.extract().camera.unwrap().world, cam_world);
}

#[test]
fn the_edit_world_draws_the_same_pixels_as_a_game_world() {
    let copy = Copy::new("pixels");
    let (mut s, assets) = open(&copy);
    let Some((device, queue)) = fynkel_render::headless_device() else {
        eprintln!("no GPU adapter: pixel comparison skipped");
        return;
    };
    for scene in [SCENES[0], SCENES[1]] {
        let h = s.open_document(scene).unwrap();
        let described = describe(&s, h);
        let document = s.document(h).unwrap().editor.document().envelope.id;
        // The edit-preview world, looked at through the editor camera
        // placed exactly where the scene camera is (the Scene view's
        // "align to camera").
        let mut edit = edit_world();
        let p = Preview::load(document, &mut edit, Some(&assets), &described);
        let (cam_world, cam) = scene_camera(&described, &edit, &p);
        edit.set_edit_camera(Some((cam_world, cam)));
        // A plain game world of the same description, not stepped (the
        // fixture's playground has a body that would fall).
        let mut game = World::new(WorldConfig::default());
        game.load_scene(&described).unwrap();
        for e in &described.entities {
            for c in &e.components {
                if let fynkel_core::components::ComponentValue::Skin(k) = c {
                    assets.register_skin(&k.skin, &mut game).unwrap();
                }
            }
        }
        let render = |w: &World| {
            let mut r = fynkel_render::ExtractRenderer::new(
                &device,
                wgpu::TextureFormat::Rgba8Unorm,
                320,
                180,
            );
            r.set_assets(Some(assets.clone()));
            fynkel_render::render_offscreen(&device, &queue, &mut r, &w.extract())
        };
        let a = render(&edit);
        let b = render(&game);
        assert_eq!(a.len(), b.len());
        let differing = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        assert_eq!(
            differing,
            0,
            "{scene}: {differing} of {} pixels differ",
            a.len()
        );
        assert!(
            a.iter().any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0),
            "{scene}: something was drawn"
        );
        // And an orthographic editor camera renders too (the projection
        // path the game never uses).
        edit.set_edit_camera(Some((
            cam_world,
            Camera {
                projection: Projection::Orthographic,
                ortho_height: 12.0,
                ..cam
            },
        )));
        let c = render(&edit);
        assert!(
            c.iter().zip(&a).any(|(x, y)| x != y),
            "{scene}: the projection changed the picture"
        );
        s.close_document(h).unwrap();
    }
}
