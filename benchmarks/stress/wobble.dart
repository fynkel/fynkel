import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

/// Bobs its object up and down and spins it, writing the transform every
/// tick — the stress scene's "ticking Dart behaviour" (blueprint §15.3):
/// 300 of these next to 5 000 native movers is the item 10 workload. It
/// does on the Dart side what `fynkel.Oscillate` + `fynkel.Rotator` do
/// natively, so the two costs compare directly in the HUD's `systems` line
/// (`commands` is these 300 writes) against `rotator`/`oscillate`.
@FynkelBehaviour(id: 'game.Wobble')
class Wobble extends Behaviour {
  @Expose(min: 0, step: 0.05, tooltip: 'Bob height in metres')
  double amplitude = 0.3;

  @Expose(min: 0, step: 0.1, tooltip: 'Bobs per second')
  double frequency = 0.5;

  @Expose(step: 0.1, tooltip: 'Yaw in radians per second')
  double spin = 1.0;

  @Expose(step: 0.1, tooltip: 'Starting angle of the bob in radians')
  double phase = 0.0;

  late double _restY;
  double _t = 0;

  @override
  void start() {
    _restY = transform.position.y;
  }

  @override
  void fixedUpdate(FixedUpdateContext context) {
    _t += context.dt;
    final p = transform.position;
    final y = _restY + amplitude * math.sin(2 * math.pi * frequency * _t + phase);
    final rotation = Quat.axisAngle(Vec3.up, spin * context.dt) * transform.rotation;
    transform.value = Transform(position: Vec3(p.x, y, p.z), rotation: rotation, scale: transform.scale);
  }
}
