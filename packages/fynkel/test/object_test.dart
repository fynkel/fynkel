// Everything is an Object (ADR-0036 §1): GameObjects, components (the
// transform and scripts) and assets share Unity's `UnityEngine.Object`
// base — a name, `isAlive`, `destroy()` — and `getComponent<T>` finds
// any component kind.

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Health extends Behaviour {}

class Armor extends Behaviour {}

class Hunter extends Behaviour {
  GameObject? prey;
  Health? health;
  GameObject? home;
}

/// What the generator emits for [Hunter].
final hunterType = BehaviourType(
  id: 'test.Hunter',
  create: Hunter.new,
  fields: [
    BehaviourField(
      name: 'prey',
      type: 'gameObject',
      get: (b) => referenceJson((b as Hunter).prey),
      set: (b, v) => (b as Hunter).prey = v as GameObject?,
      target: (b) => (b as Hunter).prey,
    ),
    BehaviourField(
      name: 'health',
      type: 'behaviour',
      get: (b) => referenceJson((b as Hunter).health),
      set: (b, v) => (b as Hunter).health = v as Health?,
      target: (b) => (b as Hunter).health,
    ),
    BehaviourField(
      name: 'home',
      type: 'gameObject',
      get: (b) => referenceJson((b as Hunter).home),
      set: (b, v) => (b as Hunter).home = v as GameObject?,
      target: (b) => (b as Hunter).home,
    ),
  ],
);

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

  test('a script field reads null once its object or script is destroyed, as Unity\'s == null (ADR-0036 §5)', () {
    final w = fresh();
    w.registry.register(hunterType);
    final prey = w.spawn(name: 'Prey');
    final health = w.spawn(name: 'Medic').addBehaviour(Health());
    final home = w.spawn(name: 'Home');
    // One built from the registry, one the game made itself.
    final a = w.spawn(name: 'A').addBehaviourFromData('test.Hunter') as Hunter;
    final b = w.spawn(name: 'B').addBehaviour(Hunter());
    for (final h in [a, b]) {
      h
        ..prey = prey
        ..health = health
        ..home = home;
    }
    w.frame(1 / 60);

    prey.destroy();
    health.destroy();
    w.frame(1 / 60);
    for (final h in [a, b]) {
      expect(h.prey, isNull);
      expect(h.health, isNull);
      expect(h.home, same(home), reason: 'a live reference is kept');
    }
  });

  test('a built-in component is a NativeComponent: a live handle on its object (ADR-0036 §2)', () {
    final w = fresh();
    final hero = w.spawn(name: 'Hero')..add(const PointLight(intensity: 2));
    w.frame(1 / 60);
    final light = hero.component<PointLight>()!;
    expect(light, isA<Component>());
    expect(hero.component<PointLight>(), same(light), reason: 'one handle per component');
    expect(hero.getComponent<NativeComponent<PointLight>>(), same(light));
    expect(hero.getComponent<NativeComponent<SpotLight>>(), isNull);
    expect(hero.component<SpotLight>(), isNull);
    expect((light.name, light.gameObject, light.transform), ('Hero', hero, hero.transform));
    expect(light.value.intensity, 2);

    light.value = light.value.copyWith(intensity: 5);
    expect(hero.get<PointLight>()!.intensity, 5, reason: 'read-your-writes, as get/set');
    w.frame(1 / 60);
    expect(light.value.intensity, 5);

    light.destroy();
    expect(hero.has<PointLight>(), isFalse);
    expect(light.isAlive, isFalse);
    expect(() => light.value, throwsStateError);
    expect(() => hero.component<Pose>(), throwsArgumentError, reason: 'the transform is hero.transform');
  });
}
