// CameraCollision: a rig's camera pulled in front of what stands between
// it and its target, as Cinemachine's Deoccluder (games 1 and 7 asked).

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

void main() {
  late World world;
  late GameObject player, wall;

  setUp(() {
    world = World();
    // A player at the origin, a capsule the camera looks out of.
    player = world.add(GameObject(name: 'player', components: [CapsuleCollider(radius: 0.4, height: 1.8)]));
    // A wall between the player and the camera: its near face at z 2.9.
    wall = world.add(
      GameObject(
        name: 'wall',
        transform: Transform(position: const Vec3(0, 0, 3)),
        components: [BoxCollider(size: const Vec3(4, 4, 0.2))],
      ),
    );
  });
  tearDown(() => world.dispose());

  void frames(int n, [double dt = 0.25]) {
    for (var i = 0; i < n; i++) {
      world.update(dt);
    }
  }

  GameObject orbitCamera(CameraCollision collision, {double distance = 6}) => world.add(
    GameObject(
      components: [
        Camera(),
        Orbit(player, distance: distance, yaw: 0, pitch: 0, controls: false),
        collision,
      ],
    ),
  );

  test('an orbit camera behind a wall comes in front of it, looking at the target still', () {
    final collision = CameraCollision(radius: 0.2);
    final camera = orbitCamera(collision);
    frames(1);
    // The sphere stops where it touches the wall's face: 2.9 - 0.2.
    expect(camera.position.z, closeTo(2.7, 1e-3));
    expect(camera.position.x.abs() + camera.position.y.abs(), lessThan(1e-6));
    expect(camera.forward.closeTo(Vec3.forward, 1e-6), isTrue);
    expect(collision.isPulledIn, isTrue);
  });

  test('clear of walls it stays where the rig put it, and goes back out once clear', () {
    final collision = CameraCollision(radius: 0.2, damping: 1);
    final camera = orbitCamera(collision);
    frames(1);
    expect(camera.position.z, closeTo(2.7, 1e-3));
    wall.position = const Vec3(50, 0, 3);
    // In at once, out over its damping: 99 % of the way back in 1 s.
    frames(4);
    expect(camera.position.z, closeTo(2.7 + 3.3 * 0.99, 1e-3));
    expect(collision.isPulledIn, isTrue);
    frames(40);
    expect(camera.position.z, closeTo(6, 1e-3));
    expect(collision.isPulledIn, isFalse);
  });

  test('it pulls in at once, however much damping it has', () {
    final collision = CameraCollision(radius: 0.2, damping: 1);
    wall.position = const Vec3(50, 0, 3);
    final camera = orbitCamera(collision);
    frames(1);
    expect(camera.position.z, closeTo(6, 1e-3));
    wall.position = const Vec3(0, 0, 3);
    frames(1);
    expect(camera.position.z, closeTo(2.7, 1e-3));
  });

  test('only the layers given; never the target, its children or a trigger', () {
    wall.layer = 3;
    final collision = CameraCollision(radius: 0.2, layers: LayerMask.except([3]));
    final camera = orbitCamera(collision);
    final shield = GameObject(
      transform: Transform(position: const Vec3(0, 0, 1.5)),
      components: [BoxCollider(size: const Vec3(2, 2, 0.2))],
    );
    player.add(shield);
    world.add(
      GameObject(
        transform: Transform(position: const Vec3(0, 0, 4.5)),
        components: [BoxCollider(size: const Vec3(4, 4, 0.2), isTrigger: true)],
      ),
    );
    frames(1);
    expect(camera.position.z, closeTo(6, 1e-3));
    expect(collision.isPulledIn, isFalse);
  });

  test('no nearer the target than minDistance', () {
    wall.position = const Vec3(0, 0, 0.6);
    final camera = orbitCamera(CameraCollision(radius: 0.2, minDistance: 1));
    frames(1);
    expect(camera.position.z, closeTo(1, 1e-3));
  });

  test('a Follow and LookAt camera keeps the LookAt point in sight', () {
    final camera = world.add(
      GameObject(
        components: [
          Camera(),
          Follow(player, offset: const Vec3(0, 3, 6)),
          LookAt(player, offset: const Vec3(0, 1, 0)),
          CameraCollision(radius: 0.2),
        ],
      ),
    );
    // A wall across the line from (0, 1, 0) to (0, 3, 6), at z 3.
    frames(1);
    final look = const Vec3(0, 1, 0);
    final along = (const Vec3(0, 3, 6) - look).normalized;
    final pulled = camera.position - look;
    expect(pulled.normalized.closeTo(along, 1e-4), isTrue, reason: '${camera.position}');
    // It touches the face at z 2.9 with its centre 0.2 short of it.
    expect(camera.position.z, closeTo(2.7, 1e-3));
    expect(camera.forward.closeTo(-along, 1e-4), isTrue);
  });

  test('a target given beats the rig\'s, and one that is gone leaves the camera alone', () {
    final other = world.add(GameObject(transform: Transform(position: const Vec3(0, 0, 5))));
    final collision = CameraCollision(radius: 0.2, target: other);
    final camera = orbitCamera(collision);
    frames(1);
    // From (0, 0, 5), the wall isn't between it and the camera at z 6.
    expect(camera.position.z, closeTo(6, 1e-3));
    other.destroy();
    collision.target = null;
    player.destroy();
    final errors = [];
    world.errors.listen(errors.add);
    frames(1);
    expect(errors, isEmpty);
    // The orbit holds still with its target gone, and so does this.
    expect(camera.position.z, closeTo(6, 1e-3));
  });
}
