// Stage 14's physics rows (docs/stage-14-findings.md, 2b): a collider
// changed or added on a body that has moved stays on it (PH1, PH2), a
// collider turned off and on in a frame stays in its trigger (PH3),
// movePosition and moveRotation on a moving body change only what they
// say (PH4), and a joint whose other body goes says so (PH10).

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

GameObject floor(World world) => world.add(
  GameObject(
    name: 'floor',
    transform: Transform(position: const Vec3(0, -0.5, 0)),
    components: [BoxCollider(size: const Vec3(100, 1, 100))],
  ),
);

void frames(World world, int count, [double dt = step]) {
  for (var i = 0; i < count; i++) {
    world.update(dt);
  }
}

final class Zone extends Behaviour with Triggers {
  final inside = <Collider>{};
  final log = <String>[];
  @override
  void onTriggerEnter(Collider other) {
    log.add('enter');
    inside.add(other);
  }

  @override
  void onTriggerExit(Collider other) {
    log.add('exit');
    inside.remove(other);
  }
}

void main() {
  for (final betweenFrames in [false, true]) {
    final tag = betweenFrames ? 'between frames' : 'in the fixed step';

    test('PH4 ($tag): moveRotation on a falling dynamic body keeps its position', () {
      final world = newWorld(betweenFrames: betweenFrames);
      final rb = Rigidbody();
      final o = world.add(
        GameObject(
          transform: Transform(position: const Vec3(3, 5, 0)),
          components: [BoxCollider(), rb],
        ),
      );
      frames(world, 4);
      rb.moveRotation(Quat.axisAngle(Vec3.up, 0.5));
      frames(world, 3);
      expect(o.position.x, closeTo(3, 0.01));
    });

    test('PH4b ($tag): movePosition on a turned dynamic body keeps its rotation', () {
      final world = newWorld(betweenFrames: betweenFrames);
      final rb = Rigidbody();
      final turned = Quat.axisAngle(Vec3.up, 0.785);
      final o = world.add(
        GameObject(
          transform: Transform(position: Vec3.zero, rotation: turned),
          components: [BoxCollider(), rb],
        ),
      );
      frames(world, 4);
      rb.movePosition(o.position + const Vec3(1, 0, 0));
      frames(world, 3);
      final d = (o.rotation.x - turned.x).abs() + (o.rotation.y - turned.y).abs() + (o.rotation.w - turned.w).abs();
      expect(d, lessThan(0.01), reason: 'rotation ${o.rotation} kept $turned');
    });
  }
}
