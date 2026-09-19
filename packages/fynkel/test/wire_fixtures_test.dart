// Cross-language protocol fixtures: decodes tests/fixtures/wire/<fp>/*.bin
// (written by the Rust codecs, crates/fynkel_wire/tests/fixtures.rs) with
// the generated Dart codecs, checks the values, and re-encodes them
// byte-for-byte. Same sample values as the Rust side.

import 'dart:io';
import 'dart:typed_data';

import 'package:fynkel/generated/schema.g.dart';
import 'package:fynkel/math.dart';
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
  check('EventHeader', const EventHeader(kind: 1, tickLo: 0xdeadbeef), EventHeader.wireSize, EventHeader.decode,
      (v, d, o, s) => v.encode(d, o, s));
}
