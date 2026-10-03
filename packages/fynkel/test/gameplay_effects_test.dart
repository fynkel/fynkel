import 'package:fynkel/gameplay.dart';
import 'package:test/test.dart';

void main() {
  late World world;
  setUp(() => world = World.create()..throwOnDiagnostics = true);
  tearDown(() => world.dispose());

  GameObject body() => world.spawn(name: 'body')
    ..set(const RigidBody(bodyType: BodyType.dynamic, mass: 2, gravityScale: 0, linearDamping: 0))
    ..set(const Collider());

  test('pending body effects commit in order and execute once at the next tick', () {
    final go = world.edit(() {
      final go = body();
      go.setLinearVelocity(const Vec3(3, 0, 0));
      go.applyImpulse(const Vec3(4, 0, 0));
      expect(go.bodyState(), isNull);
      return go;
    });
    world.edit(() => go.name = 'renamed');
    world.frame(0);
    expect(go.bodyState(), isNull);
    world.frame(1 / 60);
    expect(go.bodyState()!.linearVelocity.x, closeTo(5, 1e-5));
    world.frame(1 / 60);
    expect(go.bodyState()!.linearVelocity.x, closeTo(5, 1e-5));
  });

  test('aborted effects and final component failure leave existing body unchanged', () {
    final go = world.edit(body);
    world.frame(1 / 60);
    for (final abort in [true, false]) {
      expect(
        () => world.edit(() {
          go.name = 'discard';
          go.applyImpulse(const Vec3(10, 0, 0));
          if (abort) throw StateError('abort');
          go.remove<RigidBody>();
        }),
        throwsStateError,
      );
      expect(go.name, 'body');
      world.frame(1 / 60);
      expect(go.bodyState()!.linearVelocity.x, 0);
    }
  });

  test('invalid effect errors stay sticky when caught', () {
    final go = world.edit(body);
    final operations = <void Function()>[
      () => go.applyForce(const Vec3(double.nan, 0, 0)),
      () => go.setKinematicTarget(Vec3.zero),
      () => go.move(Vec3.one),
      () => go.teleport(Vec3.zero, const Quat(0, 0, 0, 0)),
    ];
    for (final operation in operations) {
      expect(
        () => world.edit(() {
          go.name = 'discard';
          try {
            operation();
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(go.name, 'body');
    }
    world.frame(1 / 60);
    expect(go.bodyState()!.linearVelocity.x, 0);
  });

  test('effects target canonical spec placeholders and cancel with destruction', () {
    final spec = ObjectSpec(
      'body',
      components: [
        const RigidBody(bodyType: BodyType.dynamic),
        const Collider(),
      ],
    );
    final handle = spec.object;
    world.edit(() {
      world.build(spec);
      handle.applyImpulse(Vec3.one);
      handle.destroy();
    });
    world.frame(1 / 60);
    expect(handle.isAlive, isFalse);
    expect(world.find('body'), isNull);
  });

  for (final hz in [30, 60, 120, 240]) {
    test('committed impulse executes once across $hz Hz zero/multiple-tick frames', () {
      final go = world.edit(() {
        final go = body();
        go.applyImpulse(const Vec3(2, 0, 0));
        return go;
      });
      for (var frame = 0; frame < hz; frame++) {
        world.frame(1 / hz);
      }
      expect(go.bodyState()!.linearVelocity.x, closeTo(1, 1e-5));
      expect(go.transform.position.x, closeTo(1, 0.02));
    });
  }

  test('teleport and character movement preserve completed state until the tick', () {
    final go = world.edit(body);
    world.frame(1 / 60);
    world.edit(() => go.teleport(const Vec3(8, 0, 0)));
    expect(go.transform.position.x, 0);
    world.frame(1 / 60);
    expect(go.get<Pose>()!.position.x, closeTo(8, 1e-5));
    expect(go.transform.position.x, closeTo(8, 1e-5));
    final character = world.edit(() {
      final c = world.spawn()..set(const CharacterController());
      c.move(const Vec3(3, 0, 0));
      c.jump();
      return c;
    });
    world.frame(1 / 60);
    expect(character.characterState(), isNotNull);
    expect(character.transform.position.x, 0, reason: 'first tick places the capsule');
    world.frame(1 / 60);
    final moved = character.transform.position.x;
    expect(moved, greaterThan(0));
    world.frame(1 / 60);
    expect(character.transform.position.x, closeTo(moved, 1e-5), reason: 'move is consumed once');
  });
}
