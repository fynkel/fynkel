part of 'world.dart';

// Cameras (docs/stage-4.md §3.8), as Cinemachine: the view has one real
// camera, and virtual cameras and rigs decide where it is. They run last
// in the frame, after the late updates, so they see where everything went:
// the rigs move their objects (Follow and Orbit, then LookAt), then the
// world's [Cameras] puts the camera the view draws from where the live
// virtual camera is, blending from the last one, and shakes it.

/// How the view's camera goes from one virtual camera to the next: over
/// [seconds], with [ease]. [Blend.cut] goes straight there.
final class Blend {
  const Blend(this.seconds, {this.ease = Ease.inOutQuad});

  /// Straight to the next camera, in one frame.
  static const cut = Blend(0);

  final double seconds;
  final EaseFunction ease;

  /// Whether it goes straight there.
  bool get isCut => !(seconds > 0);

  @override
  String toString() => isCut ? 'Blend.cut' : 'Blend(${seconds}s)';
}

/// Where a virtual camera would put the view's camera, and its lens.
final class _Pose {
  const _Pose(this.position, this.rotation, this.fovY, this.size);

  final Vec3 position;
  final Quat rotation;
  final double fovY, size;
}

/// A place the view's camera can be: put it on an object, move the object
/// (by hand, a tween, or a rig such as [Follow]), and while it's the live
/// one the view's camera is where it is. The live one is the enabled one
/// with the highest [priority]; of equal ones, the one enabled or given
/// its priority last. Going from one to another blends ([Cameras]).
///
/// ```dart
/// final close = GameObject(components: [VirtualCamera(priority: 1), Follow(player)]);
/// close.get<VirtualCamera>()!.priority = 20; // the view blends to it
/// ```
final class VirtualCamera extends Behaviour {
  VirtualCamera({this._priority = 0, this.fovY = 60, this.size = 5});

  int _priority;
  int _order = 0;
  static var _orders = 0;

  /// The highest wins. Setting it makes this the newest of its priority.
  int get priority => _priority;
  set priority(int value) {
    _priority = value;
    _order = ++_orders;
  }

  /// The view's field of view, in degrees, while this is live ([Camera.fovY]).
  double fovY;

  /// Half the view's height, while this is live and the camera is
  /// orthographic ([Camera.size]).
  double size;

  /// Whether the view's camera follows this one now (or is blending to it).
  bool get isLive => _enabledNow && world.cameras.live == this;

  _Pose get _pose {
    final object = gameObject;
    final t = object.parent == null ? object.transform : object.worldTransform;
    return _Pose(t.position, t.rotation, fovY, size);
  }

  @override
  void onEnable() {
    super.onEnable();
    _order = ++_orders;
    world.cameras._virtual.add(this);
  }

  @override
  void onDisable() {
    world.cameras._virtual.remove(this);
    super.onDisable();
  }

  @override
  String toString() => 'VirtualCamera(priority: $priority)';
}

/// A behaviour that moves its object as a camera rig does, each frame
/// after the late updates.
sealed class _Rig extends Behaviour {
  /// Turns the object rather than moving it: runs after those that move.
  bool get _aims => false;

  void _move(double dt);

  /// Starts again from where it would be, without damping.
  void _reset() {}

  @override
  void onEnable() {
    super.onEnable();
    _reset();
    world.cameras._rigs.add(this);
  }

  @override
  void onDisable() {
    world.cameras._rigs.remove(this);
    super.onDisable();
  }
}

bool _present(GameObject o) => o._world != null && !o._gone;

/// How much of the way to go this frame with [damping]: all of it with
/// none; with some, 99 % of the way after [damping] seconds (Cinemachine's).
double _damp(double damping, double dt) => damping > 0 ? 1 - math.pow(0.01, dt / damping).toDouble() : 1;

/// Keeps its object at [offset] from [target], in the world's directions:
/// `Follow(player, offset: Vec3(0, 6, 8))` stays above and behind it. It
/// moves the object, and doesn't turn it: add a [LookAt], or turn it once.
final class Follow extends _Rig {
  Follow(this.target, {this.offset = const Vec3(0, 3, 6), this.damping = 0, this.lookAhead = 0});

  GameObject target;

  /// From [target] to where the object is kept, in the world.
  Vec3 offset;

  /// Seconds it takes to close 99 % of the gap when [target] moves; 0
  /// keeps it right there.
  double damping;

  /// Seconds ahead of [target] it aims for, where it would be at the speed
  /// it's going: a camera that shows more of where the player is heading.
  double lookAhead;

  Vec3? _at, _targetWas;

  @override
  void _reset() => _at = _targetWas = null;

  @override
  void _move(double dt) {
    if (!_present(target)) return;
    final p = target.position;
    var goal = p + offset;
    final was = _targetWas;
    _targetWas = p;
    if (lookAhead != 0 && was != null && dt > 0) goal += (p - was) * (lookAhead / dt);
    final at = _at;
    final next = at == null ? goal : Vec3.lerp(at, goal, _damp(damping, dt));
    _at = next;
    gameObject._setPosition(next);
  }

  @override
  String toString() => 'Follow($target)';
}

/// Turns its object to look at [target] (plus [offset], in the world),
/// with its up as close to the world's as it can be.
final class LookAt extends _Rig {
  LookAt(this.target, {this.offset = Vec3.zero, this.damping = 0});

  GameObject target;

  /// From [target]'s position to the point looked at, in the world.
  Vec3 offset;

  /// Seconds it takes to turn 99 % of the way when [target] moves; 0
  /// keeps it right on it.
  double damping;

  Quat? _turn;

  @override
  bool get _aims => true;

  @override
  void _reset() => _turn = null;

  @override
  void _move(double dt) {
    if (!_present(target)) return;
    final to = target.position + offset - gameObject.position;
    if (to.length < 1e-9) return;
    final goal = Quat.lookRotation(to);
    final turn = _turn;
    final next = turn == null ? goal : Quat.slerp(turn, goal, _damp(damping, dt));
    _turn = next;
    gameObject._setRotation(next);
  }

  @override
  String toString() => 'LookAt($target)';
}

/// Circles [target] at [distance], looking at it: [yaw] around it (0 is
/// on its +Z side, the way a camera looks along -Z), [pitch] above it.
/// With [controls] the player turns it by dragging (where no object took
/// the drag), and zooms with a pinch or the mouse wheel, within the limits.
final class Orbit extends _Rig {
  Orbit(
    this.target, {
    this.distance = 8,
    this.yaw = 0,
    this.pitch = 0.4,
    this.offset = Vec3.zero,
    this.minPitch = -0.2,
    this.maxPitch = 1.45,
    this.minDistance = 1,
    this.maxDistance = 100,
    this.controls = true,
    this.dragSpeed = 0.01,
    this.scrollSpeed = 0.002,
  });

  GameObject target;

  /// From [target], in metres.
  double distance;

  /// Around [target], in radians: 0 on its +Z side, growing to the right.
  double yaw;

  /// Above [target], in radians: 0 level, π/2 straight down on it.
  double pitch;

  /// From [target]'s position to the point circled, in the world.
  Vec3 offset;

  double minPitch, maxPitch, minDistance, maxDistance;

  /// Whether dragging, pinching and the mouse wheel move it.
  bool controls;

  /// Radians a logical pixel of drag turns it.
  double dragSpeed;

  /// How much a logical pixel of scroll zooms: the distance grows by
  /// e^(pixels × scrollSpeed).
  double scrollSpeed;

  @override
  void onStart() {
    super.onStart();
    on(input.onDrag, (finger) {
      if (!controls || !_enabledNow || finger._captor != null) return;
      // Two fingers pinch rather than turn.
      var down = 0;
      for (final f in input.fingers) {
        if (f.isDown) down++;
      }
      if (down > 1) return;
      final d = finger.delta;
      yaw -= d.x * dragSpeed;
      pitch += d.y * dragSpeed;
    });
    on(input.onPinch, (pinch) {
      if (controls && _enabledNow && pinch.scale > 0) distance /= pinch.scale;
    });
    on(input.onScroll, (scroll) {
      if (controls && _enabledNow) distance *= math.exp(scroll.y * scrollSpeed);
    });
  }

  @override
  void _move(double dt) {
    if (!_present(target)) return;
    pitch = pitch.clamp(minPitch, maxPitch);
    distance = distance.clamp(minDistance, maxDistance);
    final centre = target.position + offset;
    final around = Vec3(math.sin(yaw) * math.cos(pitch), math.sin(pitch), math.cos(yaw) * math.cos(pitch));
    gameObject._setPosition(centre + around * distance);
    gameObject._setRotation(Quat.lookRotation(-around));
  }

  @override
  String toString() => 'Orbit($target)';
}

/// The world's cameras ([World.cameras]): each frame after the late
/// updates, it puts the camera the view draws from ([World.camera]) where
/// the live [VirtualCamera] is, with its lens, blending when the live one
/// changes, and adds the shakes on top. With no virtual camera it leaves
/// the camera where it is (still shaking it).
final class Cameras {
  Cameras._(this._world);

  final World _world;
  final _virtual = <VirtualCamera>[];
  final _rigs = <_Rig>[];
  final _blends = <(VirtualCamera?, VirtualCamera?, Blend)>[];

  /// How the view goes from one virtual camera to another, unless
  /// [setBlend] says otherwise for the pair.
  Blend defaultBlend = const Blend(1);

  /// Whether blends go on while the game is paused ([Time.scale] 0).
  bool unscaled = false;

  /// The live virtual camera changed: the new one (null for none).
  final onLiveChanged = Signal<VirtualCamera?>();

  VirtualCamera? _live;
  VirtualCamera? _from;
  _Pose? _fromPose, _current;
  Blend? _blend;
  double _blendTime = 0;

  /// The virtual camera the view's camera follows (or blends to).
  VirtualCamera? get live => _live;

  /// Whether the view's camera is between two virtual cameras.
  bool get isBlending => _blend != null;

  /// Uses [blend] going [from] one virtual camera [to] another; leave one
  /// out for any. The most exact pair wins: both, then [to], then [from].
  void setBlend(Blend blend, {VirtualCamera? from, VirtualCamera? to}) {
    _blends
      ..removeWhere((b) => b.$1 == from && b.$2 == to)
      ..add((from, to, blend));
  }

  Blend _blendFor(VirtualCamera from, VirtualCamera to) {
    Blend? any, toOnly, fromOnly;
    for (final (f, t, blend) in _blends) {
      if (f == from && t == to) return blend;
      if (f == null && t == to) toOnly = blend;
      if (f == from && t == null) fromOnly = blend;
      if (f == null && t == null) any = blend;
    }
    return toOnly ?? fromOnly ?? any ?? defaultBlend;
  }

  // --- Shake ---

  final _shakes = <_Shake>[];
  GameObject? _shaken;
  Transform? _wrote;
  Vec3 _base = Vec3.zero;

  /// Shakes the view's camera by up to [strength] metres, across the view,
  /// fading out over [seconds]; [vibrato] is about how many times a second
  /// it goes to and fro. On top of wherever it is: a virtual camera, a
  /// rig, a tween or the game's own code moving it, and it ends there.
  /// Shakes at once add up. [unscaled] keeps it going while the game is
  /// paused; [seed] makes it the same each time.
  void shake(double strength, {double seconds = 0.4, double vibrato = 20, bool unscaled = false, int? seed}) {
    if (!(seconds > 0)) return;
    final random = math.Random(seed);
    _shakes.add(
      _Shake(
        strength,
        seconds,
        vibrato,
        unscaled,
        random.nextDouble() * 2 * math.pi,
        random.nextDouble() * 2 * math.pi,
      ),
    );
  }

  /// Whether a shake is going on.
  bool get isShaking => _shakes.isNotEmpty;

  /// Stops every shake: the camera goes back to where it would be.
  void stopShaking() {
    for (final s in _shakes) {
      s.time = s.seconds;
    }
  }

  // --- The frame ---

  void _run() {
    // Nothing to do, and nothing made: a frame without cameras allocates
    // nothing.
    if (_rigs.isEmpty && _virtual.isEmpty && _live == null && _shakes.isEmpty && _shaken == null) return;
    final time = _world.time;
    final dt = time._delta;
    final rigs = _rigs;
    for (var i = 0; i < rigs.length; i++) {
      final rig = rigs[i];
      if (!rig._aims) _safely(rig, dt);
    }
    for (var i = 0; i < rigs.length; i++) {
      final rig = rigs[i];
      if (rig._aims) _safely(rig, dt);
    }
    if (_virtual.isEmpty && _live == null && _shakes.isEmpty && _shaken == null) return;

    final camera = _world.camera;
    _choose();
    final live = _live;
    if (live != null && camera != null && camera != live._owner) {
      final pose = _poseNow(live, unscaled ? time._unscaledDelta : dt);
      _current = pose;
      _put(camera, pose);
    }
    _shake(camera, dt, time._unscaledDelta);
  }

  void _safely(_Rig rig, double dt) {
    try {
      rig._move(dt);
    } catch (error, stack) {
      _world._report(rig._owner, error, stack);
    }
  }

  /// Makes the best virtual camera the live one, blending from the last.
  void _choose() {
    VirtualCamera? best;
    for (final v in _virtual) {
      if (best == null || v._priority > best._priority || (v._priority == best._priority && v._order > best._order)) {
        best = v;
      }
    }
    final last = _live;
    if (best == last) return;
    _live = best;
    final blend = best == null || last == null ? Blend.cut : _blendFor(last, best);
    if (blend.isCut) {
      _blend = null;
    } else {
      // From a blend still going: from where it got to, held there.
      final current = _current;
      final blending = _blend != null && current != null;
      _from = blending ? null : last;
      _fromPose = blending ? current : last!._pose;
      _blend = blend;
      _blendTime = 0;
    }
    onLiveChanged.emit(best);
  }

  _Pose _poseNow(VirtualCamera live, double dt) {
    final to = live._pose;
    final blend = _blend;
    if (blend == null) return to;
    _blendTime += dt;
    final from = _from;
    final fromPose = _fromPose = from != null && from._enabledNow ? from._pose : _fromPose!;
    final p = _blendTime / blend.seconds;
    if (p >= 1) {
      _blend = _from = _fromPose = null;
      return to;
    }
    final t = blend.ease(p);
    return _Pose(
      Vec3.lerp(fromPose.position, to.position, t),
      Quat.slerp(fromPose.rotation, to.rotation, t),
      lerp(fromPose.fovY, to.fovY, t),
      lerp(fromPose.size, to.size, t),
    );
  }

  static void _put(GameObject camera, _Pose pose) {
    if (camera.parent == null) {
      camera.transform = camera.transform.copyWith(position: pose.position, rotation: pose.rotation);
    } else {
      camera._setPosition(pose.position);
      camera._setRotation(pose.rotation);
    }
    final lens = camera.get<Camera>();
    if (lens == null) return;
    if (lens.orthographic) {
      if (lens.size != pose.size) lens.size = pose.size;
    } else if (lens.fovY != pose.fovY) {
      lens.fovY = pose.fovY;
    }
  }

  void _shake(GameObject? camera, double dt, double unscaledDt) {
    final shaken = _shaken;
    if (camera == null || (shaken != null && shaken != camera)) {
      // The view has another camera now: the old one goes back.
      if (shaken != null && _present(shaken)) shaken.transform = shaken.transform.copyWith(position: _base);
      _shaken = _wrote = null;
      if (camera == null) {
        _shakes.clear();
        return;
      }
    }
    if (_shakes.isEmpty && _shaken == null) return;
    final t = camera.transform;
    // Moved since it was shaken (by a virtual camera, a rig or the game):
    // that's where it shakes about now.
    if (_shaken == null || t != _wrote) _base = t.position;
    var x = 0.0, y = 0.0;
    for (var i = _shakes.length - 1; i >= 0; i--) {
      final s = _shakes[i];
      s.time += s.unscaled ? unscaledDt : dt;
      final p = s.time / s.seconds;
      if (p >= 1) {
        _shakes.removeAt(i);
        continue;
      }
      // Two waves across the view, together within the strength.
      final fade = s.strength * (1 - p) * (1 - p) * math.sqrt1_2;
      final turns = 2 * math.pi * s.vibrato * s.time;
      x += math.sin(turns + s.phaseX) * fade;
      y += math.sin(turns * 1.13 + s.phaseY) * fade;
    }
    if (_shakes.isEmpty) {
      camera.transform = t.copyWith(position: _base);
      _shaken = _wrote = null;
      return;
    }
    final moved = t.copyWith(position: _base + t.rotation.rotate(Vec3(x, y, 0)));
    camera.transform = moved;
    _wrote = moved;
    _shaken = camera;
  }
}

final class _Shake {
  _Shake(this.strength, this.seconds, this.vibrato, this.unscaled, this.phaseX, this.phaseY);

  final double strength, seconds, vibrato, phaseX, phaseY;
  final bool unscaled;
  double time = 0;
}
