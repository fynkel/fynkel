import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/native/bridge.dart' show QueryKind, NativeGameplayRef;
import 'package:test/test.dart';

void main() {
  test('owned publications survive repeated queries, growth, poisoning and disposal', () {
    final world = World.create();
    final first = world.spawn(name: 'first');
    final entity = first.entity;
    final old = world.native.query(QueryKind.allEntities);
    final lazy = old.fixed(16, (d, o, tail) => d.getUint32(o));
    final oldBytes = old.bytes.toList();
    world.edit(() { for (var i = 0; i < 1500; i++) { world.spawn(name: 'grown-$i'); } });
    expect(world.native.query(QueryKind.allEntities).recordCount, 1501);
    expect(old.entities(), [entity]);
    first.destroy();
    world.native.query(QueryKind.allEntities);
    expect(world.native.debugPanic, throwsA(isA<FynkelBridgeError>()));
    world.dispose();
    expect(old.entities(), [entity]);
    expect(old.bytes, oldBytes);
    expect(lazy.toList(), hasLength(1));
    expect(() => old.bytes[0] = 0, throwsUnsupportedError);
    expect(() => old.bytes.buffer.asUint8List()[0] = 0, throwsUnsupportedError);
    expect(() => old.data.setUint32(0, 0), throwsUnsupportedError);
    expect(() => old.records.setUint32(0, 0), throwsUnsupportedError);
    expect(() => world.native.query(QueryKind.allEntities), throwsStateError);
  });

  test('sparse native renames and deaths update only affected SDK identities', () {
    final world = World.create();
    addTearDown(world.dispose);
    final objects = world.edit(() => List.generate(2048, (i) => world.spawn(name: 'object-$i')));
    world.edit(() {});
    final beforeSnapshots = world.native.gameplayStateSnapshots;
    final beforeBytes = world.native.gameplayStateBytes;
    world.native.editGameplay((edit) {
      edit.setName(NativeGameplayRef.live(objects[17].entity), 'changed');
      edit.destroy(NativeGameplayRef.live(objects[100].entity));
    });
    world.edit(() {
      expect(objects[17].name, 'changed');
      expect(objects[100].isAlive, isFalse);
      expect(objects[18].name, 'object-18');
    });
    expect(world.native.gameplayStateSnapshots, beforeSnapshots);
    expect(world.native.gameplayStateBytes - beforeBytes, lessThan(128));
    final bytes = world.native.gameplayStateBytes;
    world.edit(() {});
    expect(world.native.gameplayStateBytes - bytes, 12);
  });
}
