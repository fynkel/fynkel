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
import 'package:test/test.dart';

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
  check('DestroyCommand', DestroyCommand(entity: entity(9, 1)), DestroyCommand.wireSize, DestroyCommand.decode,
      (v, d, o, s) => v.encode(d, o, s));
  check('SetTransformCommand', SetTransformCommand(entity: entity(2, 2), transform: transform),
      SetTransformCommand.wireSize, SetTransformCommand.decode, (v, d, o, s) => v.encode(d, o, s));
  check('SetParentCommand', SetParentCommand(child: entity(5, 1), parent: null, keepWorld: true),
      SetParentCommand.wireSize, SetParentCommand.decode, (v, d, o, s) => v.encode(d, o, s));
  check('SetNameCommand', SetNameCommand(entity: entity(1, 1), name: ''), SetNameCommand.wireSize,
      SetNameCommand.decode, (v, d, o, s) => v.encode(d, o, s));
  check('SpawnedEvent', SpawnedEvent(tempId: 42, entity: entity(11, 1)), SpawnedEvent.wireSize, SpawnedEvent.decode,
      (v, d, o, s) => v.encode(d, o, s));
  check('DestroyedEvent', DestroyedEvent(entity: entity(11, 1)), DestroyedEvent.wireSize, DestroyedEvent.decode,
      (v, d, o, s) => v.encode(d, o, s));
  check('ParentChangedEvent', ParentChangedEvent(child: entity(5, 1), parent: entity(6, 1)),
      ParentChangedEvent.wireSize, ParentChangedEvent.decode, (v, d, o, s) => v.encode(d, o, s));
  check('MissedDeadlineEvent', const MissedDeadlineEvent(droppedSeconds: 0.1), MissedDeadlineEvent.wireSize,
      MissedDeadlineEvent.decode, (v, d, o, s) => v.encode(d, o, s));
  check('SceneReadyEvent', const SceneReadyEvent(), SceneReadyEvent.wireSize, SceneReadyEvent.decode,
      (v, d, o, s) => v.encode(d, o, s));
  check('ContactEvent', ContactEvent(a: entity(1, 1), b: entity(2, 1)), ContactEvent.wireSize, ContactEvent.decode,
      (v, d, o, s) => v.encode(d, o, s));
  check('SnapshotTransformRecord', SnapshotTransformRecord(entity: entity(3, 4), local: transform, world: identity),
      SnapshotTransformRecord.wireSize, SnapshotTransformRecord.decode, (v, d, o, s) => v.encode(d, o, s));
  check(
    'DiagnosticRecord',
    DiagnosticRecord(
        commandIndex: 3, code: 2, entity: entity(8, 8), component: 1, lane: 1, message: 'stale or unknown entity'),
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
        exposure: 13.5),
    Camera.wireSize,
    Camera.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check('DirectionalLight', const DirectionalLight(color: Vec3(1.0, 0.95, 0.9), intensity: 10000.0, castShadows: true),
      DirectionalLight.wireSize, DirectionalLight.decode, (v, d, o, s) => v.encode(d, o, s));
  check('PointLight', const PointLight(color: Vec3(0.2, 0.4, 1.0), intensity: 800.0, range: 12.5), PointLight.wireSize,
      PointLight.decode, (v, d, o, s) => v.encode(d, o, s));
  check(
      'SpotLight',
      const SpotLight(color: Vec3(1.0, 0.9, 0.8), intensity: 1500.0, range: 15.0, innerAngle: 0.4, outerAngle: 0.6),
      SpotLight.wireSize,
      SpotLight.decode,
      (v, d, o, s) => v.encode(d, o, s));
  check(
      'Environment',
      const Environment(
          ambientColor: Vec3(0.8, 0.9, 1.0),
          ambientIntensity: 1500.0,
          sky: AssetRef('70d7ba8b-f7fb-4eba-920d-ec9e6b968330', sub: 'image/1'),
          skyIntensity: 5000.0,
          skyRotation: 0.25),
      Environment.wireSize,
      Environment.decode,
      (v, d, o, s) => v.encode(d, o, s));
  test('unknown enum index and malformed asset ids fail loudly', () {
    final d = ByteData(Camera.wireSize);
    d.setUint32(0, 2, Endian.little);
    expect(() => Camera.decode(d, 0, Uint8List(0)), throwsFormatException);
    final bad = const MeshRenderer(
        mesh: AssetRef('not-a-uuid'), material: AssetRef.none, castShadows: true, receiveShadows: true, visible: true);
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
    ),
    InputActionRecord.wireSize,
    InputActionRecord.decode,
    (v, d, o, s) => v.encode(d, o, s),
  );
  check('CommandHeader', const CommandHeader(kind: 1, size: 64), CommandHeader.wireSize, CommandHeader.decode,
      (v, d, o, s) => v.encode(d, o, s));
  check('EventHeader', const EventHeader(kind: 1, size: 16, tickLo: 0xdeadbeef, flags: 0), EventHeader.wireSize,
      EventHeader.decode, (v, d, o, s) => v.encode(d, o, s));
}
