// Physics from Dart (M3-08, blueprint §9): RigidBody/Collider are native
// components on the M3-07a path, forces are body commands, contacts reach
// the object's behaviours as callbacks, and spatial queries go through one
// buffer. The simulation itself is native (Rapier); nothing is mirrored.

import 'dart:io';

import 'package:fynkel/fynkel.dart';
import 'package:fynkel/testing.dart';
import 'package:test/test.dart';

/// Counts its contacts and hops once when it first lands.
class Crate extends Behaviour {
  final List<String> log = [];
  int enters = 0, stays = 0, exits = 0;

  @override
  void onCollisionEnter(Contact contact) {
    enters++;
    final other = world.byEntity(contact.other(gameObject.entity));
    log.add(
      'enter ${other?.name ?? '?'} n=${contact.normalFor(gameObject.entity).y.toStringAsFixed(1)} '
      'j>0=${contact.impulse > 0}',
    );
    if (enters == 1) applyImpulse(const Vec3(0, 6, 0));
  }

  @override
  void onCollisionStay(Contact contact) => stays++;

  @override
  void onCollisionExit(Contact contact) {
    exits++;
    log.add('exit');
  }
}

class StaySensor extends Behaviour {
  int entered = 0, stays = 0;
  final Set<String?> others = {};
  @override
  void onTriggerEnter(Contact contact) => entered++;
  @override
  void onTriggerStay(Contact contact) {
    stays++;
    expect(contact.isTrigger, isTrue);
    others.add(world.byEntity(contact.other(gameObject.entity))?.name);
  }
}

/// Records the landings its object hears.
class Landing extends Behaviour {
  final List<String> log = [];
  @override
  void onCollisionEnter(Contact contact) {
    final me = gameObject.entity;
    log.add('${world.byEntity(contact.other(me))?.name} body=${world.byEntity(contact.bodyOf(me) ?? 0)?.name} '
        'n=${contact.normalFor(me).y.toStringAsFixed(1)}');
  }
}

class Sensor extends Behaviour {
  final List<String> log = [];
  @override
  void onTriggerEnter(Contact contact) => log.add('in ${world.byEntity(contact.other(gameObject.entity))?.name}');
  @override
  void onTriggerExit(Contact contact) => log.add('out ${world.byEntity(contact.other(gameObject.entity))?.name}');
}

void main() {
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-physics-cache-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });

  World fresh() {
    final w = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(w.dispose);
    return w;
  }

  /// A 20×1×20 slab whose top is y = 0.
  GameObject floor(World w) => w.spawn(name: 'floor')
    ..add(
      const Collider(
        shape: ColliderShape.box,
        size: Vec3(10, 0.5, 10),
        offset: Vec3(0, -0.5, 0),
        friction: 0.6,
        restitution: 0,
        isSensor: false,
        mask: 0xffffffff,
      ),
    );

  const unitBox = Collider(
    shape: ColliderShape.box,
    size: Vec3(0.5, 0.5, 0.5),
    offset: Vec3.zero,
    friction: 0.5,
    restitution: 0,
    isSensor: false,
    mask: 0xffffffff,
  );
  // f32-exact values so the read-back compares equal.
  const dynamicBody = RigidBody(
    bodyType: BodyType.dynamic,
    mass: 2,
    linearDamping: 0,
    angularDamping: 0.0625,
    gravityScale: 1,
    ccd: false,
  );

  test('a crate falls onto the floor: physics owns it, contacts reach its behaviour, it sleeps', () {
    final w = fresh();
    floor(w);
    final crate =
        w.spawn(
            transform: identityTransform.copyWith(position: const Vec3(0, 3, 0)),
            name: 'crate',
          )
          ..add(dynamicBody)
          ..add(unitBox);
    final b = crate.addBehaviour(Crate());
    expect(crate.get<RigidBody>(), dynamicBody, reason: 'read-your-writes before the spawn');
    w.frame(1 / 60); // the spawn; the components follow it into the next tick
    w.frame(1 / 60);
    expect(crate.get<RigidBody>(), dynamicBody);
    expect(crate.get<Collider>(), unitBox);
    expect(crate.components.map((c) => c.runtimeType), [Transform, Name, RigidBody, Collider]);
    var state = crate.bodyState()!;
    expect(state.linearVelocity.y, lessThan(0), reason: 'falling after one tick');
    expect(state.sleeping, isFalse);

    for (var i = 0; i < 300; i++) {
      w.frame(1 / 60);
    }
    expect(b.enters, 2, reason: 'lands, hops on the first contact, lands again: ${b.log}');
    expect(b.exits, 1);
    expect(b.stays, greaterThan(10));
    expect(b.log.first, 'enter floor n=-1.0 j>0=true');
    expect(crate.localTransform.position.y, closeTo(0.5, 0.05));
    state = crate.bodyState()!;
    expect(state.sleeping, isTrue, reason: '$state');
    expect(state.speed, lessThan(1e-3));

    // Physics owns the transform: a presentation write is rejected, a
    // simulation-lane write is a teleport.
    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    crate.setPresentationPosition(5, 5, 5);
    w.flushPresentation();
    expect(seen.map((d) => d.code), [DiagnosticCode.physicsOwned]);
    crate.setLocalPosition(2, 4, 0);
    w.frame(1 / 60);
    expect(crate.localTransform.position.x, closeTo(2, 1e-3));
    expect(crate.localTransform.position.y, lessThan(4), reason: 'falling again after the teleport');
  });

  test("a child's collider joins its parent's body; both objects hear the landing (ADR-0034)", () {
    final w = fresh();
    floor(w);
    final cart = w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(0, 3, 0)),
      name: 'cart',
    )..add(dynamicBody);
    final wheel = w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(0, -1, 0)),
      name: 'wheel',
      parent: cart,
    )..add(unitBox);
    final onCart = cart.addBehaviour(Landing());
    final onWheel = wheel.addBehaviour(Landing());
    for (var i = 0; i < 240; i++) {
      w.frame(1 / 60);
    }
    expect(cart.localTransform.position.y, closeTo(1.5, 0.05), reason: "the wheel's box holds the cart up");
    expect(onWheel.log, ['floor body=cart n=-1.0']);
    expect(onCart.log, ['floor body=cart n=-1.0'], reason: "the body's object hears its child's contact, once");
  });

  test('body commands: impulse, velocity, teleport; rejections name the reason', () {
    final w = fresh();
    floor(w);
    final crate =
        w.spawn(
            transform: identityTransform.copyWith(position: const Vec3(0, 0.5, 0)),
            name: 'crate',
          )
          ..add(dynamicBody)
          ..add(unitBox);
    final wall = w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(0, 0.5, 5)),
      name: 'wall',
    )..add(unitBox);
    w.frame(1 / 60); // spawns, components and bodies in one tick (M3-08b)
    crate.applyImpulse(const Vec3(4, 0, 0));
    w.frame(1 / 60);
    expect(crate.bodyState()!.linearVelocity.x, closeTo(1.9, 0.1), reason: 'j = m·Δv, minus friction over one tick');
    crate.setLinearVelocity(const Vec3(0, 0, -1));
    crate.setAngularVelocity(Vec3.zero); // the push also tipped it
    w.frame(1 / 60);
    var v = crate.bodyState()!.linearVelocity;
    expect(v.z, closeTo(-1, 0.15), reason: 'set, minus one tick of friction');
    expect(v.x.abs(), lessThan(0.3), reason: 'the push is gone (warm-started friction aside)');
    crate.teleport(const Vec3(-3, 0.5, 0));
    w.frame(1 / 60);
    expect(crate.localTransform.position.x, closeTo(-3, 1e-3));
    expect(wall.bodyState(), isNull, reason: 'a collider alone is static geometry, no body state');

    w.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    w.onDiagnostics = seen.addAll;
    wall.applyForce(Vec3.up); // no body
    crate.setKinematicTarget(Vec3.zero); // not kinematic
    crate.applyForce(const Vec3(double.nan, 0, 0)); // not finite
    w.frame(1 / 60);
    expect(seen.map((d) => d.code), [
      DiagnosticCode.missingComponent,
      DiagnosticCode.invalidPayload,
      DiagnosticCode.invalidPayload,
    ]);
    expect(seen.map((d) => d.component), [RigidBody.componentId, RigidBody.componentId, RigidBody.componentId]);
    expect(seen[1].message, contains('kinematic'));
  });

  test('a sensor reports triggers once per entry and exit', () {
    final w = fresh();
    final zone = w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(0, 2, 0)),
      name: 'zone',
    )..add(unitBox.copyWith(isSensor: true, size: const Vec3(1, 0.5, 1)));
    final s = zone.addBehaviour(Sensor());
    w.spawn(
        transform: identityTransform.copyWith(position: const Vec3(0, 5, 0)),
        name: 'ball',
      )
      ..add(dynamicBody)
      ..add(unitBox.copyWith(shape: ColliderShape.sphere, size: const Vec3(0.25, 0, 0)));
    for (var i = 0; i < 120; i++) {
      w.frame(1 / 60);
    }
    expect(s.log, ['in ball', 'out ball']);
  });

  test('a body resting in a sensor gets onTriggerStay every tick, as Unity (foundation 2.3)', () {
    final w = fresh();
    floor(w);
    final zone = w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(0, 1, 0)),
      name: 'zone',
    )..add(unitBox.copyWith(isSensor: true, size: const Vec3(2, 1, 2)));
    final s = zone.addBehaviour(StaySensor());
    w.frame(1 / 60); // the zone's script exists before anything enters
    w.spawn(
        transform: identityTransform.copyWith(position: const Vec3(0, 0.5, 0)),
        name: 'crate',
      )
      ..add(dynamicBody)
      ..add(unitBox);
    for (var i = 0; i < 30; i++) {
      w.frame(1 / 60);
    }
    expect(s.entered, 1);
    expect(s.stays, greaterThanOrEqualTo(28), reason: 'one a tick after the enter');
    expect(s.others, {'crate'});
  });

  test('objects made overlapping get their enter, on both sides (foundation 2.13)', () {
    final w = fresh();
    floor(w);
    final zone = w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(0, 1, 0)),
      name: 'zone',
    )..add(unitBox.copyWith(isSensor: true, size: const Vec3(2, 1, 2)));
    final crate =
        w.spawn(
            transform: identityTransform.copyWith(position: const Vec3(0, 0.5, 0)),
            name: 'crate',
          )
          ..add(dynamicBody)
          ..add(unitBox);
    final onZone = zone.addBehaviour(StaySensor());
    final onCrate = crate.addBehaviour(StaySensor());
    for (var i = 0; i < 5; i++) {
      w.frame(1 / 60);
    }
    expect(onZone.entered, 1, reason: 'the zone was made with the crate inside');
    expect(onCrate.entered, 1);
  });

  test('a disabled behaviour gets no contact callbacks (foundation 2.10, on purpose)', () {
    final w = fresh();
    floor(w);
    final crate =
        w.spawn(
            transform: identityTransform.copyWith(position: const Vec3(0, 2, 0)),
            name: 'crate',
          )
          ..add(dynamicBody)
          ..add(unitBox);
    final on = crate.addBehaviour(Landing());
    final off = crate.addBehaviour(Landing())..enabled = false;
    for (var i = 0; i < 90; i++) {
      w.frame(1 / 60);
    }
    expect(on.log, hasLength(1));
    expect(off.log, isEmpty, reason: 'Unity would call it; here disabled means off');
  });

  test('a clone is found by a query before the next tick, as Unity (foundation 2.7)', () {
    final w = fresh();
    final template = w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(0, 0.5, 0)),
      name: 'crate',
    )..add(unitBox);
    w.frame(1 / 60);
    w.frame(1 / 60);
    final copy = template.clone(name: 'copy');
    final hit = w.raycast(const Vec3(0, 5, 0), const Vec3(0, -1, 0), exclude: template);
    expect(hit?.entity, copy.entity, reason: "Instantiate's collider is in the physics scene at once");
  });

  test('queries hit triggers by default and skip a collider they start in, as Unity (foundation 2.8)', () {
    final w = fresh();
    final f = floor(w);
    final zone = w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(0, 2, 0)),
      name: 'zone',
    )..add(unitBox.copyWith(isSensor: true));
    w.spawn(
      transform: identityTransform.copyWith(position: const Vec3(5, 0.5, 0)),
      name: 'crate',
    ).add(unitBox);
    w.frame(1 / 60);
    w.frame(1 / 60);
    const down = Vec3(0, -1, 0);
    expect(w.raycast(const Vec3(0, 5, 0), down)?.entity, zone.entity, reason: "Unity's queriesHitTriggers");
    expect(w.raycast(const Vec3(0, 5, 0), down, hitSensors: false)?.entity, f.entity);
    expect(
      w.overlap(ColliderShape.sphere, const Vec3(0.1, 0, 0), const Vec3(0, 2, 0)).map((h) => h.entity),
      [zone.entity],
    );
    // From inside the crate: the crate is skipped, the floor below is hit.
    final inside = w.raycast(const Vec3(5, 0.5, 0), down);
    expect(inside?.entity, f.entity, reason: 'a ray does not detect a collider it starts in');
    expect(inside!.distance, closeTo(0.5, 1e-3));
    expect(w.raycastAll(const Vec3(5, 0.5, 0), down).map((h) => h.entity), [f.entity]);
    expect(
      w.shapeCast(ColliderShape.sphere, const Vec3(0.1, 0, 0), const Vec3(5, 0.5, 0), const Vec3(1, 0, 0)),
      isNull,
      reason: 'nor does a cast it starts in',
    );
  });

  test('raycast, shape cast and overlap read the last step', () {
    final w = fresh();
    final f = floor(w);
    final crate =
        w.spawn(
            transform: identityTransform.copyWith(position: const Vec3(0, 0.5, 0)),
            name: 'crate',
          )
          ..add(dynamicBody)
          ..add(unitBox);
    w.frame(1 / 60);
    w.frame(1 / 60);
    final hit = w.raycast(const Vec3(0, 5, 0), const Vec3(0, -1, 0));
    expect(hit, isNotNull);
    expect(hit!.entity, crate.entity);
    expect(hit.point.y, closeTo(1, 0.05));
    expect(hit.normal.y, closeTo(1, 1e-3));
    expect(hit.distance, closeTo(4, 0.05));
    final all = w.raycastAll(const Vec3(0, 5, 0), const Vec3(0, -1, 0));
    expect(all.map((h) => h.entity), [crate.entity, f.entity]);
    expect(w.raycast(const Vec3(0, 5, 0), const Vec3(0, -1, 0), exclude: crate)!.entity, f.entity);
    expect(w.raycast(const Vec3(0, 5, 0), const Vec3(0, -1, 0), maxDistance: 1), isNull);
    expect(w.raycast(const Vec3(0, 5, 0), const Vec3(0, -1, 0), mask: 2), isNull, reason: 'nothing on layer 1');

    final sweep = w.shapeCast(
      ColliderShape.sphere,
      const Vec3(0.25, 0, 0),
      const Vec3(3, 0.5, 0),
      const Vec3(-1, 0, 0),
    );
    expect(sweep, isNotNull);
    expect(sweep!.entity, crate.entity);
    expect(sweep.distance, closeTo(3 - 0.5 - 0.25, 0.05));

    final around = w.overlap(ColliderShape.box, const Vec3(2, 2, 2), Vec3.zero);
    expect(around.map((h) => h.entity).toSet(), {crate.entity, f.entity});
    expect(w.overlap(ColliderShape.sphere, const Vec3(0.1, 0, 0), const Vec3(0, 10, 0)), isEmpty);

    // Several queries in one call, answered in order with their ids.
    final hits = w.physicsQuery([
      for (final (id, x) in [(7, 0.0), (9, 50.0)])
        PhysicsQueryRecord(
          id: id,
          kind: PhysicsQueryKind.raycast,
          shape: ColliderShape.box,
          size: Vec3.zero,
          origin: Vec3(x, 5, 0),
          rotation: Quat.identity,
          direction: const Vec3(0, -1, 0),
          maxDistance: 100,
          mask: 0xffffffff,
          maxHits: 4,
          hitSensors: false,
        ),
    ]);
    expect(hits.map((h) => (h.query, h.entity)), [(7, crate.entity), (7, f.entity)]);
  });

  test('the fixture project brings gravity, named layers and its own crate', () {
    final w = fresh();
    w.onLoadDiagnostics = (_) {};
    final report = w.loadProject(fixture, cacheDir: cache);
    expect(report.physics.gravity.y, closeTo(-9.81, 1e-5));
    expect(w.layersAndTags.layers, ['default', 'props']);
    expect(report.layersAndTags.layers, ['default', 'props']);
    expect(report.layersAndTags.tags, isEmpty);
    expect(w.layerMask(['props']), 2);
    expect(w.layerMask(['default', 'props']), 3);
    expect(() => w.layerMask(['ghosts']), throwsArgumentError);
    final crate = w.attach(w.allEntities().firstWhere((e) => w.nameOfEntity(e) == 'Crate B'));
    for (var i = 0; i < 240; i++) {
      w.frame(1 / 60);
    }
    expect(crate.localTransform.position.y, closeTo(0.5, 0.05));
    expect(crate.get<Layer>()!.layer, 1);
    expect(w.raycast(const Vec3(-2, 5, 0), const Vec3(0, -1, 0), mask: w.layerMask(['props']))!.entity, crate.entity);
    expect(
      w.raycast(const Vec3(-2, 5, 0), const Vec3(0, -1, 0), mask: w.layerMask(['default']))!.entity,
      isNot(crate.entity),
    );
  });

  test('a body added and pushed on an object spawned in the same tick moves', () {
    final w = fresh();
    final go = w.spawn(name: 'early')
      ..add(dynamicBody.copyWith(gravityScale: 0))
      ..add(unitBox.copyWith(shape: ColliderShape.sphere))
      ..setLinearVelocity(const Vec3(0, 0, 6));
    expect(go.isPending, isTrue);
    w.frame(1 / 60); // creates the body and applies the velocity in that tick
    expect(go.isPending, isFalse);
    expect(go.bodyState()!.linearVelocity.z, closeTo(6, 1e-4));
    w.frame(1 / 60);
    expect(go.localTransform.position.z, closeTo(2 * 6 / 60, 1e-3));
    go.destroy();
    w.frame(1 / 60);
    expect(() => go.applyForce(Vec3.up), throwsStateError, reason: 'destroyed');
  });
}
