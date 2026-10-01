import 'dart:typed_data';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class _Script extends Behaviour {
  final List<String> events = [];
  @override
  void onEnable() => events.add('enable');
  @override
  void onDisable() => events.add('disable');
  @override
  void onDestroy() => events.add('destroy');
}

void main() {
  late World world;
  setUp(() => world = World.create());
  tearDown(() => world.dispose());

  GameObject existing() {
    final go = world.spawn(name: 'original');
    world.step();
    world.drainEvents();
    return go;
  }

  test('ordinary objects share pending identity, data and references with the journal', () {
    final (target, owner) = world.edit(() {
      final target = world.spawn(
        name: 'target',
        transform: const Pose(position: Vec3(4, 0, 0)),
      );
      final owner = world.spawn(name: 'owner', parent: target);
      owner.set(Follow(target: target, offset: const Vec3(1, 2, 3)));
      owner.set(ThirdPersonCamera(target: target));
      owner.set(Skin(skeleton: owner));
      expect(owner.get<Follow>()!.target, same(target));
      expect(owner.get<ThirdPersonCamera>()!.target, same(target));
      expect(owner.get<Skin>()!.skeleton, same(owner));
      expect(owner.parent, same(target));
      expect(target.children, [owner]);
      expect(target.findChild('owner'), same(owner));
      expect(target.findDescendant('owner'), same(owner));
      expect(owner.transform.position, const Vec3(4, 0, 0));
      expect(owner.worldTransform!.position, const Vec3(4, 0, 0));
      expect(world.find('owner'), same(owner));
      expect(world.findAll('owner'), [owner]);
      expect(world.findObjectsWith<Follow>(), [owner]);
      expect(world.findObjectsWith<Name>(), [target, owner]);
      expect(world.objects, [target, owner]);
      expect(owner.isAlive, isTrue);
      expect(owner.isPending, isTrue);
      expect(owner.components.whereType<Follow>().single.target, same(target));
      owner.component<Follow>()!.value = const Follow();
      expect(owner.get<Follow>()!.target, isNull);
      return (target, owner);
    });
    expect(target.isPending, isFalse);
    expect(owner.isPending, isFalse);
    expect(world.byEntity(owner.entity), same(owner));
    expect(world.find('owner'), same(owner));
    expect(owner.get<ThirdPersonCamera>()!.target, same(target));
    expect(owner.get<Skin>()!.skeleton, same(owner));
  });

  test('commit invalidates cached names, components, hierarchy and snapshot-backed poses', () {
    final root = existing();
    final child = world.spawn(name: 'child', parent: root);
    root.set(const Layer(layer: 1));
    root.localTransform;
    child.localTransform;
    world.step();
    world.drainEvents();
    world.step();
    world.drainEvents();
    expect(root.get<Layer>()!.layer, 1);
    expect(world.find('original'), root);
    expect(child.parent, root);
    expect(child.worldTransform!.position, Vec3.zero);
    final native = world.native;
    final snapshot = Uint8List.fromList(native.snapshot.bytes);
    world.edit(() {
      root.name = 'renamed';
      root.set(const Layer(layer: 7));
      root.setLocalPosition(5, 0, 0);
      child.setLocalScale(2, 2, 2);
      expect(world.find('original'), isNull);
      expect(world.find('renamed'), root);
      expect(root.get<Name>()!.value, 'renamed');
      expect(world.nameOf(root), 'renamed');
      expect(child.transform.position, const Vec3(5, 0, 0));
    });
    expect(native.snapshot.bytes, snapshot, reason: 'completed snapshot stays immutable');
    expect(root.name, 'renamed');
    expect(root.get<Name>()!.value, 'renamed');
    expect(root.get<Layer>()!.layer, 7);
    expect(world.find('original'), isNull);
    expect(world.find('renamed'), root);
    expect(root.localTransform.position, const Vec3(5, 0, 0));
    expect(child.worldTransform!.position, const Vec3(5, 0, 0));
    expect(child.localTransform.scale, const Vec3(2, 2, 2));
    // Legacy partial writes must seed from the committed edit, not the old snapshot.
    root.setLocalScale(3, 3, 3);
    expect(root.localTransform.position, const Vec3(5, 0, 0));
    world.step();
    world.drainEvents();
    expect(root.localTransform.position, const Vec3(5, 0, 0));
    expect(root.localTransform.scale, const Vec3(3, 3, 3));
  });

  test('callback failure preserves caches and expires pending objects', () {
    final go = existing();
    go.get<Layer>();
    go.localTransform;
    world.find('original');
    late GameObject pending;
    expect(
      () => world.edit(() {
        go.name = 'discarded';
        go.set(const Layer(layer: 5));
        go.setLocalPosition(9, 0, 0);
        pending = world.spawn(parent: go);
        throw StateError('rollback');
      }),
      throwsStateError,
    );
    expect(go.name, 'original');
    expect(go.has<Layer>(), isFalse);
    expect(go.localTransform.position, Vec3.zero);
    expect(world.find('original'), go);
    expect(pending.isAlive, isFalse);
    expect(() => pending.set(const Layer()), throwsStateError);
    expect(go.children, isEmpty);
    world.edit(() => go.name = 'works again');
    expect(go.name, 'works again');
  });

  test('caught validation and final reference failures still abort all SDK edits', () {
    final go = existing();
    expect(
      () => world.edit(() {
        go.name = 'discarded';
        try {
          go.set(const AudioSource(volume: -1));
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(go.name, 'original');
    late GameObject target;
    expect(
      () => world.edit(() {
        target = world.spawn();
        go.set(Follow(target: target));
        target.destroy();
      }),
      throwsStateError,
    );
    expect(go.has<Follow>(), isFalse);
    expect(target.isAlive, isFalse);
  });

  test('reparent, activity and subtree destruction use staged hierarchy', () {
    final root = world.edit(() => world.spawn(name: 'root'));
    final (child, survivor) = world.edit(
      () => (world.spawn(name: 'child', parent: root), world.spawn(name: 'survivor', parent: root)),
    );
    world.edit(() {
      root.setActive(false);
      expect(child.activeSelf, isTrue);
      expect(child.activeInHierarchy, isFalse);
      child.set(const Layer(layer: 2));
      expect(world.findObjectsWith<Layer>(), isEmpty);
      expect(world.findObjectsWith<Layer>(includeInactive: true), [child]);
      survivor.setParent(null, keepWorld: true);
      expect(survivor.activeInHierarchy, isTrue);
      root.destroy();
      expect(root.isAlive, isFalse);
      expect(child.isAlive, isFalse);
      expect(world.find('child'), isNull);
      expect(world.objects, [survivor]);
    });
    expect(root.isAlive, isFalse);
    expect(child.isAlive, isFalse);
    expect(survivor.isAlive, isTrue);
    expect(survivor.parent, isNull);
    world.step();
    world.drainEvents();
    expect(world.objects, [survivor]);
  });

  test('cancelled spawns never publish and component removal reaches canonical storage', () {
    final go = existing();
    late GameObject cancelled;
    world.edit(() {
      cancelled = world.spawn();
      cancelled.destroy();
      go.set(const Layer(layer: 7));
      expect(go.has<Layer>(), isTrue);
      go.remove<Layer>();
      expect(go.has<Layer>(), isFalse);
      go.writeLocal(
        TransformWriter()
          ..setPosition(1, 2, 3)
          ..setScale(2, 2, 2),
      );
      go.setLocalPosition(7, 8, 9);
      expect(go.localTransform.scale, const Vec3(2, 2, 2));
    });
    expect(cancelled.isAlive, isFalse);
    expect(go.has<Layer>(), isFalse);
    expect(go.localTransform.position, const Vec3(7, 8, 9));
  });

  test('legacy simulation work is adopted before an explicit edit', () {
    final pending = world.spawn(name: 'legacy');
    world.edit(() {
      expect(pending.isPending, isFalse);
      expect(world.find('legacy'), same(pending));
    });
    pending.set(const Layer(layer: 7));
    world.edit(() => expect(pending.get<Layer>()!.layer, 7));
    world.step();
    world.drainEvents();
    expect(world.objects.length, 1);
  });

  test('nested edits and cross-world references fail stickily', () {
    final other = World.create();
    addTearDown(other.dispose);
    final foreign = other.edit(() => other.spawn());
    final go = existing();
    for (final operation in <void Function()>[
      () => world.edit(() {}),
      () => go.set(Follow(target: foreign)),
      () => other.spawn(),
      () => go.setParent(foreign),
      () => go.remove<Pose>(),
      () => go.layer = 'undeclared',
      () => go.addTag('undeclared'),
    ]) {
      expect(
        () => world.edit(() {
          go.name = 'discarded';
          try {
            operation();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(go.name, 'original');
    }
    expect(other.objects, [foreign]);
  });

  test('unsupported effects, lifecycle and frame operations cannot leak queued work', () {
    final go = existing();
    final script = go.addBehaviour(_Script());
    world.frame(0);
    for (final operation in <void Function()>[
      () => go.applyForce(Vec3.one),
      () => world.build(const ObjectSpec.prefab(AssetRef('00000000-0000-0000-0000-000000000000'))),
      () => world.audio.play(AssetRef.none, presentation: true),
      () => world.audio.open(),
      () => world.step(),
      () => world.frame(0),
      () => world.dispose(),
    ]) {
      expect(
        () => world.edit(() {
          go.name = 'discarded';
          try {
            operation();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(go.name, 'original');
      expect(script.enabled, isTrue);
      world.edit(() {}); // proves no legacy command leaked out of the failed edit
    }
    expect(go.behaviours, [script]);
    world.frame(0);
    expect(script.events, ['enable']);
  });

  test('committed destruction reaches behaviour lifecycle once at the next boundary', () {
    final go = existing();
    final script = go.addBehaviour(_Script());
    world.frame(0);
    world.edit(go.destroy);
    expect(go.isAlive, isFalse);
    expect(script.events, ['enable']);
    world.frame(0);
    expect(script.events, ['enable', 'disable', 'destroy']);
    world.frame(0);
    expect(script.events, ['enable', 'disable', 'destroy']);
  });

  test('activity lifecycle waits for commit and fires once at a safe boundary', () {
    final go = existing();
    final script = go.addBehaviour(_Script());
    world.frame(0);
    expect(
      () => world.edit(() {
        go.setActive(false);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    world.frame(0);
    expect(script.events, ['enable']);
    world.edit(() => go.setActive(false));
    expect(go.activeInHierarchy, isFalse);
    expect(script.events, ['enable']);
    world.frame(0);
    expect(script.events, ['enable', 'disable']);
    world.edit(() => go.setActive(true));
    world.frame(0);
    expect(script.events, ['enable', 'disable', 'enable']);
  });

  test('newly discovered live handles survive rollback without a staged name leak', () {
    final native = world.native;
    final committed = native.editGameplay((edit) => edit.spawn(name: 'native'));
    final entity = committed.resolve(committed.value)!;
    late GameObject found;
    expect(
      () => world.edit(() {
        found = world.attach(entity);
        found.name = 'staged';
        expect(found.name, 'staged');
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(found.isAlive, isTrue);
    expect(found.name, 'native');
    expect(world.attach(entity), same(found));
    found.name = 'legacy rename';
    expect(found.name, 'legacy rename');
  });

  test('caught singular world-space conversion aborts earlier valid edits', () {
    final root = existing();
    final child = world.edit(() => world.spawn(parent: root));
    expect(
      () => world.edit(() {
        root.name = 'discarded';
        root.setLocalScale(0, 1, 1);
        try {
          child.transform.position = Vec3.one;
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(root.name, 'original');
    expect(root.localTransform.scale, Vec3.one);
  });

  test('destroy removes pending subscriptions without dropping a surviving subscription', () {
    final dead = existing();
    final survivor = world.edit(() => world.spawn());
    dead.localTransform;
    survivor.localTransform;
    world.edit(dead.destroy);
    world.step();
    world.drainEvents();
    expect(world.native.snapshot.recordCount, 1);
    expect(survivor.isAlive, isTrue);
    expect(dead.isAlive, isFalse);
  });

  test('async continuations retain the expired edit and cannot enqueue later writes', () async {
    final go = existing();
    var continued = false;
    expect(
      () => world.edit(() async {
        go.name = 'discarded';
        await Future<void>.value();
        continued = true;
        world.spawn(name: 'must not leak');
      }),
      throwsStateError,
    );
    await Future<void>.delayed(Duration.zero);
    expect(continued, isTrue);
    expect(go.name, 'original');
    world.edit(() => expect(world.objects, [go]));
    world.step();
    world.drainEvents();
    expect(world.find('must not leak'), isNull);
  });
}
