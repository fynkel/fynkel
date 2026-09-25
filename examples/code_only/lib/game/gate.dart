import 'package:fynkel/fynkel.dart';

/// A slab that slides up by [rise] over [seconds] once [open] is called
/// (by the `Switch`). It is static geometry — a collider without a body —
/// so moving its transform moves the collider: the player walks through
/// once it has risen. The rise is a tween (`transform.moveTo`).
@FynkelBehaviour(id: 'game.Gate')
class Gate extends Behaviour {
  /// Metres to rise.
  @Expose(min: 0, step: 0.1)
  double rise = 2.6;

  /// Seconds the rise takes.
  @Expose(min: 0.01, step: 0.1)
  double seconds = 1.2;

  Coroutine? _rising;

  /// [open] was called.
  bool get opening => _rising != null;

  /// The rise has finished.
  bool get opened => _rising != null && !_rising!.isRunning;

  void open() {
    if (_rising != null) return;
    final p = transform.position;
    _rising = transform.moveTo(Vec3(p.x, p.y + rise, p.z), seconds, ease: Ease.inOutSine);
  }
}
