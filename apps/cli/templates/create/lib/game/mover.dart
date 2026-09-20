import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

/// Walks its object with the `move` action and hops on `jump`.
///
/// Input reaches a behaviour through the project's actions
/// (`fynkel.project.json` → `inputActions`, docs/input.md), never through
/// keys: `move` is an `axis2d` bound to WASD, the left stick and the touch
/// overlay's stick; `jump` a `button` bound to Space, the south gamepad
/// button and the overlay's jump button. `Input.action2D` and
/// `Input.held` read the state as of this tick; `Input.pressed` is true
/// on exactly one tick per press, however many ticks a frame runs.
/// `test/game_test.dart` replays `test/walk.input.json` through this class.
@FynkelBehaviour(id: 'game.Mover')
class Mover extends Behaviour {
  @Expose(min: 0, step: 0.1, tooltip: 'Walking speed in metres per second')
  double speed = 2.0;

  @Expose(min: 0, step: 0.1, tooltip: 'How high a jump lifts the object, in metres')
  double jumpHeight = 1.0;

  /// Seconds a jump lasts, take-off to landing.
  static const jumpDuration = 0.5;

  /// Presses seen so far (not exposed: runtime state).
  int jumps = 0;

  /// Seconds into the current jump, or null on the ground.
  double? airTime;

  /// The ground height the object rests at (its authored y).
  double? _groundY;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    final p = transform.position;
    final ground = _groundY ??= p.y;

    // `move.y` is forward (W, stick up); forward is -z in this scene.
    final move = Input.action2D('move');
    var x = p.x + move.x * speed * context.dt;
    var z = p.z - move.y * speed * context.dt;

    if (airTime == null && Input.pressed('jump')) {
      jumps++;
      airTime = 0;
    }
    var y = ground;
    if (airTime case final t?) {
      final next = t + context.dt;
      if (next >= jumpDuration) {
        airTime = null;
      } else {
        airTime = next;
        y = ground + jumpHeight * math.sin(math.pi * next / jumpDuration);
      }
    }
    transform.position = Vec3(x, y, z);
  }
}
