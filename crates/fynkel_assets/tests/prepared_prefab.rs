use fynkel_assets::Assets;
use fynkel_contracts::{AssetRef, Uuid};
use fynkel_core::{World, WorldConfig};
use fynkel_import::{cook_project, CookOptions};
use serde_json::json;
use std::path::{Path, PathBuf};

const PREFAB: &str = "c0000000-0000-4000-8000-000000000040";
const ROOT: &str = "c0000000-0000-4000-8000-0000000000b1";
const ROBOT: &str = "c0000000-0000-4000-8000-000000000001";
const TONE: &str = "c0000000-0000-4000-8000-000000000006";
const RIG: &str = "c0000000-0000-4000-8000-000000000005";
fn id(s: &str) -> Uuid {
    s.parse().unwrap()
}
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let e = entry.unwrap();
        if e.file_name() == ".fynkel" {
            continue;
        }
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("fynkel-prepared-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("project");
        copy(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../fynkel_document/fixtures/project"),
            &project,
        );
        let file = project.join("assets/prefabs/crate.prefab.json");
        let mut doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        let components = doc["entities"][ROOT]["components"].as_object_mut().unwrap();
        for (n, ty, data) in [
            (
                1,
                "fynkel.Collider",
                json!({"shape":"mesh", "size":[0.5,0.5,0.5], "offset":[0,0,0], "friction":0.5, "restitution":0, "is_sensor":false, "mask":4294967295u32, "mesh":{"asset": ROBOT, "sub":"mesh/Cube"}}),
            ),
            (
                2,
                "fynkel.AudioSource",
                json!({"clip":{"asset":TONE},"bus":"sfx","volume":1,"pitch":1,"looping":false,"playing":false,"priority":128}),
            ),
            (
                3,
                "fynkel.Animator",
                json!({"clip":{"asset":RIG,"sub":"anim/wave"},"speed":1,"looping":true,"playing":false,"weight":1,"additive":false,"root_motion":false}),
            ),
        ] {
            components.insert(
                format!("d0000000-0000-4000-8000-{n:012}"),
                json!({"type":ty,"version":1,"data":data}),
            );
        }
        std::fs::write(file, doc.to_string()).unwrap();
        let report =
            cook_project(&project, Some(&dir.join("cache")), &CookOptions::default()).unwrap();
        assert!(
            !report.has_errors(),
            "{:?}",
            report
                .diagnostics
                .iter()
                .filter(|d| d.severity == fynkel_contracts::Severity::Error)
                .collect::<Vec<_>>()
        );
        Self(dir)
    }
    fn assets(&self) -> Assets {
        Assets::open(&self.0.join("cache")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn world() -> World {
    World::new(WorldConfig::default())
}
fn mesh() -> AssetRef {
    AssetRef::with_sub(id(ROBOT), "mesh/Cube")
}
fn clip() -> AssetRef {
    AssetRef::with_sub(id(RIG), "anim/wave")
}
fn sound() -> AssetRef {
    AssetRef::new(id(TONE))
}
fn untouched(w: &World) {
    assert!(!w.physics().has_mesh(&mesh()));
    assert!(!w.has_clip(&clip()));
    assert!(!w.has_sound(&sound()));
    assert!(!w.has_prefab(id(PREFAB)));
}

#[test]
fn preparation_and_drop_leave_world_untouched_and_balance_residency() {
    let f = Fixture::new("drop");
    let assets = f.assets();
    let w = world();
    let existing = assets.retain(&[id(TONE)]);
    let p = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(p.prefab().id, id(PREFAB));
    assert!(assets.is_resident(id(PREFAB)));
    untouched(&w);
    drop(p);
    assert!(!assets.is_resident(id(PREFAB)));
    assert!(assets.is_resident(id(TONE)));
    untouched(&w);
    drop(existing);
    assert!(!assets.is_resident(id(TONE)));
}

#[test]
fn publication_needs_no_source_files_and_registers_without_spawning() {
    let f = Fixture::new("publish");
    let assets = f.assets();
    let mut w = world();
    let p = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"));
    std::fs::remove_dir_all(f.0.join("cache")).unwrap();
    let published = p.publish(&mut w).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(w.physics().has_mesh(&mesh()));
    assert!(w.has_clip(&clip()));
    assert!(w.has_sound(&sound()));
    assert!(w.has_prefab(id(PREFAB)));
    assert!(w
        .edit_gameplay(|edit| edit.entities())
        .unwrap()
        .value
        .is_empty());
    let instance = w
        .edit_gameplay(|edit| {
            edit.instantiate(
                &published.prefab.description,
                "",
                fynkel_core::Transform::IDENTITY,
                None,
            )
        })
        .unwrap();
    assert!(instance.resolve(instance.value.root).is_some());
    drop(published);
    assert!(!assets.is_resident(id(PREFAB)));
}

#[test]
fn late_decode_failure_releases_preparation_without_partial_registration() {
    let f = Fixture::new("failure");
    let assets = f.assets();
    let w = world();
    std::fs::remove_file(assets.entry(id(TONE)).unwrap().file.unwrap()).unwrap();
    assert!(assets.prepare_prefab(id(PREFAB)).is_err());
    untouched(&w);
    assert!(!assets.is_resident(id(PREFAB)));
    assert!(!assets.is_resident(id(ROBOT)));
}

#[test]
fn audio_capacity_failure_happens_before_any_resource_publication() {
    let f = Fixture::new("capacity");
    let assets = f.assets();
    let mut w = world();
    let p = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"));
    p.validate_publication(&w).unwrap();
    let data = assets.sound(&sound()).unwrap();
    for i in 1..=fynkel_core::audio::MAX_CLIPS {
        w.register_sound(
            AssetRef::new(Uuid::from_bytes((i as u128).to_be_bytes())),
            data.clone(),
        )
        .unwrap();
    }
    assert!(p.publish(&mut w).is_err());
    untouched(&w);
    assert_eq!(w.audio().clip_count(), fynkel_core::audio::MAX_CLIPS);
    assert!(!assets.is_resident(id(PREFAB)));
}

#[test]
fn repeated_publication_reuses_registered_sound_slots() {
    let f = Fixture::new("reuse");
    let assets = f.assets();
    let mut w = world();
    let first = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"))
        .publish(&mut w)
        .unwrap_or_else(|d| panic!("{d:?}"));
    let second = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"))
        .publish(&mut w)
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(w.audio().clip_count(), 1);
    drop(first);
    assert!(assets.is_resident(id(PREFAB)));
    drop(second);
    assert!(!assets.is_resident(id(PREFAB)));
}

#[test]
fn data_commit_and_resources_publish_together_under_pending_parent() {
    let f = Fixture::new("commit");
    let assets = f.assets();
    let mut w = world();
    let p = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"));
    let (c, lease) = p
        .commit(&mut w, |edit, prefab| {
            let parent = edit.spawn("parent", fynkel_core::Transform::IDENTITY, None)?;
            edit.instantiate(
                &prefab.description,
                "instance",
                fynkel_core::Transform::IDENTITY,
                Some(parent.into()),
            )
        })
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(c.resolve(c.value.root).is_some());
    assert!(w.physics().has_mesh(&mesh()));
    assert!(w.has_clip(&clip()));
    assert!(w.has_sound(&sound()));
    assert!(assets.is_resident(id(PREFAB)));
    drop(lease);
}

#[test]
fn rejected_native_edit_drops_resources_and_residency() {
    let f = Fixture::new("abort");
    let assets = f.assets();
    let mut w = world();
    let p = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert!(p
        .commit(&mut w, |edit, prefab| {
            edit.instantiate(
                &prefab.description,
                "instance",
                fynkel_core::Transform::IDENTITY,
                None,
            )?;
            Err::<(), _>(fynkel_core::EditError::Aborted)
        })
        .is_err());
    untouched(&w);
    assert!(!assets.is_resident(id(PREFAB)));
    assert!(w
        .edit_gameplay(|edit| edit.entities())
        .unwrap()
        .value
        .is_empty());
}

#[test]
fn imported_rig_prepares_and_publishes_skin_resources() {
    let f = Fixture::new("rig");
    let assets = f.assets();
    let mut w = world();
    let p = assets
        .prepare_prefab(id(RIG))
        .unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(w.registered_skins().count(), 0);
    let lease = p.publish(&mut w).unwrap_or_else(|e| panic!("{e}"));
    assert!(w.registered_skins().count() > 0);
    drop(lease);
}

#[test]
fn owned_session_preflights_union_of_sound_keys_not_sum_of_plans() {
    let f = Fixture::new("session-union");
    let assets = f.assets();
    let mut w = world();
    let data = assets.sound(&sound()).unwrap();
    for i in 1..fynkel_core::audio::MAX_CLIPS {
        w.register_sound(
            AssetRef::new(Uuid::from_bytes((i as u128).to_be_bytes())),
            data.clone(),
        )
        .unwrap();
    }
    let a = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"));
    let b = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"));
    let mut session = fynkel_core::GameplaySession::begin(w).unwrap_or_else(|_| panic!("begin"));
    let instance = session
        .access(|edit| {
            edit.instantiate(
                &a.prefab().description,
                "instance",
                fynkel_core::Transform::IDENTITY,
                None,
            )
        })
        .unwrap();
    let (w, result) = fynkel_assets::PreparedPrefab::finish_session(vec![a, b], session);
    let (commit, leases) = result.unwrap_or_else(|e| panic!("{e}"));
    assert!(commit.resolve(instance.root).is_some());
    assert_eq!(w.audio().clip_count(), fynkel_core::audio::MAX_CLIPS);
    assert_eq!(leases.len(), 2);
    assert!(w.physics().has_mesh(&mesh()));
}

#[test]
fn owned_session_capacity_rejection_aborts_entities_and_resources() {
    let f = Fixture::new("session-full");
    let assets = f.assets();
    let mut w = world();
    let data = assets.sound(&sound()).unwrap();
    for i in 1..=fynkel_core::audio::MAX_CLIPS {
        w.register_sound(
            AssetRef::new(Uuid::from_bytes((i as u128).to_be_bytes())),
            data.clone(),
        )
        .unwrap();
    }
    let p = assets
        .prepare_prefab(id(PREFAB))
        .unwrap_or_else(|d| panic!("{d:?}"));
    let mut session = fynkel_core::GameplaySession::begin(w).unwrap_or_else(|_| panic!("begin"));
    session
        .access(|edit| {
            edit.instantiate(
                &p.prefab().description,
                "discard",
                fynkel_core::Transform::IDENTITY,
                None,
            )
        })
        .unwrap();
    let (mut w, result) = fynkel_assets::PreparedPrefab::finish_session(vec![p], session);
    assert!(result.is_err());
    untouched(&w);
    assert!(w
        .edit_gameplay(|edit| edit.entities())
        .unwrap()
        .value
        .is_empty());
    assert!(!assets.is_resident(id(PREFAB)));
}

#[test]
fn component_resource_plan_publishes_only_with_successful_session() {
    let fixture = Fixture::new("component-session");
    let assets = fixture.assets();
    let prefab = assets.prefab(id(PREFAB)).unwrap();
    let prepare = |world: &World| {
        let mut resources = fynkel_assets::PreparedGameplayResources::default();
        for entity in &prefab.description.entities {
            for value in &entity.components {
                resources.component(&assets, world, value);
            }
        }
        resources
    };
    let original = world();
    let resources = prepare(&original);
    untouched(&original);
    let mut session =
        fynkel_core::GameplaySession::begin(original).unwrap_or_else(|_| panic!("begin"));
    session
        .access(|edit| {
            edit.spawn("discarded", fynkel_core::Transform::default(), None)?;
            Err::<(), _>(fynkel_core::EditError::Aborted)
        })
        .unwrap_err();
    let (original, rejected) =
        fynkel_assets::PreparedPrefab::finish_session_with_resources(vec![], resources, session);
    assert!(rejected.is_err());
    untouched(&original);
    let resources = prepare(&original);
    let mut session =
        fynkel_core::GameplaySession::begin(original).unwrap_or_else(|_| panic!("begin"));
    session
        .access(|edit| edit.spawn("committed", fynkel_core::Transform::default(), None))
        .unwrap();
    let (published, result) =
        fynkel_assets::PreparedPrefab::finish_session_with_resources(vec![], resources, session);
    assert!(result.is_ok());
    assert!(published.physics().has_mesh(&mesh()));
    assert!(published.has_clip(&clip()));
    assert!(published.has_sound(&sound()));
}
