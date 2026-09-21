/// The behaviours of the example game. `fynkel codegen` (build_runner)
/// turns every `@FynkelBehaviour` class here into
/// `lib/generated/behaviours.g.dart` and the schemas under
/// `schemas/behaviours/`.
library;

import 'package:fynkel/fynkel.dart';

/// Rotates its object about [axis] at [speed] radians per second.
///
/// The `@Expose` hints here are the ADR-0018 set the editor's Inspector
/// reads (docs/codegen.md § Behaviour generator): a group, an order, a unit,
/// a widget kind, a visibility condition.
@FynkelBehaviour(id: 'game.Spinner', icon: 'rotate_right', category: 'Motion')
class Spinner extends Behaviour {
  /// Radians per second.
  @Expose(min: 0, step: 0.1, tooltip: 'Angular speed in radians per second', group: 'Motion', order: 1, unit: 'rad/s', showIf: 'spinning')
  double speed = 1.0;

  /// Unit axis of rotation (local space).
  @Expose(group: 'Motion', order: 2)
  Vec3 axis = Vec3.up;

  /// Spins only while set; a paused spinner keeps its angle.
  @Expose(label: 'Spinning', group: 'Motion', order: 0)
  bool spinning = true;

  /// The mesh this spinner shows off, if any (for the Inspector; unused).
  @Expose(kind: 'asset(mesh)', header: 'Presentation')
  AssetRef? showcase;

  /// Accumulated angle in radians (not authored).
  double angle = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (!spinning) return;
    angle += speed * context.dt;
    transform.rotation = Quat.axisAngle(axis.normalized, angle);
  }
}

/// A second type with nothing exposed, to show an empty field list and the
/// version knob.
@FynkelBehaviour(id: 'game.Blinker', version: 2)
class Blinker extends Behaviour {
  int ticks = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) => ticks++;
}
