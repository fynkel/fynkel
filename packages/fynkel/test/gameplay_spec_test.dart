import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

class _Script extends Behaviour {
  int creates = 0;
  GameObject? target;
  _Script? peer;
  NativeComponent<RigidBody>? body;
  @override
  void onCreate() => creates++;
}

void main() {
  late World world;
  setUp(() => world = World.create());
  tearDown(() => world.dispose());

  test('tree construction resolves forward component references and defers callbacks', () {
    final script = _Script();
    final target = ObjectSpec('target', transform: const Pose(position: Vec3(3, 0, 0)));
    final source = ObjectSpec(
      'source',
      components: [Follow(target: target.object)],
      behaviours: [script],
    );
    final spec = ObjectSpec('root', children: [source, target]);
    final root = world.edit(() {
      final root = world.build(spec);
      expect(spec.object, same(root));
      expect(source.object.get<Follow>()!.target, same(target.object));
      expect(target.object.parent, same(root));
      expect(script.gameObject, same(source.object));
      expect(script.creates, 0);
      return root;
    });
    expect(spec.object, same(root));
    expect(source.object.get<Follow>()!.target, same(target.object));
    expect(target.object.transform.position.x, 3);
    world.frame(0);
    expect(script.creates, 1);
  });

  test('abort preserves prior spec binding and leaves scripts reusable', () {
    final spec = ObjectSpec('repeat');
    final original = world.edit(() => world.build(spec));
    late GameObject canceled;
    final script = _Script();
    final fresh = ObjectSpec('fresh', behaviours: [script]);
    final placeholder = fresh.object;
    expect(
      () => world.edit(() {
        canceled = world.build(spec);
        world.build(fresh);
        expect(spec.object, same(canceled));
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(spec.object, same(original));
    expect(fresh.object, same(placeholder));
    expect(canceled.isAlive, isFalse);
    expect(script.creates, 0);
    world.edit(() => world.build(fresh));
    world.frame(0);
    expect(script.creates, 1);
  });

  test('invalid final reference rolls back entire construction', () {
    final target = ObjectSpec('target');
    final source = ObjectSpec('source', components: [Follow(target: target.object)]);
    final spec = ObjectSpec('root', children: [source, target]);
    final placeholder = spec.object;
    expect(
      () => world.edit(() {
        world.build(spec);
        target.object.destroy();
      }),
      throwsStateError,
    );
    expect(spec.object, same(placeholder));
    expect(world.find('root'), isNull);
  });

  test('cycles, duplicate nodes and duplicate builds fail stickily', () {
    final children = <ObjectSpec>[];
    final cycle = ObjectSpec('cycle', children: children);
    children.add(cycle);
    final child = ObjectSpec('child');
    for (final spec in [
      cycle,
      ObjectSpec('duplicate', children: [child, child]),
    ]) {
      expect(
        () => world.edit(() {
          world.spawn(name: 'earlier');
          try {
            world.build(spec);
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(world.find('earlier'), isNull);
    }
    expect(
      () => world.edit(() {
        world.build(child);
        world.build(child);
      }),
      throwsStateError,
    );
    expect(world.find('child'), isNull);
  });

  void registerReferences() {
    world.registry.register(
      BehaviourType(
        id: 'test.Script',
        create: _Script.new,
        fields: [
          BehaviourField(
            name: 'target',
            type: 'gameObject',
            get: (_) => null,
            target: (b) => (b as _Script).target,
            set: (_, _) => throw StateError('must not rewrite existing script fields'),
          ),
          BehaviourField(
            name: 'peer',
            type: 'behaviour',
            get: (_) => null,
            target: (b) => (b as _Script).peer,
            set: (_, _) => throw StateError('must not rewrite script references'),
          ),
          BehaviourField(
            name: 'body',
            type: 'component',
            get: (_) => null,
            target: (b) => (b as _Script).body,
            set: (_, _) => throw StateError('must not rewrite component references'),
          ),
        ],
      ),
    );
  }

  test('script placeholders become canonical handles without invoking setters', () {
    registerReferences();
    final target = ObjectSpec('target');
    final placeholder = target.object;
    final script = _Script()..target = placeholder;
    final rootSpec = ObjectSpec('root', behaviours: [script], children: [target]);
    world.edit(() {
      final root = world.build(rootSpec);
      expect(script.target, same(target.object));
      expect(placeholder.world, same(world));
      expect(placeholder.isAlive, isTrue);
      expect(placeholder.parent, same(root));
      placeholder.name = 'renamed';
      placeholder.transform.position = const Vec3(4, 0, 0);
      expect(placeholder.name, 'renamed');
      expect(world.find('renamed'), same(placeholder));
      script.enabled = false;
      expect(script.enabled, isFalse);
    });
    expect(world.find('renamed'), same(placeholder));
    expect(placeholder.transform.position.x, 4);
    expect(script.target, same(placeholder));
    world.frame(0);
    expect(script.creates, 1);
    expect(script.isActive, isFalse);
    world.edit(() => script.enabled = true);
    world.frame(0);
    expect(script.creates, 1);
  });

  test('aborted script references and placeholders can retry without field changes', () {
    registerReferences();
    final target = ObjectSpec('target');
    final placeholder = target.object;
    final script = _Script()..target = placeholder;
    final spec = ObjectSpec('root', behaviours: [script], children: [target]);
    expect(
      () => world.edit(() {
        world.build(spec);
        placeholder.name = 'discard';
        script.enabled = false;
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(script.target, same(placeholder));
    expect(placeholder.isAlive, isFalse);
    expect(placeholder.name, 'target');
    expect(() => placeholder.world, throwsStateError);
    expect(() => script.gameObject, throwsStateError);
    expect(script.enabled, isTrue);
    expect(world.find('root'), isNull);
    world.edit(() => world.build(spec));
    expect(script.target, same(target.object));
    world.frame(0);
    expect(script.creates, 1);
  });

  test('rebuilding a description never retargets existing object references', () {
    final spec = ObjectSpec('repeat');
    final first = spec.object;
    expect(world.edit(() => world.build(spec)), same(first));
    final second = world.edit(() => world.build(spec));
    expect(spec.object, same(second));
    expect(second, isNot(same(first)));
    world.edit(() => first.name = 'first');
    expect(world.find('first'), same(first));
    expect(second.name, 'repeat');
  });

  test('legacy build also consumes placeholder before script callbacks', () {
    registerReferences();
    final target = ObjectSpec('target');
    final placeholder = target.object;
    final script = _Script()..target = placeholder;
    world.build(ObjectSpec('root', behaviours: [script], children: [target]));
    expect(target.object, same(placeholder));
    expect(script.target!.world, same(world));
    world.frame(1 / 60);
    expect(script.creates, 1);
  });

  test('canceling first build publishes a dead handle without retargeting it later', () {
    final spec = ObjectSpec('cancel');
    final placeholder = spec.object;
    world.edit(() {
      final built = world.build(spec);
      expect(built, same(placeholder));
      built.destroy();
      expect(placeholder.isAlive, isFalse);
    });
    expect(placeholder.isAlive, isFalse);
    final next = world.edit(() => world.build(spec));
    expect(next, isNot(same(placeholder)));
    expect(placeholder.isAlive, isFalse);
  });

  test('script fields can reference a spec staged by an earlier build in the edit', () {
    registerReferences();
    final target = ObjectSpec('earlier');
    final placeholder = target.object;
    final script = _Script()..target = placeholder;
    world.edit(() {
      world.build(target);
      world.build(ObjectSpec('later', behaviours: [script]));
      expect(script.target, same(world.find('earlier')));
      script.destroy();
      expect(script.isAlive, isFalse);
    });
    expect(placeholder.isAlive, isTrue);
    expect(() => script.gameObject, throwsStateError);
    expect(script.creates, 0);
  });

  test('invalid script reference rejects earlier writes even when caught', () {
    registerReferences();
    final other = World.create();
    addTearDown(other.dispose);
    final foreign = other.edit(() => other.spawn());
    final missing = ObjectSpec('missing').object;
    final dead = world.edit(() => world.spawn());
    world.edit(dead.destroy);
    for (final target in [foreign, missing, dead]) {
      final script = _Script()..target = target;
      expect(
        () => world.edit(() {
          world.spawn(name: 'discard');
          try {
            world.build(ObjectSpec('invalid', behaviours: [script]));
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(world.find('discard'), isNull);
      expect(world.find('invalid'), isNull);
      expect(() => script.gameObject, throwsStateError);
      expect(script.target, same(target));
    }
  });

  test('script references support forward attachments and earlier staged attachments', () {
    registerReferences();
    final peer = _Script();
    final source = _Script()..peer = peer;
    final later = _Script()..peer = peer;
    world.edit(() {
      world.build(
        ObjectSpec(
          'source',
          behaviours: [source],
          children: [
            ObjectSpec('peer', behaviours: [peer]),
          ],
        ),
      );
      world.build(ObjectSpec('later', behaviours: [later]));
      expect(source.peer!.gameObject, same(world.find('peer')));
      expect(later.peer, same(source.peer));
    });
    world.frame(0);
    expect(peer.creates, 1);
    expect(source.creates, 1);
    expect(later.creates, 1);
  });

  test('native component script references reject foreign owners atomically', () {
    registerReferences();
    final other = World.create();
    addTearDown(other.dispose);
    final foreign = other.edit(() => other.spawn()..set(const RigidBody()));
    final script = _Script()..body = foreign.getComponent<NativeComponent<RigidBody>>();
    expect(script.body, isNotNull);
    expect(
      () => world.edit(() => world.build(ObjectSpec('invalid', behaviours: [script]))),
      throwsStateError,
    );
    expect(world.find('invalid'), isNull);
    expect(() => script.gameObject, throwsStateError);
  });

  test('unpublished placeholder entity access fails stickily and can retry', () {
    final spec = ObjectSpec('pending');
    final handle = spec.object;
    expect(
      () => world.edit(() {
        world.build(spec);
        try {
          handle.entity;
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(handle.isAlive, isFalse);
    expect(world.find('pending'), isNull);
    expect(world.edit(() => world.build(spec)), same(handle));
    expect(world.attach(handle.entity), same(handle));
  });

  test('cooked prefab rejection rolls back preceding writes', () {
    expect(
      () => world.edit(() {
        world.spawn(name: 'earlier');
        try {
          world.build(const ObjectSpec.prefab(AssetRef('00000000-0000-0000-0000-000000000000')));
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(world.find('earlier'), isNull);
  });

  test('primitive and inactive tree use ordinary journal component and activity paths', () {
    final script = _Script();
    final child = ObjectSpec('child', behaviours: [script]);
    final spec = ObjectSpec('root', active: false, children: [child]);
    world.edit(() {
      world.build(spec);
      final primitive = world.createPrimitive(Primitive.cube, parent: child.object);
      expect(primitive.get<MeshRenderer>(), isNotNull);
      expect(primitive.get<Collider>(), isNotNull);
      expect(primitive.activeInHierarchy, isFalse);
    });
    world.frame(0);
    expect(script.creates, 0);
    world.edit(() => spec.object.setActive(true));
    world.frame(0);
    expect(script.creates, 1);
  });
}
