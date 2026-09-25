/// The behaviours of the example game. `fynkel codegen` (build_runner)
/// turns every `@FynkelBehaviour` class here into
/// `lib/generated/behaviours.g.dart` and the schemas under
/// `schemas/behaviours/`.
library;

import 'dart:math' as math;

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

/// Walks its object along [waypoints] (local to where it started) at
/// [speed], and counts as arrived within [reach] of each.
///
/// The `@Gizmo` annotations are ADR-0019's declarative gizmos: the editor
/// draws the path and the reach sphere from the generated metadata, and
/// drags them as handles.
@FynkelBehaviour(id: 'game.Patrol', icon: 'route', category: 'Motion')
class Patrol extends Behaviour {
  /// The points to visit, in order, then back to the first.
  @Expose()
  @Gizmo.path(color: 0xFFFFC107)
  List<Vec3> waypoints = [Vec3.zero, Vec3(4, 0, 0), Vec3(4, 0, -4)];

  /// Metres from a waypoint that count as reaching it.
  @Expose(min: 0.01, step: 0.1, unit: 'm')
  @Gizmo.radius(when: GizmoWhen.always)
  double reach = 0.5;

  /// Metres per second.
  @Expose(min: 0, step: 0.1, unit: 'm/s')
  double speed = 2;

  int next = 0;
  late Vec3 _origin;

  @override
  void start() => _origin = transform.position;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (waypoints.isEmpty) return;
    next %= waypoints.length;
    final target = _origin + waypoints[next];
    final to = target - transform.position;
    if (to.length <= reach) {
      next++;
      return;
    }
    transform.position += to.normalized * math.min(speed * context.dt, to.length);
  }
}

/// How a [Chime] sounds.
enum ChimeMood { soft, bright }

/// Counts through [delays], one chime each, then starts over; shows the
/// `List<T>` fields (ADR-0027 §5): the Inspector adds, removes and
/// reorders their items.
@FynkelBehaviour(id: 'game.Chime', icon: 'notifications', category: 'Audio')
class Chime extends Behaviour {
  /// Seconds before each chime, in order.
  @Expose(min: 0, step: 0.1, unit: 's')
  List<double> delays = [0.5, 1];

  /// The sound of each chime (for the Inspector; unused).
  @Expose(kind: 'asset(audio)')
  List<AssetRef> sounds = [];

  /// The mood of each chime (for the Inspector; unused).
  @Expose()
  List<ChimeMood> moods = [ChimeMood.soft];

  /// Chimes so far (not authored).
  int chimes = 0;
  double _wait = 0;

  @override
  void fixedUpdate(FixedUpdateContext context) {
    if (delays.isEmpty) return;
    _wait += context.dt;
    final delay = delays[chimes % delays.length];
    if (_wait >= delay) {
      _wait -= delay;
      chimes++;
    }
  }
}
