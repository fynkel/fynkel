import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

void main() {
  late World w;
  setUp(() => w = World.create());
  tearDown(() => w.dispose());
  GameObject box() => w.edit(() => w.spawn(name: 'box')..set(const Collider()));
  PhysicsHit? hit() => w.raycast(const Vec3(-5, 0, 0), const Vec3(1, 0, 0));

  test('ray, shape and overlap queries do not publish staged edits', () {
    final go = box();
    w.frame(1 / 60);
    final distance = hit()!.distance;
    expect(
      () => w.edit(() {
        go.setLocalPosition(100, 0, 0);
        go.remove<Collider>();
        final pending = w.spawn()..set(const Collider());
        expect(hit()!.distance, distance);
        expect(w.raycast(const Vec3(-5, 0, 0), const Vec3(1, 0, 0), exclude: go), isNull);
        expect(w.raycast(const Vec3(-5, 0, 0), const Vec3(1, 0, 0), exclude: pending)!.entity, go.entity);
        expect(
          w
              .shapeCast(
                ColliderShape.sphere,
                const Vec3(0.2, 0.2, 0.2),
                const Vec3(-5, 0, 0),
                const Vec3(1, 0, 0),
              )!
              .entity,
          go.entity,
        );
        expect(w.overlap(ColliderShape.box, const Vec3(2, 2, 2), Vec3.zero).single.entity, go.entity);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    expect(go.localTransform.position, Vec3.zero);
    expect(hit()!.distance, distance);
    expect(w.objects.length, 1);
    w.edit(() {
      go.setLocalPosition(100, 0, 0);
      expect(hit()!.distance, distance);
    });
    w.frame(1 / 60);
    expect(hit()!.distance, greaterThan(100));
  });

  test('body and character state remain readable while effects are staged', () {
    final body = w.edit(
      () => w.spawn()
        ..set(const RigidBody(bodyType: BodyType.dynamic, gravityScale: 0))
        ..set(const Collider()),
    );
    final character = w.edit(
      () => w.spawn(transform: const Pose(position: Vec3(10, 0, 0)))..set(const CharacterController()),
    );
    w.frame(1 / 60);
    final velocity = character.characterState()!.velocity;
    expect(
      () => w.edit(() {
        body.applyImpulse(const Vec3(10, 0, 0));
        character.move(const Vec3(2, 0, 0));
        expect(body.bodyState()!.linearVelocity, Vec3.zero);
        expect(character.characterState()!.velocity, velocity);
        throw StateError('abort');
      }),
      throwsStateError,
    );
    w.frame(1 / 60);
    expect(body.bodyState()!.linearVelocity, Vec3.zero);
  });

  test('foreign query exclusions poison the edit even when caught', () {
    final go = box();
    w.frame(1 / 60);
    final other = World.create();
    try {
      final foreign = other.edit(() => other.spawn());
      expect(
        () => w.edit(() {
          go.name = 'discard';
          try {
            w.raycast(Vec3.zero, const Vec3(1, 0, 0), exclude: foreign);
          } catch (_) {}
        }),
        throwsStateError,
      );
      expect(go.name, 'box');
    } finally {
      other.dispose();
    }
  });
}
