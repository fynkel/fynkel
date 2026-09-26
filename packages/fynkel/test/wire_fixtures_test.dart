// Cross-language protocol fixtures: decodes tests/fixtures/wire/<fp>/*.bin
// (written by the Rust codecs, crates/fynkel_wire/tests/fixtures.rs) with
// the generated Dart codecs, checks the values, and re-encodes them
// byte-for-byte. Same sample values as the Rust side.

import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/generated/schema.g.dart';
import 'package:fynkel/math.dart';
import 'package:fynkel/src/asset_ref.dart';
import 'package:fynkel/src/wire/support.dart';
import 'package:test/test.dart' hide Tags;

Directory fixtureDir() {
  var dir = Directory.current;
  while (!File('${dir.path}/Cargo.toml').existsSync()) {
    final parent = dir.parent;
    if (parent.path == dir.path) throw StateError('repository root not found');
    dir = parent;
  }
  // Dart ints are signed: format the u64 fingerprint as Rust's {:016x}.
  final hi = (schemaFingerprint >>> 32).toRadixString(16).padLeft(8, '0');
  final lo = schemaFingerprint.toUnsigned(32).toRadixString(16).padLeft(8, '0');
  return Directory('${dir.path}/tests/fixtures/wire/$hi$lo');
}

/// world 1, slot, generation — mirrors fynkel_contracts::Entity::new.
int entity(int slot, int gen) => (1 << 56) | (gen << 32) | slot;

const transform = Transform(
  position: Vec3(1.5, -2.0, 3.25),
  rotation: Quat(0.0, 0.5, 0.0, 0.5),
  scale: Vec3(1.0, 2.0, 0.5),
);

const identity = Transform(position: Vec3.zero, rotation: Quat.identity, scale: Vec3.one);

void check<T>(
  String name,
  T expected,
  int wireSize,
  T Function(ByteData d, int o, Uint8List tail) decode,
  void Function(T v, ByteData d, int o, WireStrings s) encode,
) {
  test(name, () {
    final file = File('${fixtureDir().path}/$name.bin').readAsBytesSync();
    final bodyLen = ByteData.sublistView(file).getUint32(0, Endian.little);
    expect(bodyLen, wireSize, reason: 'record size');
    final body = Uint8List.sublistView(file, 4, 4 + bodyLen);
    final tail = Uint8List.sublistView(file, 4 + bodyLen);

    // Encoding the Dart sample must reproduce the Rust bytes exactly.
    final out = ByteData(wireSize);
    final strings = WireStrings();
    encode(expected, out, 0, strings);
    expect(out.buffer.asUint8List(), body, reason: 'body bytes');
    expect(strings.takeBytes(), tail, reason: 'string tail');

    // Decoding gives the f32-rounded sample (doubles pass through f32).
    final decoded = decode(ByteData.sublistView(body), 0, tail);
    final roundTripped = decode(out, 0, tail);
    expect(decoded, roundTripped);
  });
}

void main() {
  test('fixture directory exists for this fingerprint', () {
    expect(fixtureDir().existsSync(), isTrue, reason: fixtureDir().path);
  });

  check('Transform', transform, Transform.wireSize, Transform.decode, (v, d, o, s) => v.encode(d, o, s));
  check('Name', const Name(value: 'héllo wörld'), Name.wireSize, Name.decode, (v, d, o, s) => v.encode(d, o, s));
  check(
    'SpawnCommand',
    SpawnCommand(tempId: 42, transform: transform, parent: entity(7, 3), name: 'player'),
    SpawnCommand.wireSize,
    SpawnCommand.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'DestroyCommand',
    DestroyCommand(entity: entity(9, 1)),
    DestroyCommand.wireSize,
    DestroyCommand.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SetTransformCommand',
    SetTransformCommand(entity: entity(2, 2), transform: transform),
    SetTransformCommand.wireSize,
    SetTransformCommand.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SetParentCommand',
    SetParentCommand(child: entity(5, 1), parent: null, keepWorld: true),
    SetParentCommand.wireSize,
    SetParentCommand.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SetNameCommand',
    SetNameCommand(entity: entity(1, 1), name: ''),
    SetNameCommand.wireSize,
    SetNameCommand.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SpawnedEvent',
    SpawnedEvent(tempId: 42, entity: entity(11, 1)),
    SpawnedEvent.wireSize,
    SpawnedEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'DestroyedEvent',
    DestroyedEvent(entity: entity(11, 1)),
    DestroyedEvent.wireSize,
    DestroyedEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'ParentChangedEvent',
    ParentChangedEvent(child: entity(5, 1), parent: entity(6, 1)),
    ParentChangedEvent.wireSize,
    ParentChangedEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'MissedDeadlineEvent',
    const MissedDeadlineEvent(droppedSeconds: 0.1),
    MissedDeadlineEvent.wireSize,
    MissedDeadlineEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SceneReadyEvent',
    const SceneReadyEvent(scene: AssetRef('0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0ff')),
    SceneReadyEvent.wireSize,
    SceneReadyEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'ContactEvent',
    ContactEvent(
      a: entity(1, 1),
      b: entity(2, 1),
      point: const Vec3(0.5, 1.0, -0.25),
      normal: const Vec3(0, 1, 0),
      impulse: 3.5,
    ),
    ContactEvent.wireSize,
    ContactEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SnapshotTransformRecord',
    SnapshotTransformRecord(entity: entity(3, 4), local: transform, world: identity),
    SnapshotTransformRecord.wireSize,
    SnapshotTransformRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'DiagnosticRecord',
    DiagnosticRecord(
      commandIndex: 3,
      code: 2,
      entity: entity(8, 8),
      component: 1,
      lane: 1,
      message: 'stale or unknown entity',
    ),
    DiagnosticRecord.wireSize,
    DiagnosticRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'MeshRenderer',
    const MeshRenderer(
      mesh: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'mesh/0'),
      material: AssetRef('0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0ff'),
      castShadows: true,
      receiveShadows: false,
      visible: true,
    ),
    MeshRenderer.wireSize,
    MeshRenderer.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Camera',
    const Camera(
      projection: Projection.orthographic,
      fovY: 1.25,
      orthoHeight: 10.0,
      near: 0.1,
      far: 1000.0,
      priority: -1,
      exposureMode: ExposureMode.manual,
      exposure: 13.5,
      cullingMask: 0xfffffff7,
    ),
    Camera.wireSize,
    Camera.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'DirectionalLight',
    const DirectionalLight(color: Vec3(1.0, 0.95, 0.9), intensity: 10000.0, castShadows: true),
    DirectionalLight.wireSize,
    DirectionalLight.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'PointLight',
    const PointLight(color: Vec3(0.2, 0.4, 1.0), intensity: 800.0, range: 12.5, cullingMask: 11),
    PointLight.wireSize,
    PointLight.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SpotLight',
    const SpotLight(
        color: Vec3(1.0, 0.9, 0.8), intensity: 1500.0, range: 15.0, innerAngle: 0.4, outerAngle: 0.6, cullingMask: 0xfffffffe),
    SpotLight.wireSize,
    SpotLight.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Environment',
    const Environment(
      ambientColor: Vec3(0.8, 0.9, 1.0),
      ambientIntensity: 1500.0,
      sky: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'image/1'),
      skyIntensity: 5000.0,
      skyRotation: 0.25,
    ),
    Environment.wireSize,
    Environment.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  test('unknown enum index and malformed asset ids fail loudly', () {
    final d = ByteData(Camera.wireSize);
    d.setUint32(0, 2, Endian.little);
    expect(() => Camera.decode(d, 0, Uint8List(0)), throwsFormatException);
    final bad = const MeshRenderer(
      mesh: AssetRef('not-a-uuid'),
      material: AssetRef.none,
      castShadows: true,
      receiveShadows: true,
      visible: true,
    );
    expect(() => bad.encode(ByteData(MeshRenderer.wireSize), 0, WireStrings()), throwsFormatException);
    expect(const AssetRef('70D7BA8B-F7FB-4EBA-920D-EC9E6B968330').isValid, isFalse);
    expect(AssetRef.none.isNone, isTrue);
  });
  check(
    'PbrMaterial',
    const PbrMaterial(
      baseColor: Vec4(0.8, 0.5, 0.3, 1.0),
      metallic: 1.0,
      roughness: 0.65,
      emissive: Vec3(0.1, 0.1, 0.0),
      emissiveStrength: 3.0,
      normalScale: 0.5,
      occlusionStrength: 0.8,
      alphaMode: AlphaMode.mask,
      alphaCutoff: 0.4,
      doubleSided: true,
      baseColorTexture: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'image/0'),
      metallicRoughnessTexture: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'image/0/linear'),
      emissiveTexture: AssetRef('0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0ff'),
    ),
    PbrMaterial.wireSize,
    PbrMaterial.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'InputEventRecord',
    const InputEventRecord(device: 2, control: 'leftStick', phase: 2, x: -0.5, y: 0.25),
    InputEventRecord.wireSize,
    InputEventRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'InputActionRecord',
    const InputActionRecord(
      name: 'jump',
      kind: 0,
      held: true,
      pressed: true,
      released: false,
      framePressed: true,
      frameReleased: false,
      x: 1.0,
      y: 0.0,
      frameX: 1.0,
      frameY: 0.0,
    ),
    InputActionRecord.wireSize,
    InputActionRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'CommandHeader',
    const CommandHeader(kind: 1, size: 64),
    CommandHeader.wireSize,
    CommandHeader.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SetComponentCommand',
    SetComponentCommand(entity: entity(1, 1), component: 7),
    SetComponentCommand.wireSize,
    SetComponentCommand.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'RemoveComponentCommand',
    RemoveComponentCommand(entity: entity(2, 1), component: 4),
    RemoveComponentCommand.wireSize,
    RemoveComponentCommand.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'ComponentHeader',
    const ComponentHeader(component: 4, size: 56),
    ComponentHeader.wireSize,
    ComponentHeader.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'EventHeader',
    const EventHeader(kind: 1, size: 16, tickLo: 0xdeadbeef, flags: 0),
    EventHeader.wireSize,
    EventHeader.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'RigidBody',
    const RigidBody(
      bodyType: BodyType.kinematic,
      mass: 2.5,
      linearDamping: 0.1,
      angularDamping: 0.2,
      gravityScale: 0.5,
      ccd: true,
    ),
    RigidBody.wireSize,
    RigidBody.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Collider',
    Collider(
      shape: ColliderShape.mesh,
      size: const Vec3(0.5, 1.0, 0.25),
      mesh: const AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'mesh/0'),
      offset: const Vec3(0, 0.5, 0),
      material: const AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330'),
      friction: 0.6,
      restitution: 0.1,
      isSensor: true,
      mask: 0xfffffffd,
    ),
    Collider.wireSize,
    Collider.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'BodyCommandRecord',
    BodyCommandRecord(
      entity: entity(1, 1),
      op: BodyOp.setKinematicTarget,
      vector: const Vec3(1, 2, 3),
      rotation: const Quat(0, 0.5, 0, 0.5),
    ),
    BodyCommandRecord.wireSize,
    BodyCommandRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'PhysicsQueryRecord',
    PhysicsQueryRecord(
      id: 7,
      kind: PhysicsQueryKind.shapeCast,
      shape: ColliderShape.sphere,
      size: const Vec3(0.25, 0, 0),
      origin: const Vec3(0, 3, 0),
      rotation: Quat.identity,
      direction: const Vec3(0, 0, -1),
      maxDistance: 100,
      mask: 0xffffffff,
      exclude: entity(2, 1),
      maxHits: 4,
      hitSensors: false,
    ),
    PhysicsQueryRecord.wireSize,
    PhysicsQueryRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'PhysicsHitRecord',
    PhysicsHitRecord(
      query: 7,
      entity: entity(3, 1),
      point: const Vec3(0, 3, -4.5),
      normal: const Vec3(0, 0, 1),
      distance: 4.5,
    ),
    PhysicsHitRecord.wireSize,
    PhysicsHitRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'BodyStateRecord',
    BodyStateRecord(
      entity: entity(1, 1),
      linearVelocity: const Vec3(0, -9.81, 0),
      angularVelocity: const Vec3(0, 1, 0),
      sleeping: false,
    ),
    BodyStateRecord.wireSize,
    BodyStateRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'CharacterController',
    const CharacterController(
      radius: 0.35,
      height: 1.7,
      center: Vec3(0, 0.85, 0),
      skinWidth: 0.03,
      maxSlope: 0.8,
      maxStepHeight: 0.25,
      snapToGround: 0.1,
      gravityScale: 1.5,
      mass: 60,
      speed: 3.5,
      jumpSpeed: 6,
      turnSpeed: 8,
      moveAction: 'move',
      jumpAction: 'jump',
      mask: 0xfffffffb,
    ),
    CharacterController.wireSize,
    CharacterController.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'ThirdPersonCamera',
    const ThirdPersonCamera(
      target: 'Player',
      pivot: Vec3(0, 1.4, 0),
      distance: 6,
      minDistance: 0.75,
      yaw: 0.5,
      pitch: 0.25,
      minPitch: -0.4,
      maxPitch: 1.2,
      lookAction: 'look',
      sensitivity: 2,
      collisionRadius: 0.15,
      mask: 0xfffffffe,
    ),
    ThirdPersonCamera.wireSize,
    ThirdPersonCamera.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'CharacterStateRecord',
    CharacterStateRecord(entity: entity(5, 1), grounded: true, sliding: false, velocity: const Vec3(0, 0, -4)),
    CharacterStateRecord.wireSize,
    CharacterStateRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check('Layer', const Layer(layer: 3), Layer.wireSize, Layer.decode, (v, d, o, s) => v.encode(d, o, s));
  check('Tags', const Tags(bits: 0x80000005), Tags.wireSize, Tags.decode, (v, d, o, s) => v.encode(d, o, s));
  check(
    'Rotator',
    const Rotator(axis: Vec3(0, 1, 0), speed: 1.5, space: MotionSpace.world),
    Rotator.wireSize,
    Rotator.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Oscillate',
    const Oscillate(axis: Vec3(0, 1, 0), amplitude: 0.5, frequency: 0.25, phase: 1, space: MotionSpace.local),
    Oscillate.wireSize,
    Oscillate.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Follow',
    const Follow(target: 'Player', offset: Vec3(0, 3, -1), smoothing: 4, lookAt: true),
    Follow.wireSize,
    Follow.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'LookAtCamera',
    const LookAtCamera(lockY: false),
    LookAtCamera.wireSize,
    LookAtCamera.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'LifetimeDespawn',
    const LifetimeDespawn(seconds: 2.5),
    LifetimeDespawn.wireSize,
    LifetimeDespawn.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Emitter',
    const Emitter(
      prefab: AssetRef('0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0ff'),
      rate: 10,
      lifetime: 4,
      maxAlive: 40,
      velocity: Vec3(0, 6, 0),
      enabled: true,
    ),
    Emitter.wireSize,
    Emitter.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SystemTimeRecord',
    const SystemTimeRecord(system: SystemKind.physics, entities: 500, micros: 640),
    SystemTimeRecord.wireSize,
    SystemTimeRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'EntityInfoRecord',
    EntityInfoRecord(entity: entity(4, 2), parent: entity(1, 1), name: 'Lamp', layer: 2, tags: 0x80000005, active: false),
    EntityInfoRecord.wireSize,
    EntityInfoRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  // M3-12.
  check(
    'TriggerZone',
    const TriggerZone(event: 'collect', once: true),
    TriggerZone.wireSize,
    TriggerZone.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'SceneUnloadedEvent',
    const SceneUnloadedEvent(scene: AssetRef('0f1e2d3c-4b5a-4978-8695-a4b3c2d1e0ff'), entities: 12),
    SceneUnloadedEvent.wireSize,
    SceneUnloadedEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'ZoneEvent',
    ZoneEvent(zone: entity(7, 1), other: entity(2, 3), event: 'collect'),
    ZoneEvent.wireSize,
    ZoneEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Settings',
    const Settings(masterVolume: 0.8, musicVolume: 0.5, sfxVolume: 1, muted: true, quality: 'high'),
    Settings.wireSize,
    Settings.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  // M3-13.
  check(
    'ProjectionAnchorRecord',
    ProjectionAnchorRecord(entity: entity(4, 2), offset: const Vec3(0, 1.5, 0), mask: 5),
    ProjectionAnchorRecord.wireSize,
    ProjectionAnchorRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'ProjectedPointRecord',
    ProjectedPointRecord(entity: entity(4, 2), x: 640.5, y: 300.25, depth: 7.5, flags: 3),
    ProjectedPointRecord.wireSize,
    ProjectedPointRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  // M3-19.
  check(
    'Animator',
    Animator(
      clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'anim/1'),
      speed: 1.5,
      looping: false,
      playing: true,
      weight: 0.75,
      additive: true,
      rootMotion: true,
      controller: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330'),
    ),
    Animator.wireSize,
    Animator.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Skin',
    Skin(
      skin: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'skin/0'),
      skeleton: 'skeleton',
    ),
    Skin.wireSize,
    Skin.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AnimateCommandRecord',
    AnimateCommandRecord(
      entity: entity(4, 2),
      op: AnimateOp.crossfade,
      seconds: 0.25,
      speed: 1,
      looping: true,
      clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'anim/0'),
      value: 0.5,
      name: 'speed',
      integer: -3,
      layer: 1,
      point: Vec3(1, -2, 3.5),
    ),
    AnimateCommandRecord.wireSize,
    AnimateCommandRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AnimationEndedEvent',
    AnimationEndedEvent(
      entity: entity(4, 2),
      clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'anim/0'),
    ),
    AnimationEndedEvent.wireSize,
    AnimationEndedEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AnimatorStateRecord',
    AnimatorStateRecord(
      entity: entity(4, 2),
      time: 0.5,
      duration: 2,
      speed: 1,
      fadeRemaining: 0.125,
      playing: true,
      looping: false,
      clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'anim/0'),
      weight: 1,
      parameter: 3.5,
      rootDelta: const Vec3(0, 0, -0.05),
      rootYaw: 0.01,
    ),
    AnimatorStateRecord.wireSize,
    AnimatorStateRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  // M3-19c.
  check(
    'AnimationMarkerEvent',
    AnimationMarkerEvent(
      entity: entity(4, 2),
      clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'anim/0'),
      name: 'footstep',
    ),
    AnimationMarkerEvent.wireSize,
    AnimationMarkerEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  // ADR-0032.
  check(
    'AnimatorStateEnteredEvent',
    AnimatorStateEnteredEvent(entity: entity(4, 2), layer: 1, state: 'Jump', from: 'Locomotion'),
    AnimatorStateEnteredEvent.wireSize,
    AnimatorStateEnteredEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AnimatorStateExitedEvent',
    AnimatorStateExitedEvent(entity: entity(4, 2), layer: 0, state: 'Locomotion'),
    AnimatorStateExitedEvent.wireSize,
    AnimatorStateExitedEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AnimatorLayerStateRecord',
    AnimatorLayerStateRecord(
      entity: entity(4, 2),
      layer: 0,
      state: 'Locomotion',
      normalizedTime: 1.5,
      length: 0.8,
      inTransition: true,
      next: 'Jump',
      nextNormalizedTime: 0.25,
      transitionProgress: 0.5,
      slot: 'UpperBody',
      oneShot: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'anim/wave'),
      oneShotTime: 0.25,
      oneShotWeight: 0.75,
    ),
    AnimatorLayerStateRecord.wireSize,
    AnimatorLayerStateRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AnimatorParameterRecord',
    AnimatorParameterRecord(
      entity: entity(4, 2),
      name: 'weapon',
      kind: AnimatorParameterKind.int,
      value: 2,
      integer: 2,
      flag: false,
    ),
    AnimatorParameterRecord.wireSize,
    AnimatorParameterRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  // M3-11.
  check(
    'AudioSource',
    AudioSource(
      clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330'),
      bus: AudioBus.music,
      volume: 0.8,
      pitch: 1.25,
      looping: true,
      playing: true,
      priority: -2,
    ),
    AudioSource.wireSize,
    AudioSource.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AudioListener',
    const AudioListener(volume: 0.5),
    AudioListener.wireSize,
    AudioListener.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AudioCommandRecord',
    AudioCommandRecord(entity: entity(4, 2), op: AudioOp.setPitch, value: 0.75),
    AudioCommandRecord.wireSize,
    AudioCommandRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'PlaySoundCommandRecord',
    PlaySoundCommandRecord(
      clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330'),
      bus: AudioBus.sfx,
      volume: 0.9,
      pitch: 1.1,
      priority: 3,
    ),
    PlaySoundCommandRecord.wireSize,
    PlaySoundCommandRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AudioBusCommandRecord',
    const AudioBusCommandRecord(bus: AudioBus.master, volume: 0.6, muted: true),
    AudioBusCommandRecord.wireSize,
    AudioBusCommandRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'PlaybackEndedEvent',
    PlaybackEndedEvent(entity: entity(4, 2), clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330')),
    PlaybackEndedEvent.wireSize,
    PlaybackEndedEvent.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AudioStateRecord',
    AudioStateRecord(
      entity: entity(4, 2),
      playing: true,
      paused: false,
      time: 0.5,
      duration: 2.0,
      volume: 0.8,
      pitch: 1.0,
      looping: false,
      clip: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330'),
      bus: AudioBus.sfx,
      voice: 0x105,
    ),
    AudioStateRecord.wireSize,
    AudioStateRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'AudioStatsRecord',
    const AudioStatsRecord(
      device: true,
      sampleRate: 48000,
      channels: 2,
      voices: 3,
      maxVoices: 32,
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
    ),
    AudioStatsRecord.wireSize,
    AudioStatsRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check(
    'Volume',
    Volume(
      profile: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330'),
      shape: VolumeShape.box,
      size: const Vec3(4.0, 3.0, 8.0),
      radius: 2.5,
      blendDistance: 1.5,
      weight: 0.75,
      priority: -3,
      enabled: true,
    ),
    Volume.wireSize,
    Volume.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  // SetActive A2.
  check(
    'SetActiveCommand',
    SetActiveCommand(entity: entity(4, 2), active: false),
    SetActiveCommand.wireSize,
    SetActiveCommand.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
}
