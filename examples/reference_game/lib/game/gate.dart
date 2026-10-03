import 'package:fynkel/gameplay.dart';

/// A slab that slides up by [rise] over [seconds] once [open] is called
/// (by the [Switch]). It is static geometry — a `fynkel.Collider` without
/// a body — so moving its transform teleports the collider with it
/// (docs/physics.md): the player walks through once it has risen.
@FynkelBehaviour(id: 'game.Gate')
class Gate extends Behaviour {
  /// Metres to rise.
  @Expose(min: 0, step: 0.1)
  double rise = 2.6;

  /// Seconds the rise takes.
  @Expose(min: 0.01, step: 0.1)
  double seconds = 1.2;

  bool opening = false;
  bool get opened => _t >= seconds;
  double _t = 0;
  late double _restY;

  @override
  void start() {
    _restY = transform.position.y;
  }

  void open() => opening = true;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (!opening || opened) return;
    _t = (_t + context.dt).clamp(0, seconds);
    final p = transform.position;
    transform.position = Vec3(p.x, _restY + rise * (_t / seconds), p.z);
  }
}
