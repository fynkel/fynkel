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
    let mut session =
        fynkel_core::GameplaySession::begin(w.into()).unwrap_or_else(|_| panic!("begin"));
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
    let mut session =
        fynkel_core::GameplaySession::begin(w.into()).unwrap_or_else(|_| panic!("begin"));
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
        fynkel_core::GameplaySession::begin(original.into()).unwrap_or_else(|_| panic!("begin"));
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

fn resource_world(assets: &Assets) -> (World, fynkel_core::Entity) {
    let mut w = world();
    w.set_command_resource_source(Some(std::sync::Arc::new(assets.clone())));
    let commit = w
        .edit_gameplay(|e| e.spawn("owner", Default::default(), None))
        .unwrap();
    let owner = commit.resolve(commit.value).unwrap();
    (w, owner)
}
fn sound_change(owner: fynkel_core::Entity, volume: f32) -> fynkel_core::Command {
    fynkel_core::Command::SetComponent {
        entity: owner,
        value: fynkel_core::ComponentValue::AudioSource(fynkel_core::AudioSource {
            clip: sound(),
            volume,
            playing: false,
            ..Default::default()
        }),
    }
}

#[test]
fn live_edit_resource_preparation_does_not_publish_rejected_dependencies() {
    let f = Fixture::new("live-reject");
    let assets = f.assets();
    let (mut w, owner) = resource_world(&assets);
    assert_eq!(
        w.apply_gameplay_now(vec![sound_change(owner, 2.0)]).len(),
        1
    );
    untouched(&w);
    assert!(!w.ecs().has::<fynkel_core::AudioSource>(owner));
    assert_eq!(w.step().rejected, 0);
    untouched(&w);
    assert!(w
        .edit_gameplay(|edit| edit.animate_control(
            owner,
            fynkel_core::AnimateOp::Play {
                clip: clip(),
                looping: false,
                speed: 1.0
            }
        ))
        .is_err());
    untouched(&w);
    assert!(w
        .apply_gameplay_now(vec![sound_change(owner, 1.0)])
        .is_empty());
    assert!(w.has_sound(&sound()));
    assert_eq!(w.audio().clip_count(), 1);
    w.edit_gameplay(|_| Ok(())).unwrap();
    assert_eq!(w.step().rejected, 0);
}

fn audio_values(volumes: &[f32]) -> Vec<fynkel_core::ComponentValue> {
    use fynkel_core::{Animator, AudioSource, ComponentValue};
    let mut values = vec![ComponentValue::Animator(Animator {
        clip: clip(),
        playing: false,
        ..Default::default()
    })];
    values.extend(volumes.iter().map(|&volume| {
        ComponentValue::AudioSource(AudioSource {
            clip: sound(),
            volume,
            playing: false,
            ..Default::default()
        })
    }));
    values
}
fn audio_plan(
    assets: &Assets,
    world: &World,
    values: &[fynkel_core::ComponentValue],
) -> fynkel_assets::PreparedGameplayResources {
    let mut plan = fynkel_assets::PreparedGameplayResources::default();
    for value in values {
        plan.component(assets, world, value);
    }
    plan
}

#[test]
fn rejected_prepared_edit_preserves_data_and_all_resource_registries() {
    let f = Fixture::new("prepared-edit-reject");
    let assets = f.assets();
    let (mut w, owner) = resource_world(&assets);
    let values = audio_values(&[1.0]);
    assert!(audio_plan(&assets, &w, &values)
        .commit(&mut w, |edit| {
            for value in &values {
                edit.set_component(owner, value.clone())?;
            }
            edit.set_parent(owner, Some(owner.into()), false)
        })
        .is_err());
    untouched(&w);
    assert!(!w.ecs().has::<fynkel_core::Animator>(owner));
    assert!(!w.ecs().has::<fynkel_core::AudioSource>(owner));
    w.step();
    untouched(&w);
    audio_plan(&assets, &w, &values)
        .commit(&mut w, |edit| {
            for value in &values {
                edit.set_component(owner, value.clone())?;
            }
            Ok(())
        })
        .unwrap();
    assert!(w.has_clip(&clip()));
    assert!(w.has_sound(&sound()));
}

#[test]
fn prepared_edit_preflights_capacity_before_any_data_or_dependency_publication() {
    let f = Fixture::new("prepared-edit-capacity");
    let assets = f.assets();
    let (mut w, owner) = resource_world(&assets);
    let data = assets.sound(&sound()).unwrap();
    for i in 1..=fynkel_core::audio::MAX_CLIPS {
        w.register_sound(
            AssetRef::new(Uuid::from_bytes((i as u128).to_be_bytes())),
            data.clone(),
        )
        .unwrap();
    }
    let values = audio_values(&[1.0]);
    let mut entered = false;
    assert!(audio_plan(&assets, &w, &values)
        .commit(&mut w, |edit| {
            entered = true;
            for value in &values {
                edit.set_component(owner, value.clone())?;
            }
            Ok(())
        })
        .is_err());
    assert!(!entered);
    untouched(&w);
    assert!(!w.ecs().has::<fynkel_core::Animator>(owner));
    assert!(!w.ecs().has::<fynkel_core::AudioSource>(owner));
    w.step();
    untouched(&w);
}

#[test]
fn prepared_edit_deduplicates_dependencies_and_publishes_only_after_success() {
    let f = Fixture::new("prepared-edit-union");
    let assets = f.assets();
    let (mut w, owner) = resource_world(&assets);
    let data = assets.sound(&sound()).unwrap();
    for i in 1..fynkel_core::audio::MAX_CLIPS {
        w.register_sound(
            AssetRef::new(Uuid::from_bytes((i as u128).to_be_bytes())),
            data.clone(),
        )
        .unwrap();
    }
    let values = audio_values(&[0.2, 0.8]);
    let plan = audio_plan(&assets, &w, &values);
    untouched(&w);
    plan.commit(&mut w, |edit| {
        for value in &values {
            edit.set_component(owner, value.clone())?;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(w.audio().clip_count(), fynkel_core::audio::MAX_CLIPS);
    assert!(w.has_clip(&clip()));
    assert_eq!(
        w.ecs()
            .copy::<fynkel_core::AudioSource>(owner)
            .unwrap()
            .volume,
        0.8
    );
}

#[test]
fn prepared_and_immediate_sound_publish_dependencies_without_replay() {
    let f = Fixture::new("typed-sound");
    let assets = f.assets();
    for immediate in [false, true] {
        let (mut w, _) = resource_world(&assets);
        let params = |volume| fynkel_core::PlayParams {
            volume,
            priority: 128,
            ..Default::default()
        };
        if immediate {
            assert!(!w.play_sound_now(sound(), params(-1.0)).unwrap());
            assert_eq!(w.diagnostics().len(), 1);
            assert_eq!(
                w.diagnostics()[0].code,
                fynkel_core::DiagnosticCode::INVALID_PAYLOAD
            );
        } else {
            let mut plan = fynkel_assets::PreparedGameplayResources::default();
            plan.sound_request(&assets, &w, &sound());
            assert!(plan
                .commit(&mut w, |edit| edit.play_sound(sound(), params(-1.0)))
                .is_err());
        }
        assert!(!w.has_sound(&sound()));
        assert_eq!(w.audio_stats().started, 0);
        if immediate {
            assert!(w.play_sound_now(sound(), params(1.0)).unwrap());
        } else {
            let mut plan = fynkel_assets::PreparedGameplayResources::default();
            plan.sound_request(&assets, &w, &sound());
            plan.commit(&mut w, |edit| edit.play_sound(sound(), params(1.0)))
                .unwrap();
            assert!(w.has_sound(&sound()));
            assert_eq!(w.audio_stats().started, 0);
            assert_eq!(w.step().rejected, 0);
        }
        assert!(w.has_sound(&sound()));
        assert_eq!(w.audio_stats().started, 1);
        w.step();
        assert_eq!(w.audio_stats().started, 1);
    }
}

#[test]
fn rejected_live_mesh_and_skin_writes_drop_prepared_resources() {
    use fynkel_core::{Collider, ColliderShape, Command, ComponentValue, Skin};
    let f = Fixture::new("raw-mesh-skin");
    let assets = f.assets();
    let (mut w, owner) = resource_world(&assets);
    let skin = AssetRef::with_sub(id(RIG), "skin/Skin");
    let other = world();
    let foreign = fynkel_contracts::Entity::new(other.id(), 0, 1);
    let commands = vec![
        Command::SetComponent {
            entity: owner,
            value: ComponentValue::Collider(Collider {
                shape: ColliderShape::Mesh,
                mesh: Some(mesh()),
                friction: -1.0,
                ..Default::default()
            }),
        },
        Command::SetComponent {
            entity: owner,
            value: ComponentValue::Skin(Skin {
                skin: skin.clone(),
                skeleton: fynkel_contracts::ObjectRef::to(foreign),
            }),
        },
    ];
    assert_eq!(w.apply_gameplay_now(commands).len(), 1);
    assert!(!w.physics().has_mesh(&mesh()));
    assert!(!w.has_skin(&skin));
    assert!(!w.ecs().has::<Collider>(owner));
    assert!(!w.ecs().has::<Skin>(owner));
    assert!(w
        .apply_gameplay_now(vec![
            Command::SetComponent {
                entity: owner,
                value: ComponentValue::Collider(Collider {
                    shape: ColliderShape::Mesh,
                    mesh: Some(mesh()),
                    ..Default::default()
                })
            },
            Command::SetComponent {
                entity: owner,
                value: ComponentValue::Skin(Skin {
                    skin: skin.clone(),
                    skeleton: fynkel_contracts::ObjectRef::to(owner)
                })
            },
        ])
        .is_empty());
    assert!(w.physics().has_mesh(&mesh()));
    assert!(w.has_skin(&skin));
}

#[test]
fn native_resource_edit_rejects_without_registration_and_retry_publishes_once() {
    use fynkel_assets::PreparedGameplayResources;
    use fynkel_core::{ComponentValue, EditError, Transform};
    let fixture = Fixture::new("native-edit-resources");
    let assets = fixture.assets();
    let mut w = world();
    let prepared = assets.prepare_prefab(id(PREFAB)).unwrap();
    let values = &prepared.prefab().description.entities[0].components;
    let prepare = |w: &World| {
        let mut resources = PreparedGameplayResources::default();
        for value in values {
            resources.component(&assets, w, value);
        }
        resources
    };
    let result = prepare(&w).commit(&mut w, |edit| {
        let pending = edit.spawn("aborted", Transform::IDENTITY, None)?;
        for value in values {
            edit.set_component(pending, value.clone())?;
        }
        Err::<(), _>(EditError::Aborted)
    });
    assert!(result.is_err());
    assert_eq!(w.live_entities(), 0);
    untouched(&w);
    let commit = prepare(&w)
        .commit(&mut w, |edit| {
            let pending = edit.spawn("accepted", Transform::IDENTITY, None)?;
            for value in values {
                if !matches!(value, ComponentValue::Skin(_)) {
                    edit.set_component(pending, value.clone())?;
                }
            }
            Ok(pending)
        })
        .unwrap();
    assert!(commit.resolve(commit.value).is_some());
    assert!(w.has_clip(&clip()));
    assert!(w.has_sound(&sound()));
    let clips = w.audio().clip_count();
    prepare(&w).commit(&mut w, |_| Ok(())).unwrap();
    assert_eq!(w.audio().clip_count(), clips);
    assert_eq!(w.step().applied, 0);
}

#[test]
fn immediate_sound_prepares_only_valid_requests_and_never_flushes_simulation_effects() {
    use fynkel_core::PlayParams;
    let fixture = Fixture::new("immediate-sound");
    let mut w = world();
    w.set_command_resource_source(Some(std::sync::Arc::new(fixture.assets())));
    let invalid = PlayParams {
        volume: f32::NAN,
        ..PlayParams::default()
    };
    assert!(!w.play_sound_now(sound(), invalid).unwrap());
    assert!(!w.has_sound(&sound()));
    assert_eq!(w.audio_stats().started, 0);
    assert_eq!(w.tick(), 0);
    w.edit_gameplay(|edit| edit.play_sound(sound(), PlayParams::default()))
        .unwrap();
    assert!(w.play_sound_now(sound(), PlayParams::default()).unwrap());
    assert!(w.has_sound(&sound()));
    assert_eq!(w.audio_stats().started, 1);
    assert_eq!(w.tick(), 0);
    w.begin_frame(0.0);
    assert_eq!(w.audio_stats().started, 1);
    w.step();
    assert_eq!(w.audio_stats().started, 2);
    w.step();
    assert_eq!(w.audio_stats().started, 2);
    let count = w.audio().clip_count();
    assert!(w.play_sound_now(sound(), PlayParams::default()).unwrap());
    assert_eq!(w.audio().clip_count(), count);
    assert_eq!(w.audio_stats().started, 3);
}

#[test]
fn scene_resources_publish_with_activation_and_failed_replacement_can_retry() {
    use fynkel_assets::PreparedGameplayResources;
    use fynkel_core::{Camera, ComponentValue, LoadMode, SceneDescription};
    let fixture = Fixture::new("scene-activation");
    let assets = fixture.assets();
    let mut w = world();
    let old = w
        .load_scene(&SceneDescription {
            id: id(ROBOT),
            ..Default::default()
        })
        .unwrap();
    w.drain_events();
    let mut description = assets.prefab(id(PREFAB)).unwrap().description;
    description.entities[0]
        .components
        .push(ComponentValue::Camera(Camera {
            near: 0.0,
            ..Default::default()
        }));
    let (plan, report) = PreparedGameplayResources::for_scene(&assets, &w, &description);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    assert!(report.sounds > 0);
    assert!(w
        .load_scene_prepared(&description, LoadMode::Replace, Some(Box::new(plan)))
        .is_err());
    assert_eq!(w.loaded_scenes(), &[old]);
    assert!(!w.has_sound(&sound()));
    assert!(!w.has_clip(&clip()));
    assert!(!w.physics().has_mesh(&mesh()));
    assert!(w.drain_events().is_empty());
    description.entities[0].components.pop();
    let (plan, _) = PreparedGameplayResources::for_scene(&assets, &w, &description);
    let loaded = w
        .load_scene_prepared(&description, LoadMode::Replace, Some(Box::new(plan)))
        .unwrap();
    assert_eq!(w.live_entities(), loaded.len() as u32);
    assert!(w.has_sound(&sound()));
    assert!(w.has_clip(&clip()));
    assert!(w.physics().has_mesh(&mesh()));
    assert_eq!(w.audio_stats().started, 0);
}

#[test]
fn scene_capacity_preflight_preserves_old_scene_and_all_other_resource_registries() {
    use fynkel_assets::PreparedGameplayResources;
    use fynkel_core::{LoadMode, SceneDescription};
    let fixture = Fixture::new("scene-capacity");
    let assets = fixture.assets();
    let mut w = world();
    let old = w
        .load_scene(&SceneDescription {
            id: id(ROBOT),
            ..Default::default()
        })
        .unwrap();
    let sample = assets.sound(&sound()).unwrap();
    for i in 1..=fynkel_core::audio::MAX_CLIPS {
        w.register_sound(
            AssetRef::new(Uuid::from_bytes((i as u128).to_be_bytes())),
            sample.clone(),
        )
        .unwrap();
    }
    w.drain_events();
    let description = assets.prefab(id(PREFAB)).unwrap().description;
    let (plan, _) = PreparedGameplayResources::for_scene(&assets, &w, &description);
    assert!(w
        .load_scene_prepared(&description, LoadMode::Replace, Some(Box::new(plan)))
        .is_err());
    assert_eq!(w.loaded_scenes(), &[old]);
    assert!(!w.has_clip(&clip()));
    assert!(!w.has_sound(&sound()));
    assert!(!w.physics().has_mesh(&mesh()));
    assert!(w.drain_events().is_empty());
    assert_eq!(w.step().rejected, 0);
}
