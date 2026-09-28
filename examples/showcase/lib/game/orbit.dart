import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

/// Circles its object around [target] at [radius] and [height], looking at
/// the target — the showcase camera. Runs in `fixedUpdate` so a headless
/// run and the app agree on where the camera is at a given tick.
@FynkelBehaviour(id: 'game.Orbit')
class Orbit extends Behaviour {
  @Expose(tooltip: 'The point to circle and look at')
  Vec3 target = Vec3.zero;

  @Expose(min: 0.1, step: 0.5, tooltip: 'Distance from the target on the ground plane')
  double radius = 10.0;

  @Expose(step: 0.5, tooltip: 'Height above the target')
  double height = 4.0;

  @Expose(step: 0.05, tooltip: 'Angular speed in radians per second (negative reverses)')
  double speed = 0.1;

  @Expose(step: 0.1, tooltip: 'Starting angle in radians')
  double phase = 0.0;

  /// Radians per second the `move` action's x scrubs the orbit at
  /// (A/D, the left stick, the touch overlay's stick), on top of [speed].
  static const scrubSpeed = 0.6;

  /// The current angle around the target (runtime state).
  double angle = 0;

  @override
  void start() {
    angle = phase;
    _place();
  }

  @override
  void fixedUpdate(FixedUpdateContext context) {
    angle += (speed + Input.action2D('move').x * scrubSpeed) * context.dt;
    _place();
  }

  void _place() {
    final eye = target + Vec3(math.sin(angle) * radius, height, math.cos(angle) * radius);
    // A camera looks down its local -Z: yaw it toward the target, then
    // pitch it down onto the target.
    final pitch = math.atan2(height, radius);
    final rotation = Quat.axisAngle(Vec3.up, angle) * Quat.axisAngle(Vec3.right, -pitch);
    transform.value = Pose(position: eye, rotation: rotation, scale: transform.scale);
  }
}
