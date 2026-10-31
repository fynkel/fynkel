// Stage 14's API pass over the core: finding objects and components as
// Unity does (docs/stage-14-findings.md, C3, C4, C5 and C11).

import 'dart:async';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Health extends Behaviour {}

class Level extends GameObject {}

/// An object whose load takes until [done] completes, and sets [hp].
class Enemy extends GameObject {
  Enemy(this.done);

  final Completer<void> done;
  late int hp;

  @override
  Future<void> onLoad() async {
    super.onLoad();
    await done.future;
    hp = 10;
  }
}

void main() {
  late World world;
  setUp(() => world = World());
  tearDown(() => world.dispose());

  test('world.all, only and first leave out objects still loading, unless includeInactive (C3)', () async {
    final done = Completer<void>();
    final enemy = world.add(Enemy(done));
    final health = enemy.add(GameObject(components: [Health()])).get<Health>()!;
    expect(world.all<Enemy>(), isEmpty);
    expect(world.first<Enemy>(), isNull);
    expect(() => world.only<Enemy>(), throwsStateError);
    expect(world.all<Health>(), isEmpty, reason: 'a component below it too');
    expect(world.all<Enemy>(includeInactive: true), [enemy]);
    done.complete();
    await enemy.loaded;
    expect(world.only<Enemy>().hp, 10);
    expect(world.all<Health>(), [health]);
  });

  test("get, has and the walks with a GameObject's type say so, rather than finding nothing (C4)", () {
    final level = world.add(Level());
    final child = level.add(GameObject(components: [Health()]));
    final health = child.get<Health>()!;
    Matcher pointsTo(String call) => throwsA(
      isA<ArgumentError>().having((e) => '$e', 'text', allOf(contains('Level is a GameObject'), contains(call))),
    );
    expect(() => child.getInParent<Level>(), pointsTo('ancestor<Level>()'));
    expect(() => health.getInParent<Level>(), pointsTo('ancestor<Level>()'));
    expect(() => level.getInChildren<Level>(), pointsTo('getInChildren<Level>()'));
    expect(() => level.get<Level>(), pointsTo('get<Level>()'));
    expect(() => level.getAll<Level>(), pointsTo('getAll<Level>()'));
    expect(() => level.has<Level>(), pointsTo('has<Level>()'));
    expect(() => level.getAllInChildren<Level>(), pointsTo('getAllInChildren<Level>()'));
    expect(level.getInChildren<Health>(), same(health), reason: 'a component type is found as before');
    expect(level.get<Object>(), isNull, reason: 'an interface objects and components share is fine');
  });

  test('getInParent and getInChildren skip objects turned off, unless includeInactive, as Unity (C5)', () {
    final root = world.add(GameObject(name: 'root'));
    final hidden = root.add(GameObject(name: 'hidden', components: [Health()]));
    final kid = hidden.add(GameObject(name: 'kid'));
    hidden.active = false;
    expect(root.getInChildren<Health>(), isNull);
    expect(root.getInChildren<Health>(includeInactive: true), same(hidden.get<Health>()));
    expect(kid.getInParent<Health>(), isNull, reason: 'its parent is off, so it is too');
    expect(kid.getInParent<Health>(includeInactive: true), same(hidden.get<Health>()));
    expect(hidden.getInChildren<Health>(), isNull, reason: 'the object itself is off');
    hidden.active = true;
    expect(root.getInChildren<Health>(), same(hidden.get<Health>()));
    expect(kid.getInParent<Health>(), same(hidden.get<Health>()));

    // Before the add, the flags given count.
    final top = GameObject(components: [Health()]);
    final below = GameObject()..active = false;
    top.add(below);
    final bottom = below.add(GameObject(components: [Health()]));
    expect(top.getInChildren<Health>(), same(top.get<Health>()));
    expect(bottom.getInParent<Health>(), same(top.get<Health>()), reason: 'the first on an object that is on');
    expect(bottom.getInParent<Health>(includeInactive: true), same(bottom.get<Health>()));
  });

  test('childCount, childAt and getAllInChildren, as Unity has (C11)', () {
    final root = world.add(
      GameObject(
        components: [Health()],
        children: [
          GameObject(name: 'a', components: [Health()]),
          GameObject(
            name: 'b',
            children: [
              GameObject(name: 'c', components: [Health()]),
            ],
          ),
        ],
      ),
    );
    expect(root.childCount, 2);
    expect(root.childAt(1).name, 'b');
    expect(() => root.childAt(2), throwsRangeError);
    expect(root.find('c')!.childCount, 0);
    expect(GameObject(children: [GameObject()]).childCount, 1, reason: 'before the add, the children given');
    final all = root.getAllInChildren<Health>();
    expect(all.map((h) => h.gameObject.name), [null, 'a', 'c'], reason: 'itself first, then depth first');
    root.find('b')!.active = false;
    expect(root.getAllInChildren<Health>(), hasLength(2));
    expect(root.getAllInChildren<Health>(includeInactive: true), hasLength(3));
    expect(root.getAllInChildren<Camera>(), isEmpty);
  });
}
