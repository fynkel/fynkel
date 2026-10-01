// Code-first specs reference each other (ADR-0036 §4): `spec.object` is
// the object a spec builds, held by a script field or a component value
// before anything exists; `world.build` makes those the built object, and
// the writer writes the spec's id. Over the real native library.

import 'dart:io';

import 'package:fynkel/authoring.dart';
import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Gate extends Behaviour {}

class Switch extends Behaviour {
  GameObject? door;
  Gate? gate;
  Vec3? doorAtStart;

  @override
  void start() => doorAtStart = door?.transform.position;
}

final gateType = BehaviourType(id: 'test.Gate', create: Gate.new, fields: const []);

/// What the generator emits for [Switch].
final switchType = BehaviourType(
  id: 'test.Switch',
  create: Switch.new,
  fields: [
    BehaviourField(
      name: 'door',
      type: 'gameObject',
      get: (b) => referenceJson((b as Switch).door),
      set: (b, v) => (b as Switch).door = v as GameObject?,
      target: (b) => (b as Switch).door,
    ),
    BehaviourField(
      name: 'gate',
      type: 'behaviour',
      get: (b) => referenceJson((b as Switch).gate),
      set: (b, v) => (b as Switch).gate = v as Gate?,
      target: (b) => (b as Switch).gate,
    ),
  ],
);

void registerBehaviours(BehaviourRegistry r) => r
  ..register(gateType)
  ..register(switchType);

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
    registerBehaviours(w.registry);
    addTearDown(w.dispose);
    return w;
  }

  test('a script field holds a spec\'s object before the build and the built object after', () {
    final w = fresh();
    final gate = Gate();
    final door = ObjectSpec(
      'Door',
      transform: const Pose(position: Vec3(0, 0, 5)),
      behaviours: [gate],
    );
    // The switch comes first: its field names an object not spawned yet.
    final pad = Switch()
      ..door = door.object
      ..gate = gate;
    final level = ObjectSpec(
      'Level',
      children: [
        ObjectSpec('Switch', behaviours: [pad]),
        door,
      ],
    );
    expect(pad.door!.isAlive, isFalse, reason: 'a stand-in until it is built');
    expect(() => pad.door!.transform.position, throwsStateError);

    w.build(level);
    final built = door.object;
    expect(!built.isPending && built.isAlive, isTrue, reason: 'the object this build spawned');
    expect(pad.door, same(built));
    expect(pad.gate, same(gate));
    expect(gate.gameObject, same(built));
    w.frame(1 / 60);
    expect(pad.doorAtStart, const Vec3(0, 0, 5), reason: 'set before the script starts');
  });

  test('a component value names a spec\'s object: Follow(target: hero.object)', () {
    final w = fresh();
    final hero = ObjectSpec('Hero', transform: const Pose(position: Vec3(3, 0, 0)));
    final root = w.build(
      ObjectSpec(
        'Level',
        children: [
          ObjectSpec('Camera', components: [Follow(target: hero.object)]),
          hero,
        ],
      ),
    );
    w
      ..frame(1 / 60)
      ..frame(1 / 60);
    final camera = root.findChild('Camera')!;
    expect(camera.get<Follow>()!.target, same(root.findChild('Hero')));
  });

  test(
    'a spec built before resolves; one never built, or built twice, is an error before anything spawns',
    () {
      final w = fresh();
      final door = ObjectSpec('Door', behaviours: [Gate()]);
      final pad = Switch()..door = door.object;
      final built = w.build(door);
      w.build(ObjectSpec('Switch', behaviours: [pad]));
      expect(pad.door, same(built));

      const lone = ObjectSpec('Lone');
      final orphan = Switch()..door = lone.object;
      expect(() => w.build(ObjectSpec('Switch 2', behaviours: [orphan])), throwsStateError);
      const twin = ObjectSpec('Twin');
      final both = Switch()..door = twin.object;
      expect(
        () => w.build(ObjectSpec('Pair', behaviours: [both], children: const [twin, twin])),
        throwsStateError,
        reason: 'a const spec listed twice builds two objects: which one?',
      );
      final loose = Switch()..gate = Gate();
      expect(
        () => w.build(ObjectSpec('Switch 3', behaviours: [loose])),
        throwsStateError,
        reason: 'the gate is on no object',
      );
      w.frame(1 / 60);
      expect(w.find('Switch 2') ?? w.find('Pair') ?? w.find('Switch 3'), isNull);
    },
  );

  test('the writer writes a spec\'s id for each reference, forward ones too', () {
    final writer = ProjectWriter(
      Directory('../../crates/fynkel_document/fixtures/project').absolute.path,
      registerBehaviours: registerBehaviours,
    );
    final gate = Gate();
    final door = ObjectSpec('Door', behaviours: [gate]);
    final hero = ObjectSpec('Hero');
    final (:entities, :roots) = writer.entitiesOf([
      ObjectSpec(
        'Switch',
        behaviours: [
          Switch()
            ..door = door.object
            ..gate = gate,
        ],
      ),
      ObjectSpec('Camera', components: [Follow(target: hero.object)]),
      door,
      hero,
    ]);
    final [switchId, cameraId, doorId, heroId] = roots;
    Map<String, Object?> data(String entity, String type) =>
        ((entities[entity]! as Map)['components']! as Map).values.cast<Map<String, Object?>>().firstWhere(
              (c) => c['type'] == type,
            )['data']!
            as Map<String, Object?>;
    final doorScript = ((entities[doorId]! as Map)['components']! as Map).entries
        .firstWhere((e) => (e.value as Map)['type'] == 'test.Gate')
        .key;
    expect(data(switchId, 'test.Switch'), {
      'door': {'entity': doorId},
      'gate': {'entity': doorId, 'component': doorScript},
    });
    expect(data(cameraId, 'fynkel.Follow')['target'], {'entity': heroId});

    expect(
      () => writer.entitiesOf([
        ObjectSpec('Switch', behaviours: [Switch()..door = const ObjectSpec('Elsewhere').object]),
      ]),
      throwsArgumentError,
      reason: 'a reference to a spec not in the document',
    );
  });
}
