// Stage 5 step 4: a small, fast ball fired through thin walls that move,
// with and without continuous collision detection.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class RigidbodyWallsScene extends LabScene {
  @override
  String get title => 'Rigidbodies: a ball through walls';
  @override
  String get step => 'stage 5 step 4';
  @override
  String get about =>
      'A 0.2 m ball at 100 m/s goes 2 m each step: between two steps it can be on both sides of a 0.1 m wall and '
      'never touch it. These walls are kinematic rigidbodies, sliding up and down. With "continuous" on '
      '(collisionDetectionMode), physics sweeps the ball along its path and it stops at the first wall. (A fast '
      'body never passes a static collider: physics always sweeps those.)';

  final walls = <GameObject>[];
  late final GameObject ball;
  late final Rigidbody ballBody;
  var continuous = false;
  double? stoppedAt;
  var flying = false;

  static const _start = Vec3(-7, 1, 0);

  @override
  void build() {
    add(floorCollider());
    for (var i = 0; i < 3; i++) {
      walls.add(
        add(
          shape(
            'wall ${i + 1}',
            Mesh.cube(),
            const Color(0xFFD0D4DC),
            at: Vec3(-2.0 + i * 2.5, 1, 0),
            size: Vec3(0.1, 2, 2),
            components: [BoxCollider(), Rigidbody(isKinematic: true)],
          ),
        ),
      );
    }
    ballBody = Rigidbody(useGravity: false);
    ball = add(
      shape(
        'ball',
        Mesh.sphere(),
        const Color(0xFF3A8EE0),
        at: _start,
        size: Vec3(0.2, 0.2, 0.2),
        components: [SphereCollider(), ballBody],
      ),
    );
  }

  @override
  void onFixedUpdate() {
    super.onFixedUpdate();
    for (final (i, wall) in walls.indexed) {
      final y = 1 + 0.4 * math.sin(time.sinceStart * 1.5 + i);
      wall.get<Rigidbody>()!.movePosition(Vec3(-2.0 + i * 2.5, y, 0));
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final x = ball.position.x;
    if (flying && ballBody.linearVelocity.length < 1) {
      flying = false;
      stoppedAt = x;
    }
    if (x > 8) {
      flying = false;
      stoppedAt = null;
      ballBody.linearVelocity = Vec3.zero;
      ball.position = Vec3(8, 1, 0);
    }
    readout.value = [
      'collisionDetectionMode: ${ballBody.collisionDetectionMode.name}',
      if (flying)
        'flying'
      else if (stoppedAt case final at?)
        'it stopped at x ${at.toStringAsFixed(2)}, before ${_wallBefore(at)}'
      else if (x > 7)
        'it went through every wall',
    ].join('\n');
  }

  String _wallBefore(double x) {
    for (final wall in walls) {
      if (x < wall.position.x) return wall.name ?? 'a wall';
    }
    return 'nothing';
  }

  @override
  List<Control> get controls => [
    Button('fire', () {
      ball.position = _start;
      ballBody
        ..angularVelocity = Vec3.zero
        ..linearVelocity = Vec3(100, 0, 0);
      flying = true;
      stoppedAt = null;
      log.add('fired at 100 m/s, ${ballBody.collisionDetectionMode.name}');
    }),
    Toggle('continuous', () => continuous, (on) {
      continuous = on;
      ballBody.collisionDetectionMode = on ? CollisionDetectionMode.continuous : CollisionDetectionMode.discrete;
    }),
  ];
}
