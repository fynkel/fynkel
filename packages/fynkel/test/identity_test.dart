import 'dart:convert';
import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/entity_ref.dart' show readEntity, writeEntity;
import 'package:test/test.dart';

EntityRef spawn(World world) {
  final object = world.spawn(name: 'entity');
  world.step();
  world.drainEvents();
  return object.entity;
}

void main() {
  test('native host authority survives runtime disposal and repeated attachment', () {
    final authority = NativeLibrary.identityAuthority;
    final first = NativeLibrary.open(identityAuthority: authority);
    final a = World.create(library: first);
    final original = spawn(a);
    a.dispose();
    first.dispose();
    expect(NativeLibrary.identityAuthority, authority);
    final second = NativeLibrary.open(identityAuthority: authority);
    final b = World.create(library: second);
    final replacement = spawn(b);
    expect(replacement.world, isNot(original.world));
    b.dispose();
    second.dispose();
  });
  test('full-width wire, JSON and map keys preserve every identity bit', () {
    const value = EntityRef(-1, 0xffffffff, 0xffffffff);
    final data = ByteData(16);
    writeEntity(data, 0, value);
    expect(data.buffer.asUint8List(), List.filled(16, 255));
    final decoded = readEntity(data, 0);
    expect(decoded, value);
    expect(EntityRef.fromJson(jsonDecode(jsonEncode(value))), value);
    expect({value: 'present'}[decoded], 'present');
    expect(() => EntityRef.fromJson(1), throwsFormatException);
    expect(() => EntityRef.fromJson('00000001'), throwsFormatException);
  });

  test('actual native worlds in different runtimes reject foreign references', () {
    final first = World.create();
    final second = World.create();
    addTearDown(first.dispose);
    addTearDown(second.dispose);
    final original = spawn(first), other = spawn(second);
    expect(original.world, isNot(other.world));
    expect(original, isNot(other));
    expect(second.byEntity(original), isNull);
    expect(() => second.attach(original), throwsStateError);
    expect(second.attach(other).name, 'entity');
  });

  test('slot reuse rejects stale generation through SDK and native lookup', () {
    final world = World.create();
    addTearDown(world.dispose);
    final old = spawn(world);
    world.attach(old).destroy();
    world.step();
    world.drainEvents();
    final replacement = spawn(world);
    expect(replacement.slot, old.slot);
    expect(replacement.generation, isNot(old.generation));
    expect(() => world.attach(old), throwsStateError);
    expect(world.attach(replacement).isAlive, isTrue);
    expect(() => world.attach(EntityRef.temp(1)), throwsStateError);
    expect(() => world.attach(EntityRef.nil), throwsStateError);
  });

  test('Play restart and more than 255 worlds never recycle namespaces', () {
    final library = NativeLibrary.open();
    addTearDown(library.dispose);
    final first = World.create(library: library);
    final original = spawn(first);
    first.dispose();
    final seen = <int>{original.world};
    for (var i = 0; i < 300; i++) {
      final world = World.create(library: library);
      try {
        final current = spawn(world);
        expect(seen.add(current.world), isTrue);
        expect(() => world.attach(original), throwsStateError);
      } finally {
        world.dispose();
      }
    }
  });
}
