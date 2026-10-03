import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

import 'code_first/clone_test.dart' as fixture;

class _Strict extends Behaviour {
  int value = 1;
}

class _LegacyReference extends Behaviour {
  GameObject? target;
}

class _CloneAndFail extends Behaviour {
  _CloneAndFail(this.source);
  final GameObject source;
  GameObject? copy;
  bool ran = false;
  @override
  void update(UpdateContext context) {
    if (ran) return;
    ran = true;
    copy = source.clone(name: 'aborted-copy');
    throw StateError('abort clone callback');
  }
}

void main() {
  late World world;
  setUp(() {
    world = World.create()..registry.registerAll([fixture.spinnerType, fixture.turretType]);
  });
  tearDown(() => world.dispose());

  test('clone sees pending hierarchy, native references and staged script ownership', () {
    late GameObject source, copy, external;
    late fixture.Spinner copiedSpinner;
    world.edit(() {
      external = world.spawn(name: 'external');
      source = world.spawn(name: 'source')..set(const PointLight(range: 2));
      final child = world.spawn(name: 'child', parent: source);
      final removed = child.addBehaviour(fixture.Spinner());
      removed.destroy();
      final spinner = child.addBehaviour(fixture.Spinner()..speed = 4);
      spinner.enabled = false;
      source.addBehaviour(
        fixture.Turret()
          ..target = child
          ..spinner = spinner,
      );
      source.set(Follow(target: child));
      child.set(Follow(target: external));
      source.set(const PointLight(range: 8));
      copy = source.clone(name: 'copy');
      final copiedChild = copy.children.single;
      copiedSpinner = copiedChild.behaviour<fixture.Spinner>()!;
      expect(copy.get<PointLight>()!.range, 8);
      expect(copy.get<Follow>()!.target, same(copiedChild));
      expect(copiedChild.get<Follow>()!.target, same(external));
      expect(copiedChild.behaviours, hasLength(1));
      expect(copiedSpinner.speed, 4);
      expect(copiedSpinner.enabled, isFalse);
      expect(copiedSpinner.created, 0);
      expect(copy.behaviour<fixture.Turret>()!.spinner, same(copiedSpinner));
      expect(copy.behaviour<fixture.Turret>()!.target, same(copiedChild));
      expect(copy.isPending, isTrue);
    });
    expect(copy.isPending, isFalse);
    expect(copiedSpinner.created, 0);
    expect(copy.get<Follow>()!.target, same(copy.children.single));
    world.frame(0);
    expect(copiedSpinner.created, 1);
    expect(copiedSpinner.isActive, isFalse);
    expect(source.children.single, isNot(same(copy.children.single)));
  });

  test('aborted clone discards all copied entities, script attachments and deferred warnings', () {
    final source = world.edit(
      () => world.spawn(name: 'source')
        ..addBehaviour(fixture.Loose())
        ..addBehaviour(fixture.Spinner()),
    );
    final warnings = <LoadDiagnostic>[];
    world.onLoadDiagnostics = warnings.addAll;
    late GameObject discarded;
    late fixture.Spinner script;
    expect(
      () => world.edit(() {
        discarded = source.clone(name: 'discarded');
        script = discarded.behaviour<fixture.Spinner>()!;
        expect(warnings, isEmpty);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(discarded.isAlive, isFalse);
    expect(script.isAlive, isFalse);
    expect(script.created, 0);
    expect(world.find('discarded'), isNull);
    expect(warnings, isEmpty);
    final copy = world.edit(() => source.clone(name: 'copy'));
    expect(copy.behaviour<fixture.Loose>(), isNull);
    expect(warnings.single.code, 'FYN-B0001');
  });

  test('field copy failure is sticky and rolls back preceding writes and the native subtree', () {
    world.registry.register(
      BehaviourType(
        id: 'clone.Strict',
        create: _Strict.new,
        fields: [
          BehaviourField(
            name: 'value',
            type: 'number',
            get: (b) => (b as _Strict).value,
            set: (b, value) => throw StateError('copy refused'),
          ),
        ],
      ),
    );
    final source = world.edit(() => world.spawn(name: 'source')..addBehaviour(_Strict()));
    expect(
      () => world.edit(() {
        source.name = 'discarded-name';
        try {
          source.clone(name: 'bad-copy');
        } catch (_) {}
      }),
      throwsStateError,
    );
    expect(source.name, 'source');
    expect(world.find('bad-copy'), isNull);
    expect(world.findObjectsOfType<_Strict>(), hasLength(1));
  });

  test('placement reads staged ancestor transforms and explicit parent keeps local pose', () {
    world.edit(() {
      final parent = world.spawn(transform: const Pose(position: Vec3(1, 0, 0)));
      final source = world.spawn(
        parent: parent,
        transform: const Pose(position: Vec3(2, 0, 0)),
      );
      parent.setLocalTransform(const Pose(position: Vec3(10, 0, 0)));
      final detached = source.clone();
      expect(detached.parent, isNull);
      expect(detached.localTransform.position, const Vec3(12, 0, 0));
      final underParent = source.clone(parent: parent);
      expect(underParent.parent, same(parent));
      expect(underParent.localTransform.position, const Vec3(2, 0, 0));
      final explicit = source.clone(transform: const Pose(position: Vec3(7, 0, 0)));
      expect(explicit.localTransform.position, const Vec3(7, 0, 0));
    });
  });

  test('a copied removed script reference becomes null and external script references remain shared', () {
    world.edit(() {
      final external = world.spawn().addBehaviour(fixture.Spinner());
      final source = world.spawn(name: 'source');
      final removed = source.addBehaviour(fixture.Spinner());
      final turret = source.addBehaviour(fixture.Turret()..spinner = removed);
      removed.destroy();
      expect(source.clone().behaviour<fixture.Turret>()!.spinner, isNull);
      turret.spinner = external;
      expect(source.clone().behaviour<fixture.Turret>()!.spinner, same(external));
    });
  });
  test('a clone made by a failed automatic callback publishes no object or lifecycle', () {
    final source = world.edit(() => world.spawn()..addBehaviour(fixture.Spinner()));
    final script = world.edit(() => world.spawn().addBehaviour(_CloneAndFail(source)));
    final errors = <BehaviourError>[];
    world.onBehaviourError = errors.add;
    world.frame(0);
    expect(errors.single.callback, 'update');
    expect(script.copy!.isAlive, isFalse);
    expect(world.find('aborted-copy'), isNull);
    world.frame(0);
    expect(world.findObjectsOfType<fixture.Spinner>(), hasLength(1));
    expect(errors, hasLength(1));
  });

  test('expired and foreign script object references fail atomically', () {
    final foreignWorld = World.create();
    addTearDown(foreignWorld.dispose);
    final foreign = foreignWorld.edit(() => foreignWorld.spawn());
    final dead = world.edit(() => world.spawn());
    world.edit(dead.destroy);
    final turret = fixture.Turret();
    final source = world.edit(() => world.spawn()..addBehaviour(turret));
    for (final target in [dead, foreign]) {
      turret.target = target;
      expect(() => world.edit(() => source.clone(name: 'bad-reference')), throwsStateError);
      expect(world.find('bad-reference'), isNull);
      expect(world.findObjectsOfType<fixture.Turret>(), [turret]);
    }
  });
  test('old nonnull reference descriptors reject instead of publishing an incomplete script', () {
    world.registry.register(
      BehaviourType(
        id: 'clone.LegacyReference',
        create: _LegacyReference.new,
        fields: [
          BehaviourField(
            name: 'target',
            type: 'gameObject',
            get: (b) => referenceJson((b as _LegacyReference).target),
            set: (b, value) => (b as _LegacyReference).target = value as GameObject?,
          ),
        ],
      ),
    );
    final source = world.edit(() {
      final go = world.spawn();
      go.addBehaviour(_LegacyReference()..target = go);
      return go;
    });
    expect(() => source.clone(name: 'incomplete'), throwsStateError);
    expect(world.find('incomplete'), isNull);
  });

  test('user factories cannot change the already captured script membership or enabled state', () {
    var cloning = false;
    late GameObject child;
    late fixture.Spinner spinner;
    world.registry.register(
      BehaviourType(
        id: 'clone.Factory',
        create: () {
          if (cloning) {
            spinner.enabled = false;
            child.addBehaviour(fixture.Loose());
          }
          return _Strict();
        },
      ),
    );
    final source = world.edit(() {
      final go = world.spawn();
      go.addBehaviour(world.registry.instantiate('clone.Factory'));
      child = world.spawn(parent: go);
      spinner = child.addBehaviour(world.registry.instantiate('clone.Spinner') as fixture.Spinner);
      return go;
    });
    final warnings = <LoadDiagnostic>[];
    world.onLoadDiagnostics = warnings.addAll;
    cloning = true;
    final copy = world.edit(() => source.clone());
    expect(spinner.enabled, isFalse, reason: 'the factory mutation participates in its edit');
    expect(copy.children.single.behaviour<fixture.Spinner>()!.enabled, isTrue);
    expect(copy.children.single.behaviours, hasLength(1));
    expect(child.behaviour<fixture.Loose>(), isNotNull);
    expect(warnings, isEmpty, reason: 'later additions are outside the captured clone');
  });
}
