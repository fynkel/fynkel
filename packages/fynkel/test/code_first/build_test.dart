// Code first, Tier 3 (docs/code-first.md, ADR-0026 §2): an object tree
// described in code — `ObjectSpec` — and spawned by `world.build`; a Dart
// function returning a spec is a code prefab. Over the real native library.

import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

class Spinner extends Behaviour {
  double speed = 1;
  GameObject? seenParent;
  int created = 0;

  @override
  void onCreate() {
    created++;
    seenParent = gameObject.parent;
  }
}

/// A code prefab: a crate that falls, one per call.
ObjectSpec crate(String name, Vec3 at) => ObjectSpec(
  name,
  transform: Pose(position: at),
  components: const [RigidBody(), Collider()],
  behaviours: [Spinner()..speed = at.x],
);

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-code-first-build-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh({bool project = false}) {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    if (project) w.loadProject(fixture, cacheDir: cache);
    w.layersAndTags = const LayersAndTags(layers: ['default', 'props'], tags: ['pickup', 'enemy']);
    return w;
  }

  void tick(World w, [int n = 1]) {
    for (var i = 0; i < n; i++) {
      w.frame(1 / 60);
    }
  }

  test('a code prefab, built three times, gives three crates that fall onto the floor', () {
    final w = fresh();
    w.build(
      const ObjectSpec(
        'Floor',
        components: [Collider(size: Vec3(20, 0.5, 20), offset: Vec3(0, -0.5, 0))],
      ),
    );
    final crates = [
      for (var i = 0; i < 3; i++) w.build(crate('Crate $i', Vec3(i * 2.0, 2 + i.toDouble(), 0))),
    ];
    expect(crates.map((c) => c.name), ['Crate 0', 'Crate 1', 'Crate 2']);
    expect(crates[2].behaviour<Spinner>()!.speed, 4, reason: 'the instance the spec held');
    tick(w, 240);
    for (final c in crates) {
      expect(c.localTransform.position.y, closeTo(0.5, 0.05), reason: '${c.name} rests on the floor');
      expect(c.behaviour<Spinner>()!.created, 1);
    }
  });

  test('a tree: children in order, behaviours see their parent, usable the frame it is built', () {
    final w = fresh();
    final spinner = Spinner();
    final root = w.build(
      ObjectSpec(
        'Ship',
        transform: const Pose(position: Vec3(0, 10, 0)),
        children: [
          ObjectSpec('Hull', components: const [PointLight(range: 3)]),
          ObjectSpec(
            'Turret',
            transform: const Pose(position: Vec3(0, 1, 0)),
            behaviours: [spinner],
            children: const [
              ObjectSpec('Barrel', layer: 'props', tags: ['enemy']),
            ],
          ),
        ],
      ),
      transform: const Pose(position: Vec3(5, 0, 0)),
    );
    // Before any tick.
    expect(root.children.map((c) => c.name), ['Hull', 'Turret']);
    expect(root.getInChildren<Spinner>(), spinner);
    expect(root.getInChildren<PointLight>()!.range, 3);
    expect(
      root.localTransform.position,
      const Vec3(5, 0, 0),
      reason: 'build(transform:) replaces the spec\'s',
    );

    tick(w);
    final barrel = w.find('Barrel')!;
    expect(barrel.parent!.name, 'Turret');
    expect(barrel.layer, 'props');
    expect(barrel.tags, {'enemy'});
    tick(w); // world poses come from the tick's snapshot
    expect(w.find('Barrel')!.worldTransform!.position, const Vec3(5, 1, 0));
    expect(spinner.seenParent, root, reason: 'onCreate runs with the tree in place');
    expect(w.findObjectsWith<PointLight>().single.name, 'Hull');
  });

  test('an inactive spec builds switched off, its subtree with it', () {
    final w = fresh();
    final spinner = Spinner();
    w.build(
      ObjectSpec(
        'Off',
        active: false,
        children: [
          ObjectSpec('Child', behaviours: [spinner]),
        ],
      ),
    );
    tick(w, 2);
    expect(w.find('Off')!.activeSelf, isFalse);
    expect(w.find('Child')!.activeInHierarchy, isFalse);
    expect(spinner.created, 0, reason: 'an inactive script is not created yet');
    expect(w.findObjectsOfType<Spinner>(), isEmpty);
    expect(w.findObjectsOfType<Spinner>(includeInactive: true), [spinner]);
  });

  test(
    'prefab instances: overrides on the root, extra children, and under an object built in the same call',
    () {
      final w = fresh(project: true);
      final coinRef = w.assets.find('prefabs/coin')!;
      final row = w.build(
        ObjectSpec(
          'Coins',
          children: [
            for (var i = 0; i < 3; i++)
              ObjectSpec.prefab(
                coinRef,
                name: 'Coin $i',
                transform: Pose(position: Vec3(i.toDouble(), 0, 0)),
                tags: const ['pickup'],
                components: const [PointLight(range: 2)],
                children: [ObjectSpec('Glow')],
              ),
          ],
        ),
      );
      expect(row.children.map((c) => c.name), [
        'Coin 0',
        'Coin 1',
        'Coin 2',
      ], reason: 'the prefabs moved under the new row');
      tick(w);
      final coin = w.find('Coin 2')!;
      expect(coin.parent, row);
      expect(coin.localTransform.position, const Vec3(2, 0, 0), reason: 'local to the row');
      expect(coin.get<TriggerZone>(), isNotNull, reason: 'the prefab\'s own components');
      expect(coin.get<PointLight>()!.range, 2, reason: 'the spec\'s added on top');
      expect(coin.hasTag('pickup'), isTrue);
      expect(coin.children.map((c) => c.name), ['Glow']);
      expect(w.findObjectsWith<TriggerZone>(), hasLength(3));

      // A prefab spec at the top, under a spawned parent.
      final top = w.build(ObjectSpec.prefab(coinRef), parent: row);
      expect(top.name, 'Coin', reason: 'the prefab\'s name when the spec gives none');
      expect(top.parent, row);
    },
  );

  test('a mistake throws before anything is built', () {
    final w = fresh();
    final used = Spinner();
    w.spawn(name: 'holder').addBehaviour(used);
    expect(
      () => w.build(
        ObjectSpec(
          'A',
          children: [
            ObjectSpec('B', behaviours: [used]),
          ],
        ),
      ),
      throwsA(isA<StateError>().having((e) => e.message, 'message', contains('already attached'))),
    );
    final twice = Spinner();
    expect(
      () => w.build(
        ObjectSpec(
          'A',
          behaviours: [twice],
          children: [
            ObjectSpec('B', behaviours: [twice]),
          ],
        ),
      ),
      throwsStateError,
    );
    expect(
      () => w.build(
        const ObjectSpec(
          'A',
          children: [
            ObjectSpec('B', tags: ['nope']),
          ],
        ),
      ),
      throwsStateError,
    );
    expect(() => w.build(const ObjectSpec('A', layer: 'nope')), throwsStateError);
    expect(
      () => w.build(ObjectSpec.prefab(const AssetRef('c0000000-0000-4000-8000-000000000042', sub: 'mesh/0'))),
      throwsArgumentError,
    );
    tick(w);
    expect(w.find('A'), isNull);
    expect(w.find('B'), isNull);

    // A spec with behaviours builds once; a function builds it again.
    final spec = crate('Once', Vec3.zero);
    w.build(spec);
    expect(() => w.build(spec), throwsStateError);
    w.build(crate('Again', Vec3.zero));
  });

  // A frame's commands and their string tail cross the batch's capacity
  // together: the tail grows the buffer as it is finalized, and the world
  // sent the old, shorter one ("Too few elements"; code_only's Collect All
  // Coins, 36 sparks in a frame). One more object each frame crosses it.
  test('a frame whose names grow the command buffer as it is sent', () {
    final w = fresh();
    for (var n = 1; n <= 64; n++) {
      final burst = w.build(
        ObjectSpec('Burst $n', children: [for (var i = 0; i < n; i++) ObjectSpec('Spark $i of burst $n')]),
      );
      tick(w);
      expect(burst.children, hasLength(n));
      burst.destroy();
    }
  });
}
