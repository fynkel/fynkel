//! Physics materials through the store (ADR-0025): a cooked
//! `.physicsmaterial.json` reads back as physics' `PhysicsMaterial`, the
//! loader registers every collider's material before the first step, the
//! world bounces on it, and a reload after the document was edited gives
//! the live colliders the new values. A reference to anything else is the
//! store's diagnostic and the collider keeps its own values.

use std::path::{Path, PathBuf};

use fynkel_assets::Assets;
use fynkel_contracts::{AssetRef, Code, Uuid};
use fynkel_core::physics::{CombineRule, PhysicsMaterial};
use fynkel_core::{Transform, World, WorldConfig};
use fynkel_import::{cook_project, CookOptions};

const MAIN: &str = "c0000000-0000-4000-8000-000000000030";
const RUSTED: &str = "c0000000-0000-4000-8000-000000000020";
const CRATE_B: &str = "c0000000-0000-4000-8000-0000000000e5";
const FLOOR: &str = "c0000000-0000-4000-8000-0000000000e4";
const BOUNCY: &str = "c0000000-0000-4000-8000-000000000801";

fn id(s: &str) -> Uuid {
    s.parse().unwrap()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == ".fynkel" {
            continue;
        }
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the fixture project whose Crate B and Floor collide with
/// `bouncy.physicsmaterial.json` (removed on drop).
struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("fynkel-physmat-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../fynkel_document/fixtures/project"),
            &dir,
        );
        let f = Fixture(dir);
        f.material(r#","bounciness":1,"bounce_combine":"maximum""#);
        let path = f.0.join("assets/scenes/main.scene.json");
        let mut scene: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for (entity, material) in [(CRATE_B, BOUNCY), (FLOOR, BOUNCY)] {
            let components = scene["entities"][entity]["components"]
                .as_object_mut()
                .unwrap();
            let collider = components
                .values_mut()
                .find(|c| c["type"] == "fynkel.Collider")
                .unwrap();
            collider["data"]["material"] = serde_json::json!({ "asset": material });
        }
        std::fs::write(&path, serde_json::to_string_pretty(&scene).unwrap()).unwrap();
        f
    }

    fn material(&self, fields: &str) {
        std::fs::write(
            self.0.join("assets/bouncy.physicsmaterial.json"),
            format!(
                r#"{{"$schema":"https://fynkel.dev/schemas/physics_material.schema.json","formatVersion":1,"id":"{BOUNCY}","name":"Bouncy"{fields}}}"#
            ),
        )
        .unwrap();
    }

    fn cook(&self) -> Assets {
        let cache = self.0.join(".fynkel/cache");
        let report = cook_project(&self.0, Some(&cache), &CookOptions::default())
            .unwrap_or_else(|d| panic!("{d:#?}"));
        assert!(!report.has_errors(), "{}", report.text());
        Assets::open(&cache).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_cooked_physics_material_is_registered_bounces_and_reloads() {
    let fixture = Fixture::new("bounce");
    let assets = fixture.cook();
    let key = AssetRef::new(id(BOUNCY));
    let bouncy = PhysicsMaterial {
        friction: 0.6,
        bounciness: 1.0,
        friction_combine: CombineRule::Average,
        bounce_combine: CombineRule::Maximum,
    };
    assert_eq!(assets.physics_material(&key).unwrap(), bouncy);
    // A render material is not a physics material.
    let d = assets
        .physics_material(&AssetRef::new(id(RUSTED)))
        .unwrap_err();
    assert_eq!(d.code, Code::RUNTIME_CHUNK_MISSING);
    assert!(
        d.message.contains("not a physics material"),
        "{}",
        d.message
    );

    let scene = assets.scene(id(MAIN)).unwrap_or_else(|d| panic!("{d:#?}"));
    let mut w = World::new(WorldConfig::default());
    let (_, diags) = assets.register_mesh_colliders(&scene.description, w.physics_mut());
    assert_eq!(diags, vec![]);
    assert_eq!(w.physics().material(&key), Some(bouncy));
    let loaded = w.load_scene(&scene.description).unwrap();
    let crate_b = loaded.handle_of(id(CRATE_B)).unwrap();
    // Crate B falls from 3 m onto the floor (resting at 0.5): with
    // bounciness 1 it comes most of the way back up.
    let mut heights = Vec::new();
    for _ in 0..150 {
        w.step();
        heights.push(w.ecs().copy::<Transform>(crate_b).unwrap().position.y);
    }
    assert!(w.diagnostics().is_empty(), "{:?}", w.diagnostics());
    let (low, _) = heights
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))
        .unwrap();
    let rebound = heights[low..].iter().copied().fold(f32::MIN, f32::max) - 0.5;
    assert!(rebound > 1.0, "bounced back up {rebound} m");

    // The document edited and recooked, the scene reloaded: the live
    // colliders take the new values.
    fixture.material(r#","friction":0.2"#);
    let assets = fixture.cook();
    let scene = assets.scene(id(MAIN)).unwrap();
    let (_, diags) = assets.register_mesh_colliders(&scene.description, w.physics_mut());
    assert_eq!(diags, vec![]);
    assert_eq!(
        w.physics().material(&key),
        Some(PhysicsMaterial {
            friction: 0.2,
            ..PhysicsMaterial::default()
        })
    );
}
