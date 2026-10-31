// A character walked as a game walks one, for a minute: Ring Ring Deals'
// player stopped for a frame about once a second (2026-10-30), and only
// the game found it. Its capsule, its fall, frames as a phone gives them,
// and the stick going round: not one frame may be lost.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

/// Walks the way [heading] says at [speed], with the game's gravity, and
/// counts the frames it went less than 40 % of the way.
class Walker extends Behaviour {
  Walker(this.speed);

  final double speed;
  double heading = 0;
  var _fall = 0.0;
  var frames = 0, stalls = 0, airborne = 0;
  var lowest = 0.0, highest = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    final controller = get<CharacterController>()!;
    final dt = time.delta;
    if (controller.isGrounded) _fall = -1;
    _fall -= 9.81 * dt;
    final way = Vec3(math.cos(heading), 0, math.sin(heading)) * speed;
    final before = position;
    controller.move(way * dt + Vec3(0, _fall * dt, 0));
    final moved = position - before;
    frames++;
    if (Vec3(moved.x, 0, moved.z).length < 0.4 * speed * dt) stalls++;
    if (!controller.isGrounded) airborne++;
    lowest = math.min(lowest, position.y);
    highest = math.max(highest, position.y);
  }
}

void main() {
  test('a minute walking round on open ground, frames as a phone gives them, loses none', () {
    final world = World();
    addTearDown(world.dispose);
    world.add(
      GameObject(
        name: 'floor',
        transform: Transform(position: const Vec3(0, -0.05, 0)),
        components: [BoxCollider(size: const Vec3(40, 0.1, 40))],
      ),
    );
    final walker = Walker(3.2);
    final you = world.add(
      GameObject(
        name: 'you',
        components: [
          CharacterController(height: 1.6, radius: 0.3, center: const Vec3(0, 0.8, 0), stepOffset: 0.1, mass: 0),
          walker,
        ],
      ),
    );
    // Settles on the floor first.
    for (var i = 0; i < 30; i++) {
      world.update(1 / 60);
    }
    walker
      ..frames = 0
      ..stalls = 0
      ..airborne = 0
      ..lowest = you.position.y
      ..highest = you.position.y;
    // Mostly 60 a second, some at 120, now and then a long one; the stick
    // turning a full circle every 6 s, and wobbling as a thumb does.
    final random = math.Random(7);
    var t = 0.0;
    while (t < 60) {
      final r = random.nextDouble();
      final dt = r < 0.8 ? 1 / 60 : (r < 0.97 ? 1 / 120 : 1 / 30);
      t += dt;
      walker.heading = t / 6 * 2 * math.pi + 0.15 * math.sin(t * 7);
      world.update(dt);
    }
    expect(walker.frames, greaterThan(3000));
    expect(walker.stalls, 0, reason: 'frames it stopped for, of ${walker.frames}');
    expect(walker.airborne, 0, reason: 'frames it was off the ground');
    expect(walker.highest - walker.lowest, lessThan(1e-3), reason: 'its feet stay at one height');
  });
}
