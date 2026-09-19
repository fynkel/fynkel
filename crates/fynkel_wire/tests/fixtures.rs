//! Versioned protocol fixtures (blueprint §5.7). Each record's sample value
//! is encoded by the generated Rust codec and compared with
//! `tests/fixtures/wire/<fingerprint>/<Record>.bin`; the Dart test
//! `packages/fynkel/test/wire_fixtures_test.dart` decodes the same files
//! with the generated Dart codecs and re-encodes them byte-for-byte.
//!
//! File layout: `u32 body_len` LE, body, string tail.
//! Regenerate with `FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_wire`.

use std::path::PathBuf;

use fynkel_contracts::{Entity, WorldId};
use fynkel_core::schema::*;
use fynkel_core::Name;
use fynkel_scene::Transform;
use fynkel_wire::{StringTable, WireRecord, SCHEMA_FINGERPRINT};
use glam::{Quat, Vec3};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/wire")
        .join(format!("{SCHEMA_FINGERPRINT:016x}"))
}

fn e(slot: u32, gen: u32) -> Entity {
    Entity::new(WorldId(1), slot, gen)
}

fn transform() -> Transform {
    Transform {
        position: Vec3::new(1.5, -2.0, 3.25),
        rotation: Quat::from_xyzw(0.0, 0.5, 0.0, 0.5),
        scale: Vec3::new(1.0, 2.0, 0.5),
    }
}

fn check<T: WireRecord + PartialEq + std::fmt::Debug>(name: &str, value: &T) {
    let mut strings = StringTable::new();
    let body = value.to_vec(&mut strings);
    let mut file = (body.len() as u32).to_le_bytes().to_vec();
    file.extend_from_slice(&body);
    file.extend_from_slice(strings.as_bytes());

    let decoded = T::decode(&body, strings.as_bytes()).unwrap();
    assert_eq!(&decoded, value, "{name} round trip");

    let path = dir().join(format!("{name}.bin"));
    if std::env::var_os("FYNKEL_UPDATE_FIXTURES").is_some() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(&path, &file).unwrap();
        return;
    }
    let on_disk = std::fs::read(&path).unwrap_or_else(|_| {
        panic!(
            "missing fixture {}; run with FYNKEL_UPDATE_FIXTURES=1",
            path.display()
        )
    });
    assert_eq!(
        on_disk, file,
        "{name}: fixture drift; regenerate with FYNKEL_UPDATE_FIXTURES=1"
    );
}

#[test]
fn fixtures_match_generated_codecs() {
    check("Transform", &transform());
    check("Name", &Name::new("héllo wörld"));
    check(
        "SpawnCommand",
        &SpawnCommand {
            temp_id: 42,
            transform: transform(),
            parent: Some(e(7, 3)),
            name: "player".into(),
        },
    );
    check("DestroyCommand", &DestroyCommand { entity: e(9, 1) });
    check(
        "SetTransformCommand",
        &SetTransformCommand {
            entity: e(2, 2),
            transform: transform(),
        },
    );
    check(
        "SetParentCommand",
        &SetParentCommand {
            child: e(5, 1),
            parent: None,
            keep_world: true,
        },
    );
    check(
        "SetNameCommand",
        &SetNameCommand {
            entity: e(1, 1),
            name: "".into(),
        },
    );
    check(
        "SpawnedEvent",
        &SpawnedEvent {
            temp_id: 42,
            entity: e(11, 1),
        },
    );
    check("DestroyedEvent", &DestroyedEvent { entity: e(11, 1) });
    check(
        "ParentChangedEvent",
        &ParentChangedEvent {
            child: e(5, 1),
            parent: Some(e(6, 1)),
        },
    );
    check(
        "MissedDeadlineEvent",
        &MissedDeadlineEvent {
            dropped_seconds: 0.1,
        },
    );
    check("SceneReadyEvent", &SceneReadyEvent);
    check(
        "ContactEvent",
        &ContactEvent {
            a: e(1, 1),
            b: e(2, 1),
        },
    );
    check(
        "SnapshotTransformRecord",
        &SnapshotTransformRecord {
            entity: e(3, 4),
            local: transform(),
            world: Transform::IDENTITY,
        },
    );
    check(
        "DiagnosticRecord",
        &DiagnosticRecord {
            command_index: 3,
            code: 2,
            entity: e(8, 8),
            component: 1,
            lane: 1,
            message: "stale or unknown entity".into(),
        },
    );
    check("CommandHeader", &CommandHeader { kind: 1, size: 64 });
    check(
        "EventHeader",
        &EventHeader {
            kind: 1,
            size: 16,
            tick_lo: 0xdead_beef,
            flags: 0,
        },
    );
}

#[test]
fn truncated_and_bad_strings_fail_safely() {
    use fynkel_wire::WireError;
    assert_eq!(
        Transform::decode(&[0u8; 39], &[]),
        Err(WireError::Truncated {
            needed: 40,
            got: 39
        })
    );
    let mut b = [0u8; 8];
    fynkel_wire::put_u32(&mut b, 0, 4);
    fynkel_wire::put_u32(&mut b, 4, 10);
    assert_eq!(
        Name::decode(&b, b"tail"),
        Err(WireError::BadString { offset: 4, len: 10 })
    );
    fynkel_wire::put_u32(&mut b, 0, u32::MAX);
    assert!(Name::decode(&b, b"tail").is_err());
}
