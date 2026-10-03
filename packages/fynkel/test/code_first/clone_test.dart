// Code first, Tier 2 (docs/code-first.md, ADR-0026 §4): `GameObject.clone`,
// Unity's `Instantiate(gameObject)` — a live object and its subtree copied
// now, its native components, tags and layer, its scripts through their
// registered fields, their references re-pointed into the copy. Over the
// real native library.

import 'dart:io';

import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

class Spinner extends Behaviour {
  double speed = 1;
  int created = 0;

  @override
  void onCreate() => created++;
}

class Turret extends Behaviour {
  GameObject? target;
  Spinner? spinner;
  Ammo? ammo;
}

/// A data asset (ADR-0027): shared by the copy, never copied.
class Ammo extends DataAsset {
  int rounds = 10;
}

/// Unregistered: a clone leaves it out, with a warning.
class Loose extends Behaviour {}

/// What the generator emits for the two classes.
final spinnerType = BehaviourType(
  id: 'clone.Spinner',
  create: Spinner.new,
  fields: [
    BehaviourField(
      name: 'speed',
      type: 'number',
      get: (b) => (b as Spinner).speed,
      set: (b, v) => (b as Spinner).speed = (v as num).toDouble(),
    ),
  ],
);
final turretType = BehaviourType(
  id: 'clone.Turret',
  create: Turret.new,
  fields: [
    BehaviourField(
      name: 'target',
      type: 'gameObject',
      get: (b) => referenceJson((b as Turret).target),
      target: (b) => (b as Turret).target,
      set: (b, v) => (b as Turret).target = v as GameObject?,
    ),
    BehaviourField(
      name: 'spinner',
      type: 'behaviour',
      get: (b) => referenceJson((b as Turret).spinner),
      target: (b) => (b as Turret).spinner,
      set: (b, v) => (b as Turret).spinner = v as Spinner?,
    ),
    BehaviourField(
      name: 'ammo',
      type: 'data',
      dataType: 'clone.Ammo',
      get: (b) => dataJson((b as Turret).ammo),
      target: (b) => (b as Turret).ammo,
      set: (b, v) => (b as Turret).ammo = v as Ammo?,
    ),
  ],
);

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;
  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-code-first-clone-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)
      ..registry.registerAll([spinnerType, turretType])
      ..layersAndTags = const LayersAndTags(layers: ['default', 'props'], tags: ['enemy']);
    addTearDown(w.dispose);
    return w;
  }

  void tick(World w, [int n = 1]) {
    for (var i = 0; i < n; i++) {
      w.frame(1 / 60);
    }
  }

  test('a tree copied: components, tags, layer, children, scripts; references into the copy re-pointed', () {
    final w = fresh();
    final player = w.spawn(name: 'Player');
    final barrelSpinner = Spinner()..speed = 3;
    final turret = Turret();
    final ship = w.build(
      ObjectSpec(
        'Ship',
        transform: const Pose(position: Vec3(0, 5, 0)),
        tags: const ['enemy'],
        components: const [PointLight(range: 4)],
        behaviours: [turret],
        children: [
          ObjectSpec('Barrel', layer: 'props', behaviours: [barrelSpinner]),
          const ObjectSpec('Hatch', active: false),
        ],
      ),
    );
    final ammo = Ammo();
    turret
      ..target = player
      ..spinner = barrelSpinner
      ..ammo = ammo;
    tick(w);

    final copy = ship.clone(transform: const Pose(position: Vec3(10, 0, 0)));
    expect(copy, isNot(ship));
    expect(copy.isPending, isFalse, reason: 'made now');
    expect(copy.name, 'Ship');
    expect(copy.localTransform.position, const Vec3(10, 0, 0));
    expect(copy.get<PointLight>()!.range, 4);
    expect(copy.tags, {'enemy'});
    expect(copy.children.map((c) => c.name), ['Barrel', 'Hatch']);
    final barrel = copy.children.first;
    expect(barrel.layer, 'props');
    expect(copy.children.last.activeSelf, isFalse);

    final copiedTurret = copy.behaviour<Turret>()!;
    final copiedSpinner = barrel.behaviour<Spinner>()!;
    expect(copiedTurret, isNot(turret));
    expect(copiedSpinner.speed, 3);
    expect(copiedTurret.spinner, copiedSpinner, reason: 'a script inside the tree: its copy');
    expect(copiedTurret.target, player, reason: 'an object outside the tree: kept');
    expect(turret.spinner, barrelSpinner, reason: 'the source untouched');
    expect(copiedTurret.ammo, same(ammo), reason: 'a data asset is shared, not copied');

    tick(w);
    expect(copiedSpinner.created, 1, reason: 'the copy is created like any new script');
    expect(barrelSpinner.created, 1);
    expect(w.findObjectsOfType<Spinner>(), hasLength(2));
    expect(w.findAll('Ship'), hasLength(2));
  });

  test('placement: where the source is in the world, or its local pose under a parent', () {
    final w = fresh();
    final holder = w.spawn(
      name: 'Holder',
      transform: const Pose(position: Vec3(0, 10, 0)),
    );
    final gun = w.spawn(
      name: 'Gun',
      parent: holder,
      transform: const Pose(position: Vec3(1, 0, 0)),
    );
    final rack = w.spawn(
      name: 'Rack',
      transform: const Pose(position: Vec3(-5, 0, 0)),
    );
    tick(w, 2); // world poses come from the tick's snapshot

    final loose = gun.clone();
    expect(loose.parent, isNull);
    expect(loose.localTransform.position, const Vec3(1, 10, 0), reason: 'the source\'s world pose');

    final racked = gun.clone(parent: rack, name: 'Racked gun');
    expect(racked.parent, rack);
    expect(racked.name, 'Racked gun');
    expect(racked.localTransform.position, const Vec3(1, 0, 0), reason: 'the source\'s local pose');
    tick(w, 2);
    expect(racked.worldTransform!.position, const Vec3(-4, 0, 0));
  });

  test('a physics object copied falls like the original', () {
    final w = fresh();
    w.build(
      const ObjectSpec(
        'Floor',
        components: [Collider(size: Vec3(20, 0.5, 20), offset: Vec3(0, -0.5, 0))],
      ),
    );
    final crate = w.build(
      const ObjectSpec(
        'Crate',
        transform: Pose(position: Vec3(0, 3, 0)),
        components: [RigidBody(), Collider()],
      ),
    );
    tick(w);
    final second = crate.clone(transform: const Pose(position: Vec3(3, 6, 0)));
    tick(w, 240);
    expect(second.localTransform.position.y, closeTo(0.5, 0.05));
    expect(crate.localTransform.position.y, closeTo(0.5, 0.05));
  });

  test('a prefab instance copied, with its model and trigger', () {
    final w = fresh()..loadProject(fixture, cacheDir: cache);
    final coin = w.instantiate(w.assets.find('prefabs/coin')!, name: 'Coin');
    final copy = coin.clone(transform: const Pose(position: Vec3(2, 0, 0)));
    expect(copy.get<TriggerZone>(), isNotNull);
    expect(copy.get<MeshRenderer>(), coin.get<MeshRenderer>());
    expect(w.findObjectsWith<TriggerZone>(), hasLength(2));
    tick(w);
  });

  test('an unregistered script is left out with a warning; queued source is adopted before cloning', () {
    final w = fresh();
    final warnings = <LoadDiagnostic>[];
    w.onLoadDiagnostics = warnings.addAll;
    final o = w.spawn(name: 'Thing')
      ..addBehaviour(Loose())
      ..addBehaviour(Spinner()..speed = 7);
    expect(o.isPending, isFalse);
    final copy = o.clone();
    expect(o.isPending, isFalse, reason: 'the queued source is adopted before the clone transaction');
    expect(copy.behaviour<Loose>(), isNull);
    expect(copy.behaviour<Spinner>()!.speed, 7, reason: 'a script the game constructed, found by its class');
    expect(warnings.single.code, 'FYN-B0001');
    expect(warnings.single.message, contains('Loose'));
  });
}
