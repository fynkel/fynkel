// Executable F0 contract experiments; no assertion requires an old defect.
import 'dart:convert';

import 'package:fynkel/advanced.dart' as engine;
import 'package:test/test.dart';

import '../tool/foundation_prototype/slice.dart';
import '../tool/foundation_prototype/world.dart';

void main() {
  late engine.NativeLibrary library;
  late PrototypeWorld world;
  late EntityRef player, door;
  setUpAll(() => library = engine.NativeLibrary.open());
  tearDownAll(() => library.dispose());
  setUp(() {
    world = PrototypeWorld(library: library);
    world.register(healthType);
    world.register(doorType);
    player = world.spawn('player');
    door = world.spawn('door');
  });
  tearDown(() => world.dispose());

  void start({double hp = 100}) {
    world.initialize();
    world.edit(player, healthType, {'hp': hp, 'poisonPerSecond': 2.0});
    world.edit(door, doorType, {'open': false});
  }

  double hp() => (world.save(player, healthType)['fields'] as Map<String, Object?>)['hp'] as double;

  test('object and population reads share pending writes, including new components', () {
    start();
    world.transaction(
      writes: [healthType],
      run: (access) {
        access.object(player).set(healthType, const Health(hp: 42));
        access.set(door, healthType, const Health(hp: 7));
        final seen = <EntityRef, double>{};
        access.forEach(healthType, (entity, value) => seen[entity] = value.hp);
        expect(seen, {player: 42.0, door: 7.0});
      },
    );
    expect(hp(), 42);
  });

  test('query membership is fixed at opening; next query includes insertion', () {
    start();
    world.transaction(
      writes: [healthType],
      run: (access) {
        var firstCount = 0;
        access.forEach(healthType, (_, _) {
          firstCount++;
          access.set(door, healthType, const Health());
        });
        var nextCount = 0;
        access.forEach(healthType, (_, _) => nextCount++);
        expect(firstCount, 1);
        expect(nextCount, 2);
      },
    );
  });

  test('removal during iteration preserves traversal and commits once', () {
    start();
    world.edit(door, healthType, {'hp': 7.0});
    world.transaction(
      writes: [healthType],
      run: (access) {
        var visited = 0;
        access.forEach(healthType, (entity, _) {
          visited++;
          access.remove(entity, healthType);
          expect(access.object(entity).get(healthType), isNull);
        });
        expect(visited, 2);
        var remaining = 0;
        access.forEach(healthType, (_, _) => remaining++);
        expect(remaining, 0);
      },
    );
    expect(() => world.save(player, healthType), throwsStateError);
  });

  test('invalid multi-record callback rolls back all pending writes', () {
    start();
    expect(
      () => world.transaction(
        writes: [healthType, doorType],
        run: (access) {
          access.set(door, doorType, const DoorState(open: true));
          access.set(player, healthType, const Health(hp: -1));
        },
      ),
      throwsArgumentError,
    );
    expect(hp(), 100);
    expect((world.save(door, doorType)['fields'] as Map)['open'], false);
  });

  test('undeclared access rejects the entire callback', () {
    start();
    expect(
      () => world.transaction(
        writes: [healthType],
        run: (access) {
          access.set(player, healthType, const Health(hp: 3));
          access.get(door, doorType);
        },
      ),
      throwsStateError,
    );
    expect(hp(), 100);
  });

  test('access and object helpers expire together at the commit boundary', () {
    start();
    late GameplayAccess access;
    late ComponentObject object;
    world.transaction(
      reads: [healthType],
      run: (value) {
        access = value;
        object = value.object(player);
      },
    );
    expect(() => access.get(player, healthType), throwsStateError);
    expect(() => object.get(healthType), throwsStateError);
    expect(() => world.transaction(run: (_) => world.replaceScene()), throwsStateError);
  });

  test('owner check rejects foreign references even when native bits collide', () {
    start();
    final other = PrototypeWorld(library: library);
    try {
      other.register(healthType);
      final foreign = other.spawn('foreign');
      other.initialize();
      expect(
        () => world.transaction(reads: [healthType], run: (access) => access.get(foreign, healthType)),
        throwsStateError,
      );
    } finally {
      other.dispose();
    }
  });

  test('scene replacement retires old references and restores authored custom data', () {
    start(hp: 42);
    final document = jsonDecode(jsonEncode(world.save(player, healthType))) as Map<String, Object?>;
    world.replaceScene();
    final replacement = world.spawn('replacement');
    world.initialize();
    world.load(replacement, healthType, document);
    expect(world.save(replacement, healthType), document);
    expect(() => player.nativeHandle, throwsStateError);
    expect(() => world.save(player, healthType), throwsStateError);
  });

  test('inspector codec rejects unknown fields and versions without changing data', () {
    start();
    expect(() => world.edit(player, healthType, {'unknown': 1}), throwsFormatException);
    expect(() => world.edit(player, healthType, {'hp': double.nan}), throwsFormatException);
    final document = world.save(player, healthType);
    expect(() => world.load(player, healthType, {...document, 'version': 99}), throwsFormatException);
    expect(world.save(player, healthType), document);
  });

  test('local behaviour observes the preceding bulk system commit in a real fixed tick', () {
    final behaviour = DoorBehaviour(player: player, door: door)..requested = true;
    world.addSystem(behaviour.system);
    world.addSystem(poisonSystem());
    start(hp: .01);
    world.frame(1 / 60);
    expect(world.systemOrder, ['poison', 'door-interaction']);
    expect(hp(), 0);
    expect((world.save(door, doorType)['fields'] as Map)['open'], false);
  });

  test('dependency cycles and missing dependencies reject scheduling before a frame', () {
    world.addSystem(SystemSpec(id: 'a', after: ['b'], run: (_, _) {}));
    world.addSystem(SystemSpec(id: 'b', after: ['a'], run: (_, _) {}));
    expect(() => world.systemOrder, throwsStateError);
    final other = PrototypeWorld(library: library);
    try {
      other.addSystem(SystemSpec(id: 'a', after: ['missing'], run: (_, _) {}));
      expect(() => other.systemOrder, throwsStateError);
    } finally {
      other.dispose();
    }
  });

  test('sequential scheduling uses a stable ID tie-break independent of registration order', () {
    final order = <String>[];
    world.addSystem(SystemSpec(id: 'z', run: (_, _) => order.add('z')));
    world.addSystem(SystemSpec(id: 'a', run: (_, _) => order.add('a')));
    start();
    world.frame(1 / 60);
    expect(order, ['a', 'z']);
  });
}
