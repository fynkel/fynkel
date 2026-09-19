import 'package:fynkel/fynkel.dart';

/// Rotates its object around [axis] at [speed] radians per second.
///
/// A behaviour is Dart logic attached to a GameObject (blueprint §3.2), the
/// counterpart of a Unity MonoBehaviour. `@Expose` fields are authored data:
/// the scene file sets them (`assets/scenes/main.scene.json` gives this cube
/// `speed: 1.5`), the Inspector shows them, and `fynkel codegen --project .`
/// regenerates `lib/generated/behaviours.g.dart` and
/// `schemas/behaviours/game.Spinner.schema.json` whenever they change.
/// Everything else on the class is plain runtime state.
@FynkelBehaviour(id: 'game.Spinner')
class Spinner extends Behaviour {
  @Expose(min: 0, step: 0.1, tooltip: 'Angular speed in radians per second')
  double speed = 1.0;

  @Expose()
  Vec3 axis = Vec3.up;

  /// Accumulated rotation in radians (not exposed: not authored).
  double angle = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    angle += speed * context.dt;
    transform.rotation = Quat.axisAngle(axis.normalized, angle);
  }
}
