// Callbacks and the layer matrix through the package (docs/stage-5.md
// §3.4, §3.5, step 5): Unity's trigger and collision table, row by row,
// the exits §3.4 lists, what a Collision says, errors to world.errors, and
// layers and pairs that pass through each other.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

const step = 1 / 50;

World newWorld({bool betweenFrames = true}) {
  final world = World();
  addTearDown(world.dispose);
  world.physics.stepBetweenFrames = betweenFrames;
  return world;
}

void frames(World world, int count) {
  for (var i = 0; i < count; i++) {
    world.update(step);
  }
}

/// Writes down every callback it gets, as 'triggerEnter other' and the
/// like, into [log].
final class Log extends Behaviour with Triggers, Collisions {
  Log(this.log, [this.prefix = '']);

  final List<String> log;
  final String prefix;
  Collision? last;
  final collisions = <(Vec3, Vec3, int, Rigidbody?)>[];

  // A destroyed object can't be read: its collider's exit still comes.
  void _write(String what, Collider other) {
    final o = other.gameObject;
    log.add('$prefix$what ${o.isDestroyed ? 'a destroyed object' : o.name}');
  }

  @override
  void onTriggerEnter(Collider other) => _write('triggerEnter', other);
  @override
  void onTriggerStay(Collider other) => _write('triggerStay', other);
  @override
  void onTriggerExit(Collider other) => _write('triggerExit', other);
  @override
  void onCollisionEnter(Collision collision) {
    _write('collisionEnter', collision.collider);
    last = collision;
    collisions.add((collision.relativeVelocity, collision.impulse, collision.contactCount, collision.rigidbody));
  }

  @override
  void onCollisionStay(Collision collision) => _write('collisionStay', collision.collider);
  @override
  void onCollisionExit(Collision collision) {
    _write('collisionExit', collision.collider);
    expect(collision.contactCount, 0);
  }
}

/// An object that listens itself, as `extends GameObject with Triggers`.
final class Gate extends GameObject with Triggers {
  Gate({super.name, super.transform, super.components});

  final entered = <String>[];

  @override
  void onTriggerEnter(Collider other) => entered.add(other.gameObject.name ?? '');
}

enum Body { none, dynamic, kinematic }

GameObject thing(World world, String name, Vec3 at, {Body body = Body.none, bool trigger = false, Log? log}) =>
    world.add(
      GameObject(
        name: name,
        transform: Transform(position: at),
        components: [
          BoxCollider(isTrigger: trigger),
          if (body != Body.none) Rigidbody(useGravity: false, isKinematic: body == Body.kinematic),
          ?log,
        ],
      ),
    );

/// A floor 100 × 100 whose top is at y 0.
GameObject floor(World world, {List<Component> components = const [], int layer = 0}) => world.add(
  GameObject(
    name: 'floor',
    layer: layer,
    transform: Transform(position: const Vec3(0, -0.5, 0)),
    components: [
      BoxCollider(size: const Vec3(100, 1, 100)),
      ...components,
    ],
  ),
);

GameObject ball(World world, Vec3 at, {List<Component> components = const [], int layer = 0}) => world.add(
  GameObject(
    name: 'ball',
    layer: layer,
    transform: Transform(position: at),
    components: [SphereCollider(), Rigidbody(), ...components],
  ),
);

void main() {
  for (final betweenFrames in [false, true]) {
    group(betweenFrames ? 'between frames:' : 'in the fixed step:', () {
      // Unity's table: a trigger message needs a trigger and a rigidbody
      // (kinematic will do) on either side; a collision message no
      // trigger, and a dynamic rigidbody on either side.
      for (final a in Body.values) {
        for (final b in Body.values) {
          if (b.index < a.index) continue;
          for (final (aTrigger, bTrigger) in [(false, false), (true, false), (false, true), (true, true)]) {
            final anyTrigger = aTrigger || bTrigger;
            final expected = anyTrigger
                ? (a != Body.none || b != Body.none ? 'trigger' : null)
                : (a == Body.dynamic || b == Body.dynamic ? 'collision' : null);
            final what = '${a.name}${aTrigger ? ' trigger' : ''} and ${b.name}${bTrigger ? ' trigger' : ''}';
            test('$what: ${expected ?? 'nothing'}', () {
              final world = newWorld(betweenFrames: betweenFrames);
              final log = <String>[];
              thing(world, 'a', Vec3.zero, body: a, trigger: aTrigger, log: Log(log, 'a '));
              thing(world, 'b', const Vec3(0.5, 0, 0), body: b, trigger: bTrigger, log: Log(log, 'b '));
              frames(world, 3);
              if (expected == null) {
                expect(log, isEmpty);
              } else {
                expect(log.where((l) => l.contains('Enter')), ['a ${expected}Enter b', 'b ${expected}Enter a']);
                expect(log.where((l) => l.contains('Stay')), isNotEmpty);
              }
            });
          }
        }
      }

      test('a ball through a trigger: enter, a stay a step, exit', () {
        final world = newWorld(betweenFrames: betweenFrames);
        final gate = world.add(
          Gate(
            name: 'gate',
            transform: Transform(position: const Vec3(0, -2, 0)),
            components: [BoxCollider(isTrigger: true)],
          ),
        );
        final log = <String>[];
        ball(world, const Vec3(0, 0, 0), components: [Log(log)]);
        frames(world, 60);
        expect(gate.entered, ['ball']);
        expect(log.first, 'triggerEnter gate');
        expect(log.last, 'triggerExit gate');
        expect(log.where((l) => l == 'triggerEnter gate'), hasLength(1));
        expect(log.where((l) => l == 'triggerExit gate'), hasLength(1));
        expect(log.where((l) => l == 'triggerStay gate').length, greaterThan(1));
      });

      test('a ball on the floor: what each side is told', () {
        final world = newWorld(betweenFrames: betweenFrames);
        final floorLog = Log([]);
        final ballLog = Log([]);
        floor(world, components: [floorLog]);
        final dropped = ball(world, const Vec3(0, 1.5, 0), components: [ballLog]);
        frames(world, 40);
        expect(ballLog.log.first, 'collisionEnter floor');
        expect(floorLog.log.first, 'collisionEnter ball');
        expect(ballLog.log.where((l) => l.contains('Stay')), isNotEmpty);
        // The ball sees the floor come up at it, and is pushed up.
        final (ballSpeed, ballPush, ballPoints, ballOther) = ballLog.collisions.single;
        expect(ballSpeed.y, greaterThan(3));
        expect(ballPush.y, greaterThan(0.5));
        expect(ballPoints, greaterThan(0));
        expect(ballOther, isNull);
        // The floor sees the opposite, from the ball's rigidbody.
        final (floorSpeed, floorPush, _, floorOther) = floorLog.collisions.single;
        expect(floorSpeed.y, closeTo(-ballSpeed.y, 1e-4));
        expect(floorPush.y, closeTo(-ballPush.y, 1e-4));
        expect(floorOther, same(dropped.get<Rigidbody>()));
      });

      test('a collision\'s points, from its side', () {
        final world = newWorld(betweenFrames: betweenFrames);
        final points = <ContactPoint>[];
        final ballLog = _Points(points);
        floor(world);
        ball(world, const Vec3(0, 0.45, 0), components: [ballLog]);
        frames(world, 3);
        expect(points, isNotEmpty);
        final p = points.first;
        expect(p.normal.y, closeTo(1, 1e-3));
        expect(p.point.y, closeTo(0, 0.1));
        expect(p.separation, lessThan(0.1));
        expect(p.thisCollider.gameObject.name, 'ball');
        expect(p.otherCollider.gameObject.name, 'floor');
      });

      test('a rigidbody\'s object hears its children\'s colliders', () {
        final world = newWorld(betweenFrames: betweenFrames);
        floor(world);
        final log = <String>[];
        world.add(
          GameObject(
            name: 'cart',
            transform: Transform(position: const Vec3(0, 1, 0)),
            components: [Rigidbody(), Log(log)],
            children: [
              GameObject(
                name: 'wheel',
                transform: Transform(position: const Vec3(0, -0.5, 0)),
                components: [SphereCollider(radius: 0.25)],
              ),
            ],
          ),
        );
        frames(world, 40);
        expect(log.first, 'collisionEnter floor');
      });

      test('a listener put on later hears from then on', () {
        final world = newWorld(betweenFrames: betweenFrames);
        floor(world);
        final dropped = ball(world, const Vec3(0, 3, 0));
        frames(world, 2);
        final log = <String>[];
        dropped.addComponent(Log(log));
        frames(world, 60);
        expect(log.first, 'collisionEnter floor');
      });

      test('a disabled behaviour still hears, as in Unity; a removed one doesn\'t', () {
        final world = newWorld(betweenFrames: betweenFrames);
        floor(world);
        final heard = <String>[];
        final removed = <String>[];
        final disabled = Log(heard)..enabled = false;
        final gone = Log(removed);
        final dropped = ball(world, const Vec3(0, 1, 0), components: [disabled, gone]);
        frames(world, 1);
        dropped.removeComponent(gone);
        frames(world, 40);
        expect(heard, contains('collisionEnter floor'));
        expect(removed, isEmpty);
      });

      test('an error in a callback goes to world.errors, and the rest are still called', () async {
        final world = newWorld(betweenFrames: betweenFrames);
        final errors = <ObjectError>[];
        final listening = world.errors.listen(errors.add);
        addTearDown(listening.cancel);
        final log = <String>[];
        final dropped = ball(world, const Vec3(0, 0.45, 0), components: [_Throws(), Log(log)]);
        floor(world);
        frames(world, 4);
        await Future<void>.delayed(Duration.zero);
        expect(errors, isNotEmpty);
        expect(errors.first.gameObject, same(dropped));
        expect(errors.first.error, isA<StateError>());
        expect(log.first, 'collisionEnter floor');
      });

      group('the other side hears an exit when', () {
        void exits(String what, void Function(GameObject mover, GameObject zone) leave, {bool trigger = false}) {
          test(what, () {
            final world = newWorld(betweenFrames: betweenFrames);
            final log = <String>[];
            final zone = thing(world, 'zone', Vec3.zero, trigger: trigger, log: Log(log));
            final mover = thing(world, 'mover', const Vec3(0.5, 0, 0), body: Body.kinematic, trigger: !trigger);
            frames(world, 3);
            expect(log, contains('triggerEnter mover'));
            log.clear();
            leave(mover, zone);
            frames(world, 3);
            expect(log.where((l) => !l.contains('Stay')), [
              if (what == 'it is destroyed') 'triggerExit a destroyed object' else 'triggerExit mover',
            ]);
            expect(zone.isDestroyed, isFalse);
          });
        }

        exits('it moves away', (o, _) => o.position = const Vec3(5, 0, 0));
        exits('its collider is disabled', (o, _) => o.get<BoxCollider>()!.enabled = false);
        exits('its collider is removed', (o, _) => o.removeComponent(o.get<BoxCollider>()!));
        exits('it is destroyed', (o, _) => o.destroy());
        exits('it is hidden', (o, _) => o.active = false);
        exits('its rigidbody is removed', (o, _) => o.removeComponent(o.get<Rigidbody>()!));
        exits('it goes to a layer that doesn\'t meet', (o, _) {
          o.physics.ignoreLayerCollision(0, 7);
          o.layer = 7;
        });
        exits('the zone stops being a trigger', (_, zone) => zone.get<BoxCollider>()!.isTrigger = false, trigger: true);
      });

      // Enters come before exits.
      test('switching to solid starts a collision and ends the trigger', () {
        final world = newWorld(betweenFrames: betweenFrames);
        final log = <String>[];
        final a = thing(world, 'a', Vec3.zero, trigger: true, log: Log(log));
        thing(world, 'b', const Vec3(0.5, 0, 0), body: Body.dynamic);
        frames(world, 3);
        a.get<BoxCollider>()!.isTrigger = false;
        log.clear();
        frames(world, 3);
        expect(log.where((l) => !l.contains('Stay')), ['collisionEnter b', 'triggerExit b']);
      });

      test('layers that don\'t meet pass through, and meet again', () {
        final world = newWorld(betweenFrames: betweenFrames);
        final physics = world.physics;
        expect(physics.getIgnoreLayerCollision(8, 9), isFalse);
        physics.ignoreLayerCollision(9, 8);
        expect(physics.getIgnoreLayerCollision(8, 9), isTrue);
        floor(world, layer: 9);
        final dropped = ball(world, const Vec3(0, 1, 0), layer: 8);
        frames(world, 30);
        expect(dropped.position.y, lessThan(0));
        physics.ignoreLayerCollision(8, 9, false);
        expect(physics.getIgnoreLayerCollision(8, 9), isFalse);
        expect(() => physics.ignoreLayerCollision(32, 0), throwsRangeError);
        expect(() => physics.getIgnoreLayerCollision(0, -1), throwsRangeError);
      });

      test('a pair that ignores each other passes through, until one leaves the world', () {
        final world = newWorld(betweenFrames: betweenFrames);
        final physics = world.physics;
        final ground = floor(world);
        final dropped = ball(world, const Vec3(0, 1, 0));
        final (floorCollider, ballCollider) = (ground.get<BoxCollider>()!, dropped.get<SphereCollider>()!);
        physics.ignoreCollision(ballCollider, floorCollider);
        expect(physics.getIgnoreCollision(floorCollider, ballCollider), isTrue);
        frames(world, 30);
        expect(dropped.position.y, lessThan(0));
        expect(() => physics.ignoreCollision(ballCollider, ballCollider), throwsArgumentError);
        ballCollider.enabled = false;
        expect(() => physics.getIgnoreCollision(ballCollider, floorCollider), throwsStateError);
        ballCollider.enabled = true;
        frames(world, 2);
        expect(physics.getIgnoreCollision(ballCollider, floorCollider), isFalse);
      });
    });
  }
}

final class _Points extends Behaviour with Collisions {
  _Points(this.points);

  final List<ContactPoint> points;

  @override
  void onCollisionEnter(Collision collision) => points.addAll(collision.contacts);
}

final class _Throws extends Behaviour with Collisions {
  @override
  void onCollisionEnter(Collision collision) => throw StateError('a bug in a callback');
}
