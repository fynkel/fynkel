part of 'physics.dart';

/// Keeps a camera rig's view of its target clear (Cinemachine's
/// Deoccluder): each frame, after the rigs have put the camera, it casts a
/// sphere of [radius] from the point the camera looks at out to the camera,
/// and where a collider is in the way, brings the camera in front of it,
/// along the same line, so it still looks at the target. Put it beside an
/// [Orbit], or a [Follow] and [LookAt]:
///
/// ```dart
/// GameObject(components: [
///   Camera(),
///   Orbit(player, distance: 6, offset: Vec3(0, 1.5, 0)),
///   CameraCollision(),
/// ])
/// ```
///
/// It comes in at once (or over [dampingIn]) and goes back out over
/// [damping], so it doesn't pop back and forth past a post. Pulled in
/// close, the player fills the view: hide it below a [distance]. It never stops at the target's own
/// colliders (those on it and its children), nor at a trigger; leave out
/// more with [layers]. The rig keeps its own distance: an [Orbit]'s
/// [Orbit.distance] is still what the player zoomed to.
final class CameraCollision extends CameraExtension {
  /// A camera kept clear by a sphere of [radius] 0.2 m, on every layer,
  /// no nearer its target than [minDistance] 0.3 m, going back out over
  /// [damping] 0.3 s. See the fields for [target] and [offset].
  CameraCollision({
    this.target,
    this.offset = Vec3.zero,
    this.radius = 0.2,
    this.layers = LayerMask.all,
    this.minDistance = 0.3,
    this.damping = 0.3,
    this.dampingIn = 0,
  });

  /// The object kept in sight. Null, unless set, takes the rig's on the
  /// same object: an [Orbit]'s, then a [LookAt]'s, then a [Follow]'s.
  GameObject? target;

  /// From [target]'s position to the point kept in sight, in the world,
  /// when [target] is given. With a rig's target, the rig's offset is used
  /// (an [Orbit]'s or a [LookAt]'s; a [Follow] looks at its target's
  /// position).
  Vec3 offset;

  /// How much room the camera keeps from what's in the way, in metres:
  /// about its near plane's half width, so it never sees into a wall.
  double radius;

  /// The layers that block the view: every layer unless set.
  LayerMask layers;

  /// The nearest it comes to the point kept in sight, in metres.
  double minDistance;

  /// Seconds it takes to go 99 % of the way back out once the view is
  /// clear; 0 goes straight back.
  double damping;

  /// Seconds it takes to come 99 % of the way in when something gets in
  /// the way: 0, unless set, comes in at once. More is smoother as a post
  /// passes, but for that moment the post can hide the target, or the
  /// camera see into it.
  double dampingIn;

  /// How far the camera is from the point it keeps in sight, as this put
  /// it last frame: to hide the player when the camera comes too near
  /// (`model.enabled = collision.distance > 0.8`).
  double get distance => _shown ?? 0;

  /// Whether something is in the way now, so the camera is nearer than
  /// its rig put it.
  bool get isPulledIn => _pulledIn;
  var _pulledIn = false;

  // How far from the point kept in sight it was last frame, or null to
  // start again.
  double? _shown;

  @override
  void onEnable() {
    super.onEnable();
    _shown = null;
    _pulledIn = false;
  }

  /// The point it keeps in sight, and whose colliders it never stops at.
  (Vec3, GameObject)? _look() {
    final given = target;
    if (given != null) return _present(given) ? (given.position + offset, given) : null;
    final orbit = get<Orbit>();
    if (orbit != null && orbit.enabled) {
      return _present(orbit.target) ? (orbit.target.position + orbit.offset, orbit.target) : null;
    }
    final lookAt = get<LookAt>();
    if (lookAt != null && lookAt.enabled) {
      return _present(lookAt.target) ? (lookAt.target.position + lookAt.offset, lookAt.target) : null;
    }
    final follow = get<Follow>();
    if (follow != null && follow.enabled) {
      return _present(follow.target) ? (follow.target.position, follow.target) : null;
    }
    return null;
  }

  static bool _present(GameObject o) => o.isAlive;

  @override
  void adjust(double dt) {
    final look = _look();
    if (look == null) return;
    final (point, of) = look;
    final out = position - point;
    final full = out.length;
    if (!(full > 1e-6)) return;
    final direction = out / full;
    var clear = full;
    final hits = physics.sphereCastAll(
      Ray(point, direction),
      radius,
      maxDistance: full,
      layers: layers,
      triggers: QueryTriggers.ignore,
    );
    for (var i = 0; i < hits.length; i++) {
      final hit = hits[i];
      // One the sphere starts in: the target's own, or a wall it leans on.
      if (hit.distance <= 0 || _isOf(hit.gameObject, of)) continue;
      clear = math.min(full, math.max(hit.distance, minDistance));
      break;
    }
    final shown = _shown;
    final next = shown == null
        ? clear
        : shown + (clear - shown) * (clear < shown ? _damp(dampingIn, dt) : _damp(damping, dt));
    _shown = math.min(next, full);
    _pulledIn = _shown! < full - 1e-4;
    if (_pulledIn) position = point + direction * _shown!;
  }

  static double _damp(double seconds, double dt) => seconds > 0 ? 1 - math.pow(0.01, dt / seconds).toDouble() : 1;

  static bool _isOf(GameObject hit, GameObject target) {
    for (GameObject? o = hit; o != null; o = o.parent) {
      if (identical(o, target)) return true;
    }
    return false;
  }

  @override
  String toString() => 'CameraCollision(${target ?? 'the rig\'s target'})';
}
