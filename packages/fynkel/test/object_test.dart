// Everything is an Object (ADR-0036 §1): GameObjects, components (the
// transform and scripts) and assets share Unity's `UnityEngine.Object`
// base — a name, `isAlive`, `destroy()` — and `getComponent<T>` finds
// any component kind.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Health extends Behaviour {}

class Armor extends Behaviour {}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    return w;
  }

  test('GameObjects, transforms and scripts are FynkelObjects named after their object', () {
    final w = fresh();
    final hero = w.spawn(name: 'Hero');
    final health = hero.addBehaviour(Health());
    final objects = <FynkelObject>[hero, hero.transform, health];
    expect(objects.map((o) => o.name), ['Hero', 'Hero', 'Hero']);
    expect(objects.every((o) => o.isAlive), isTrue);
    expect(hero.transform, isA<Component>());
    expect(health, isA<Component>());
    expect(health.transform, same(hero.transform));
    expect(hero.transform.gameObject, same(hero));
  });

  test('getComponent finds the transform and scripts, as Unity does', () {
    final w = fresh();
    final hero = w.spawn(name: 'Hero');
    final health = hero.addBehaviour(Health());
    expect(hero.getComponent<Transform>(), same(hero.transform));
    expect(hero.getComponent<Health>(), same(health));
    expect(hero.getComponent<Armor>(), isNull);
    expect(health.getComponent<Health>(), same(health));
    // Unity's GetComponent<Component>() is the Transform, which comes first.
    expect(hero.getComponent<Component>(), same(hero.transform));
  });

  test('a destroyed object and its components are not alive', () {
    final w = fresh();
    final hero = w.spawn(name: 'Hero');
    final health = hero.addBehaviour(Health());
    final armor = hero.addBehaviour(Armor());
    w.frame(1 / 60);
    armor.destroy();
    w.frame(1 / 60);
    expect(armor.isAlive, isFalse);
    expect(health.isAlive, isTrue);
    hero.destroy();
    w.frame(1 / 60);
    expect(hero.isAlive, isFalse);
    expect(hero.transform.isAlive, isFalse);
    expect(health.isAlive, isFalse);
  });

  test('a transform cannot be destroyed', () {
    final w = fresh();
    final hero = w.spawn(name: 'Hero');
    expect(() => hero.transform.destroy(), throwsUnsupportedError);
  });

  test('a script not attached yet is not alive', () {
    expect(Health().isAlive, isFalse);
  });
}
