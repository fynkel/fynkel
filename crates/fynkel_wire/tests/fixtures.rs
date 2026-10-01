//! Versioned protocol fixtures (blueprint §5.7). Each record's sample value
//! is encoded by the generated Rust codec and compared with
//! `tests/fixtures/wire/<fingerprint>/<Record>.bin`; the Dart test
//! `packages/fynkel/test/wire_fixtures_test.dart` decodes the same files
//! with the generated Dart codecs and re-encodes them byte-for-byte.
//!
//! File layout: `u32 body_len` LE, body, string tail.
//! Regenerate with `FYNKEL_UPDATE_FIXTURES=1 cargo test -p fynkel_wire`.

use std::path::PathBuf;

use fynkel_contracts::{AssetId, AssetRef, Entity, ObjectRef, WorldId};
use fynkel_core::components::{
    Camera, DirectionalLight, Environment, ExposureMode, MeshRenderer, PointLight, Projection,
    SpotLight,
};
use fynkel_core::material::{AlphaMode, PbrMaterial};
use fynkel_core::schema::*;
use fynkel_core::{
    Animator, AudioBus, AudioListener, AudioSource, BodyType, CharacterController, Collider,
    ColliderShape, Emitter, Follow, Layer, LifetimeDespawn, LookAtCamera, MotionSpace, Name,
    Oscillate, RigidBody, Rotator, Settings, Skin, Tags, ThirdPersonCamera, TriggerZone, Volume,
    VolumeShape,
};
use fynkel_scene::Transform;
use fynkel_wire::{StringTable, WireRecord, SCHEMA_FINGERPRINT};
use glam::{Quat, Vec3, Vec4};

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
    check(
        "SceneReadyEvent",
        &SceneReadyEvent {
            scene: AssetRef::new("0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0ff".parse().unwrap()),
        },
    );
    check(
        "ContactEvent",
        &ContactEvent {
            a: e(1, 1),
            b: e(2, 1),
            point: Vec3::new(0.5, 1.0, -0.25),
            normal: Vec3::Y,
            impulse: Vec3::new(0.0, 3.5, 0.0),
            body_a: Some(e(4, 1)),
            body_b: None,
            relative_velocity: Vec3::new(0.0, 7.0, 0.5),
            contact_count: 2,
            contact0: ContactPointRecord {
                point: Vec3::new(0.5, 1.0, -0.25),
                normal: Vec3::Y,
                separation: -0.01,
                impulse: 2.0,
            },
            contact1: ContactPointRecord {
                point: Vec3::new(-0.5, 1.0, 0.25),
                normal: Vec3::Y,
                separation: 0.0,
                impulse: 1.5,
            },
            contact2: ContactPointRecord::default(),
            contact3: ContactPointRecord::default(),
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
    let mesh_id: AssetId = "70d7ba8b-f7fb-4eba-920d-ec9e6b968330".parse().unwrap();
    let material_id: AssetId = "0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0ff".parse().unwrap();
    check(
        "MeshRenderer",
        &MeshRenderer {
            mesh: AssetRef::with_sub(mesh_id, "mesh/0"),
            material: AssetRef::new(material_id),
            cast_shadows: true,
            receive_shadows: false,
            visible: true,
        },
    );
    check(
        "Camera",
        &Camera {
            projection: Projection::Orthographic,
            fov_y: 1.25,
            ortho_height: 10.0,
            near: 0.1,
            far: 1000.0,
            priority: -1,
            exposure_mode: ExposureMode::Manual,
            exposure: 13.5,
            culling_mask: 0xffff_fff7,
        },
    );
    check(
        "DirectionalLight",
        &DirectionalLight {
            color: Vec3::new(1.0, 0.95, 0.9),
            intensity: 10_000.0,
            cast_shadows: true,
        },
    );
    check(
        "PointLight",
        &PointLight {
            color: Vec3::new(0.2, 0.4, 1.0),
            intensity: 800.0,
            range: 12.5,
            culling_mask: 0b1011,
        },
    );
    check(
        "PbrMaterial",
        &PbrMaterial {
            base_color: Vec4::new(0.8, 0.5, 0.3, 1.0),
            metallic: 1.0,
            roughness: 0.65,
            emissive: Vec3::new(0.1, 0.1, 0.0),
            emissive_strength: 3.0,
            normal_scale: 0.5,
            occlusion_strength: 0.8,
            alpha_mode: AlphaMode::Mask,
            alpha_cutoff: 0.4,
            double_sided: true,
            base_color_texture: Some(AssetRef::with_sub(mesh_id, "image/0")),
            normal_texture: None,
            metallic_roughness_texture: Some(AssetRef::with_sub(mesh_id, "image/0/linear")),
            occlusion_texture: None,
            emissive_texture: Some(AssetRef::new(material_id)),
        },
    );
    check(
        "SpotLight",
        &SpotLight {
            color: Vec3::new(1.0, 0.9, 0.8),
            intensity: 1_500.0,
            range: 15.0,
            inner_angle: 0.4,
            outer_angle: 0.6,
            culling_mask: 0xffff_fffe,
        },
    );
    check(
        "Environment",
        &Environment {
            ambient_color: Vec3::new(0.8, 0.9, 1.0),
            ambient_intensity: 1_500.0,
            sky: Some(AssetRef::with_sub(mesh_id, "image/1")),
            sky_intensity: 5_000.0,
            sky_rotation: 0.25,
        },
    );
    check(
        "InputEventRecord",
        &InputEventRecord {
            device: 2,
            control: "leftStick".into(),
            phase: 2,
            x: -0.5,
            y: 0.25,
        },
    );
    check(
        "InputActionRecord",
        &InputActionRecord {
            name: "jump".into(),
            kind: 0,
            held: true,
            pressed: true,
            released: false,
            frame_pressed: true,
            frame_released: false,
            x: 1.0,
            y: 0.0,
            frame_x: 1.0,
            frame_y: 0.0,
        },
    );
    check(
        "ComponentHeader",
        &ComponentHeader {
            component: 4,
            size: 56,
        },
    );
    check(
        "EventHeader",
        &EventHeader {
            kind: 1,
            size: 16,
            tick_lo: 0xdead_beef,
            flags: 0,
        },
    );
    check(
        "RigidBody",
        &RigidBody {
            body_type: BodyType::Kinematic,
            mass: 2.5,
            linear_damping: 0.1,
            angular_damping: 0.2,
            gravity_scale: 0.5,
            ccd: true,
            constraints: 116,
        },
    );
    check(
        "Collider",
        &Collider {
            shape: ColliderShape::Mesh,
            size: Vec3::new(0.5, 1.0, 0.25),
            mesh: Some(AssetRef::with_sub(mesh_id, "mesh/0")),
            offset: Vec3::new(0.0, 0.5, 0.0),
            material: Some(AssetRef::new(mesh_id)),
            friction: 0.6,
            restitution: 0.1,
            is_sensor: true,
            mask: 0xffff_fffd,
        },
    );
    check(
        "BodyCommandRecord",
        &BodyCommandRecord {
            entity: e(1, 1),
            op: BodyOp::SetKinematicTarget,
            vector: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::from_xyzw(0.0, 0.5, 0.0, 0.5),
        },
    );
    check(
        "PhysicsQueryRecord",
        &PhysicsQueryRecord {
            id: 7,
            kind: PhysicsQueryKind::ShapeCast,
            shape: ColliderShape::Sphere,
            size: Vec3::new(0.25, 0.0, 0.0),
            origin: Vec3::new(0.0, 3.0, 0.0),
            rotation: Quat::IDENTITY,
            direction: Vec3::NEG_Z,
            max_distance: 100.0,
            mask: u32::MAX,
            exclude: Some(e(2, 1)),
            max_hits: 4,
            hit_sensors: false,
        },
    );
    check(
        "PhysicsHitRecord",
        &PhysicsHitRecord {
            query: 7,
            entity: e(3, 1),
            point: Vec3::new(0.0, 3.0, -4.5),
            normal: Vec3::Z,
            distance: 4.5,
        },
    );
    check(
        "BodyStateRecord",
        &BodyStateRecord {
            entity: e(1, 1),
            linear_velocity: Vec3::new(0.0, -9.81, 0.0),
            angular_velocity: Vec3::new(0.0, 1.0, 0.0),
            sleeping: false,
        },
    );
    check(
        "CharacterController",
        &CharacterController {
            radius: 0.35,
            height: 1.7,
            center: Vec3::new(0.0, 0.85, 0.0),
            skin_width: 0.03,
            max_slope: 0.8,
            max_step_height: 0.25,
            snap_to_ground: 0.1,
            gravity_scale: 1.5,
            mass: 60.0,
            speed: 3.5,
            jump_speed: 6.0,
            turn_speed: 8.0,
            move_action: "move".into(),
            jump_action: "jump".into(),
            mask: 0xffff_fffb,
        },
    );
    check(
        "ThirdPersonCamera",
        &ThirdPersonCamera {
            target: ObjectRef::NONE,
            pivot: Vec3::new(0.0, 1.4, 0.0),
            distance: 6.0,
            min_distance: 0.75,
            yaw: 0.5,
            pitch: 0.25,
            min_pitch: -0.4,
            max_pitch: 1.2,
            look_action: "look".into(),
            sensitivity: 2.0,
            collision_radius: 0.15,
            mask: 0xffff_fffe,
        },
    );
    check(
        "CharacterStateRecord",
        &CharacterStateRecord {
            entity: e(5, 1),
            grounded: true,
            sliding: false,
            velocity: Vec3::new(0.0, 0.0, -4.0),
            collision_flags: 5,
        },
    );
    check("Layer", &Layer { layer: 3 });
    check("Tags", &Tags { bits: 0x8000_0005 });
    check(
        "Rotator",
        &Rotator {
            axis: Vec3::new(0.0, 1.0, 0.0),
            speed: 1.5,
            space: MotionSpace::World,
        },
    );
    check(
        "Oscillate",
        &Oscillate {
            axis: Vec3::new(0.0, 1.0, 0.0),
            amplitude: 0.5,
            frequency: 0.25,
            phase: 1.0,
            space: MotionSpace::Local,
        },
    );
    check(
        "Follow",
        &Follow {
            target: ObjectRef::NONE,
            offset: Vec3::new(0.0, 3.0, -1.0),
            smoothing: 4.0,
            look_at: true,
        },
    );
    check("LookAtCamera", &LookAtCamera { lock_y: false });
    check("LifetimeDespawn", &LifetimeDespawn { seconds: 2.5 });
    check(
        "Emitter",
        &Emitter {
            prefab: AssetRef::new(material_id),
            rate: 10.0,
            lifetime: 4.0,
            max_alive: 40,
            velocity: Vec3::new(0.0, 6.0, 0.0),
            enabled: true,
        },
    );
    check(
        "SystemTimeRecord",
        &SystemTimeRecord {
            system: SystemKind::Physics,
            entities: 500,
            micros: 640,
        },
    );
    check(
        "EntityInfoRecord",
        &EntityInfoRecord {
            entity: e(4, 2),
            parent: e(1, 1),
            name: "Lamp".into(),
            layer: 2,
            tags: 0x8000_0005,
            active: false,
        },
    );
    // M3-12.
    check(
        "TriggerZone",
        &TriggerZone {
            event: "collect".into(),
            once: true,
        },
    );
    check(
        "SceneUnloadedEvent",
        &SceneUnloadedEvent {
            scene: AssetRef::new(material_id),
            entities: 12,
        },
    );
    check(
        "ZoneEvent",
        &ZoneEvent {
            zone: e(7, 1),
            other: e(2, 3),
            event: "collect".into(),
        },
    );
    check(
        "Settings",
        &Settings {
            master_volume: 0.8,
            music_volume: 0.5,
            sfx_volume: 1.0,
            muted: true,
            quality: "high".into(),
        },
    );
    // M3-13.
    check(
        "ProjectionAnchorRecord",
        &ProjectionAnchorRecord {
            entity: e(4, 2),
            offset: Vec3::new(0.0, 1.5, 0.0),
            mask: 0b101,
        },
    );
    check(
        "ProjectedPointRecord",
        &ProjectedPointRecord {
            entity: e(4, 2),
            x: 640.5,
            y: 300.25,
            depth: 7.5,
            flags: 3,
        },
    );
    // M3-19.
    check(
        "Animator",
        &Animator {
            clip: AssetRef::with_sub(mesh_id, "anim/1"),
            speed: 1.5,
            looping: false,
            playing: true,
            weight: 0.75,
            additive: true,
            root_motion: true,
            controller: Some(AssetRef::new(mesh_id)),
        },
    );
    check(
        "Skin",
        &Skin {
            skin: AssetRef::with_sub(mesh_id, "skin/0"),
            skeleton: ObjectRef::NONE,
        },
    );
    check(
        "AnimateCommandRecord",
        &AnimateCommandRecord {
            entity: e(4, 2),
            op: AnimateOp::Crossfade,
            seconds: 0.25,
            speed: 1.0,
            looping: true,
            clip: AssetRef::with_sub(mesh_id, "anim/0"),
            value: 0.5,
            name: "speed".into(),
            integer: -3,
            layer: 1,
            point: Vec3::new(1.0, -2.0, 3.5),
        },
    );
    check(
        "AnimationEndedEvent",
        &AnimationEndedEvent {
            entity: e(4, 2),
            clip: AssetRef::with_sub(mesh_id, "anim/0"),
        },
    );
    check(
        "AnimatorStateRecord",
        &AnimatorStateRecord {
            entity: e(4, 2),
            time: 0.5,
            duration: 2.0,
            speed: 1.0,
            fade_remaining: 0.125,
            playing: true,
            looping: false,
            clip: AssetRef::with_sub(mesh_id, "anim/0"),
            weight: 1.0,
            parameter: 3.5,
            root_delta: Vec3::new(0.0, 0.0, -0.05),
            root_yaw: 0.01,
        },
    );
    // M3-19c.
    check(
        "AnimationMarkerEvent",
        &AnimationMarkerEvent {
            entity: e(4, 2),
            clip: AssetRef::with_sub(mesh_id, "anim/0"),
            name: "footstep".into(),
        },
    );
    // ADR-0032.
    check(
        "AnimatorStateEnteredEvent",
        &AnimatorStateEnteredEvent {
            entity: e(4, 2),
            layer: 1,
            state: "Jump".into(),
            from: "Locomotion".into(),
        },
    );
    check(
        "AnimatorStateExitedEvent",
        &AnimatorStateExitedEvent {
            entity: e(4, 2),
            layer: 0,
            state: "Locomotion".into(),
        },
    );
    check(
        "AnimatorLayerStateRecord",
        &AnimatorLayerStateRecord {
            entity: e(4, 2),
            layer: 0,
            state: "Locomotion".into(),
            normalized_time: 1.5,
            length: 0.8,
            in_transition: true,
            next: "Jump".into(),
            next_normalized_time: 0.25,
            transition_progress: 0.5,
            slot: "UpperBody".into(),
            one_shot: AssetRef::with_sub(mesh_id, "anim/wave"),
            one_shot_time: 0.25,
            one_shot_weight: 0.75,
        },
    );
    check(
        "AnimatorParameterRecord",
        &AnimatorParameterRecord {
            entity: e(4, 2),
            name: "weapon".into(),
            kind: AnimatorParameterKind::Int,
            value: 2.0,
            integer: 2,
            flag: false,
        },
    );
    // M3-11.
    check(
        "AudioSource",
        &AudioSource {
            clip: AssetRef::new(mesh_id),
            bus: AudioBus::Music,
            volume: 0.8,
            pitch: 1.25,
            looping: true,
            playing: true,
            priority: -2,
        },
    );
    check("AudioListener", &AudioListener { volume: 0.5 });
    check(
        "AudioCommandRecord",
        &AudioCommandRecord {
            entity: e(4, 2),
            op: AudioOp::SetPitch,
            value: 0.75,
        },
    );
    check(
        "PlaySoundCommandRecord",
        &PlaySoundCommandRecord {
            clip: AssetRef::new(mesh_id),
            bus: AudioBus::Sfx,
            volume: 0.9,
            pitch: 1.1,
            priority: 3,
        },
    );
    check(
        "PlaybackEndedEvent",
        &PlaybackEndedEvent {
            entity: e(4, 2),
            clip: AssetRef::new(mesh_id),
        },
    );
    check(
        "AudioStateRecord",
        &AudioStateRecord {
            entity: e(4, 2),
            playing: true,
            paused: false,
            time: 0.5,
            duration: 2.0,
            volume: 0.8,
            pitch: 1.0,
            looping: false,
            clip: AssetRef::new(mesh_id),
            bus: AudioBus::Sfx,
            voice: 0x105,
        },
    );
    check(
        "AudioStatsRecord",
        &AudioStatsRecord {
            device: true,
            sample_rate: 48_000,
            channels: 2,
            voices: 3,
            max_voices: 32,
            started: 10,
            stolen: 1,
            refused: 2,
            ended: 7,
            clips: 4,
            callbacks: 1000,
            underruns: 0,
            suspended: false,
            streams: 1,
            starved: 5,
        },
    );
    check(
        "Volume",
        &Volume {
            profile: AssetRef::new(mesh_id),
            shape: VolumeShape::Box,
            size: Vec3::new(4.0, 3.0, 8.0),
            radius: 2.5,
            blend_distance: 1.5,
            weight: 0.75,
            priority: -3,
            enabled: true,
        },
    );
    // SetActive A2.
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

    // An enum index from a newer peer is an error, not a silent default.
    let mut b = [0u8; Camera::SIZE];
    fynkel_wire::put_u32(&mut b, 0, 2);
    assert_eq!(
        Camera::decode(&b, &[]),
        Err(WireError::BadEnum {
            name: "fynkel.Projection",
            index: 2
        })
    );
}
