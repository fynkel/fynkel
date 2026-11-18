part of 'world.dart';

// Splines on objects (docs/stage-13.md §3.3), as Unity's Splines package:
// a SplineContainer holds splines in its object's space; SplineInstantiate
// places copies along one, again when it changes; SplineAnimate moves its
// object along one. world.debug.drawSplines draws every container's.

/// Splines in its object's space, as Unity's SplineContainer: a road's,
/// a track's, where a fence goes. Move the object and its splines move
/// with it; [positionAt] and the rest give points in the world, and take
/// and give distances in metres in the world too: on an object scaled 2,
/// a spline's 10 m are 20 ([length]).
///
/// ```dart
/// final track = GameObject(components: [SplineContainer([Spline.smooth(points, closed: true)])]);
/// final cart = GameObject(components: [SplineAnimate(track.get<SplineContainer>()!, speed: 4)]);
/// ```
final class SplineContainer extends Behaviour {
  /// A container of [splines], none unless given.
  SplineContainer([List<Spline>? splines]) : splines = splines ?? [];

  /// Its splines, in its object's space: add, take away and change them
  /// as the game likes.
  final List<Spline> splines;

  /// Its first spline (Unity's `Spline`). Throws a [StateError] if it has
  /// none.
  Spline get spline {
    if (splines.isEmpty) throw StateError('this SplineContainer has no splines');
    return splines.first;
  }

  Spline _at(int index) {
    RangeError.checkValidIndex(index, splines, 'index');
    return splines[index];
  }

  /// How long spline [index] is in the world, in metres: its own length
  /// times the object's scale.
  double length({int index = 0}) {
    final spline = _at(index);
    final s = gameObject.worldTransform.scale;
    final x = s.x.abs(), y = s.y.abs(), z = s.z.abs();
    if (x == y && y == z) return spline.length * x;
    spline._scaleTo(x, y, z);
    return spline._scaledLength;
  }

  /// The point [distance] metres along spline [index], in the world.
  Vec3 positionAt(double distance, {int index = 0}) {
    final spline = _at(index);
    return transformPoint(spline.positionAt(_alongOwn(spline, distance)));
  }

  /// The way spline [index] goes [distance] metres along, in the world,
  /// unit length.
  Vec3 directionAt(double distance, {int index = 0}) {
    final spline = _at(index);
    return _turned(spline.directionAt(_alongOwn(spline, distance)));
  }

  /// Which way is up [distance] metres along spline [index], in the world.
  Vec3 upAt(double distance, {int index = 0}) {
    final spline = _at(index);
    return _turned(spline.upAt(_alongOwn(spline, distance)));
  }

  /// The rotation that faces along spline [index] [distance] metres
  /// along, in the world: an object turned so has its -Z along it
  /// ([directionAt]) and its up as near [upAt] as it can be.
  Quat rotationAt(double distance, {int index = 0}) {
    final spline = _at(index);
    final own = _alongOwn(spline, distance);
    return Quat.lookRotation(_turned(spline.directionAt(own)), up: _turned(spline.upAt(own)));
  }

  /// The point of spline [index] nearest [point], given in the world: how
  /// far along it is, in metres in the world, and where in the world.
  ({double distance, Vec3 position}) nearest(Vec3 point, {int index = 0}) {
    final spline = _at(index);
    final found = spline.nearest(inverseTransformPoint(point));
    final s = gameObject.worldTransform.scale;
    final x = s.x.abs(), y = s.y.abs(), z = s.z.abs();
    final double distance;
    if (x == y && y == z) {
      distance = found.distance * x;
    } else {
      spline._scaleTo(x, y, z);
      distance = spline._toScaled(found.distance);
    }
    return (distance: distance, position: transformPoint(found.position));
  }

  // [distance] metres along [spline] in the world, as a distance in its
  // own space: the object's scale undone.
  double _alongOwn(Spline spline, double distance) {
    final s = gameObject.worldTransform.scale;
    final x = s.x.abs(), y = s.y.abs(), z = s.z.abs();
    if (x == y && y == z) return x == 0 ? 0 : distance / x;
    spline._scaleTo(x, y, z);
    return spline._unscaled(distance);
  }

  // A direction in the object's space, in the world's, unit length.
  Vec3 _turned(Vec3 v) {
    final t = gameObject.worldTransform;
    return t.rotation.rotate(v.scaledBy(t.scale)).normalized;
  }

  @override
  void onEnable() {
    super.onEnable();
    world._splineContainers.add(this);
  }

  @override
  void onDisable() {
    world._splineContainers.remove(this);
    super.onDisable();
  }

  @override
  String toString() => 'SplineContainer(${splines.length} splines)';
}

/// Copies of an object placed along a spline, as Unity's
/// SplineInstantiate: fence posts, lamp posts, trees along a road. Each
/// copy is made by [make], hung from this behaviour's object, and placed
/// again when the spline changes; there are as many as fit a [spacing]
/// apart, or [count] of them, spread from end to end.
///
/// ```dart
/// GameObject(components: [
///   SplineContainer([Spline.smooth(points)]),
///   SplineInstantiate((i) => GameObject(components: [MeshRenderer(post)]), spacing: 2),
/// ]);
/// ```
final class SplineInstantiate extends Behaviour {
  /// Copies made by [make] along [container]'s spline [index] (this
  /// object's own container unless given), [spacing] metres apart, or
  /// [count] of them.
  SplineInstantiate(
    this.make, {
    this._spacing = 1,
    this._count,
    this._align = true,
    this._offset = Vec3.zero,
    this.container,
    this._index = 0,
  }) {
    _checkSettings();
  }

  /// Makes copy `i` (from 0, along the spline): a new object each time,
  /// not yet added.
  GameObject Function(int i) make;

  /// The container whose spline the copies go along; this object's own
  /// when null.
  SplineContainer? container;

  double _spacing;
  int? _count;
  bool _align;
  Vec3 _offset;
  int _index;

  /// How far apart the copies are along it, in metres in the spline's
  /// own space (scaling the container's object spaces them out with it),
  /// when [count] is null: as many as fit, the first at its start. More
  /// than 0.
  double get spacing => _spacing;
  set spacing(double value) => _set(() => _spacing = value);

  /// How many copies, spread evenly from its start to its end (around
  /// it, for a closed one); null for as many as fit [spacing] apart. A
  /// spline with no knots has none.
  int? get count => _count;
  set count(int? value) => _set(() => _count = value);

  /// Whether each copy is turned to face along it, its -Z forward and its
  /// up the spline's; otherwise each keeps its own rotation.
  bool get align => _align;
  set align(bool value) => _set(() => _align = value);

  /// Where each copy is from the spline, X to the right, Y up and -Z
  /// forward along it.
  Vec3 get offset => _offset;
  set offset(Vec3 value) => _set(() => _offset = value);

  /// Which of the container's splines.
  int get index => _index;
  set index(int value) => _set(() => _index = value);

  final _copies = <GameObject>[];
  Spline? _placedOn;
  var _placedVersion = -1;

  /// The copies along it now, first to last.
  List<GameObject> get copies => List.unmodifiable(_copies);

  void _set(void Function() change) {
    change();
    _checkSettings();
    _placedVersion = -1;
  }

  void _checkSettings() {
    if (!(_spacing > 0 && _spacing.isFinite)) throw ArgumentError.value(_spacing, 'spacing', 'must be more than 0');
    if (_count case final n? when n < 0) throw ArgumentError.value(n, 'count', 'can\'t be less than 0');
    if (!(_offset.x.isFinite && _offset.y.isFinite && _offset.z.isFinite)) {
      throw ArgumentError('offset is $_offset; it must be finite');
    }
  }

  SplineContainer get _container {
    final own = container ?? gameObject.get<SplineContainer>();
    if (own == null) throw StateError('$this has no SplineContainer: put one on its object, or give it one');
    return own;
  }

  /// Places the copies again now, making or destroying them so there are
  /// as many as there should be. It runs by itself when the spline or a
  /// setting changes; call it after moving a container on another object.
  void rebuild() {
    final container = _container;
    final spline = container._at(_index);
    final length = spline.length;
    final closed = spline.closed;
    // A spline with no knots has nowhere to put any.
    final n = switch (_count) {
      _ when spline.knotCount == 0 => 0,
      final n? => n,
      _ when spline.knotCount < 2 => 0,
      _ => closed ? (length / _spacing).floor() : (length / _spacing + 1e-9).floor() + 1,
    };
    while (_copies.length > n) {
      _copies.removeLast().destroy();
    }
    while (_copies.length < n) {
      final copy = make(_copies.length);
      _copies.add(gameObject.add(copy));
    }
    // From the spline's space into this object's.
    final own = identical(container.gameObject, gameObject);
    final step = _count == null
        ? _spacing
        : closed
        ? length / math.max(n, 1)
        : n > 1
        ? length / (n - 1)
        : 0.0;
    for (var i = 0; i < n; i++) {
      final copy = _copies[i];
      final d = i * step;
      final turn = spline.rotationAt(d);
      var at = spline.positionAt(d) + turn.rotate(_offset);
      var rotation = turn;
      if (!own) {
        at = inverseTransformPoint(container.transformPoint(at));
        rotation = (gameObject.worldTransform.rotation.inverse * container.gameObject.worldTransform.rotation * turn)
            .normalized;
      }
      final t = copy.transform;
      copy.transform = Transform(position: at, rotation: _align ? rotation : t.rotation, scale: t.scale);
    }
    _placedOn = spline;
    _placedVersion = spline.version;
  }

  void _maybeRebuild() {
    final spline = _container._at(_index);
    if (!identical(spline, _placedOn) || spline.version != _placedVersion) rebuild();
  }

  @override
  void onStart() {
    super.onStart();
    rebuild();
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _copies.removeWhere((c) => c.isDestroyed);
    _maybeRebuild();
  }

  @override
  String toString() => 'SplineInstantiate(${_copies.length} copies)';
}

/// What a [SplineAnimate] does at the end of its spline.
enum SplineLoop {
  /// Stops there.
  once,

  /// Starts again from the start (round again, on a closed one).
  loop,

  /// Comes back, and goes again, for ever.
  pingPong,
}

/// Moves its object along a spline, as Unity's SplineAnimate: a cart on
/// its track, a boat down a river, a camera on a dolly. It goes at
/// [speed] metres a second, or takes [seconds] for the whole spline (give
/// one), turned to face along it when [align]. When its container goes
/// (its object destroyed, as a level unloads), it holds still where it
/// is, as [Follow] does; give it another [container] to go on.
///
/// ```dart
/// cart.addComponent(SplineAnimate(track, speed: 4, loop: SplineLoop.loop));
/// ```
final class SplineAnimate extends Behaviour {
  /// Along [container]'s spline [index], from [startAt] metres along.
  SplineAnimate(
    this.container, {
    double? speed,
    double? seconds,
    this.loop = SplineLoop.loop,
    this.align = true,
    this.playOnStart = true,
    double startAt = 0,
    this.index = 0,
    this.onComplete,
  }) : _speed = speed ?? 0,
       _seconds = seconds,
       _distance = startAt {
    if ((speed == null) == (seconds == null)) throw ArgumentError('give a SplineAnimate one of speed and seconds');
    if (speed != null && !(speed >= 0 && speed.isFinite)) {
      throw ArgumentError.value(speed, 'speed', 'must be 0 or more');
    }
    if (seconds != null && !(seconds > 0 && seconds.isFinite)) {
      throw ArgumentError.value(seconds, 'seconds', 'must be more than 0');
    }
    if (!startAt.isFinite) throw ArgumentError.value(startAt, 'startAt', 'must be finite');
  }

  /// The container whose spline it rides.
  SplineContainer container;

  /// Which of the container's splines.
  int index;

  /// What it does at the end: stops, starts again or comes back.
  SplineLoop loop;

  /// Whether the object is turned to face along it, its -Z forward and
  /// its up the spline's; otherwise only moved.
  bool align;

  /// Whether it starts moving by itself; otherwise at [play].
  bool playOnStart;

  /// Called each time it reaches the end with [SplineLoop.once].
  void Function()? onComplete;

  // Whether its container is still there: when it was taken away, or its
  // object destroyed, it holds still where it is, as [Follow] does.
  bool get _hasTrack => !container.isRemoved && container._owner != null && _present(container._owner!);

  double _speed;
  double? _seconds;
  double _distance;
  var _back = false;
  var _playing = false;

  /// How fast it goes, in metres a second; with [seconds] given, the
  /// spline's length over them.
  double get speed {
    final seconds = _seconds;
    return seconds == null ? _speed : container.length(index: index) / seconds;
  }

  set speed(double value) {
    if (!(value >= 0 && value.isFinite)) throw ArgumentError.value(value, 'speed', 'must be 0 or more');
    _speed = value;
    _seconds = null;
  }

  /// How long the whole spline takes, if it was given so; null when it
  /// goes at a [speed].
  double? get seconds => _seconds;
  set seconds(double? value) {
    if (value != null && !(value > 0 && value.isFinite)) {
      throw ArgumentError.value(value, 'seconds', 'must be more than 0');
    }
    if (value == null) _speed = speed;
    _seconds = value;
  }

  /// How far along it the object is, in metres. Setting it puts it there.
  double get distance => _distance;
  set distance(double value) {
    if (!value.isFinite) throw ArgumentError.value(value, 'distance', 'must be finite');
    _distance = value;
    if (_loaded) _place();
  }

  /// How far along it is, 0 at the start and 1 at the end (Unity's
  /// `NormalizedTime`, within one go).
  double get normalizedTime {
    final length = container.length(index: index);
    return length == 0 ? 0 : _distance / length;
  }

  set normalizedTime(double value) => distance = value * container.length(index: index);

  /// Whether it's moving.
  bool get isPlaying => _playing;

  /// Moves it from where it is.
  void play() => _playing = true;

  /// Stops it where it is.
  void pause() => _playing = false;

  /// Back to the start, going forward, and moving.
  void restart() {
    _back = false;
    _playing = true;
    distance = 0;
  }

  void _place() {
    final c = container;
    final at = c.positionAt(_distance, index: index);
    if (align) {
      var turn = c.rotationAt(_distance, index: index);
      // Coming back, it faces the way it goes.
      if (_back) turn = (turn * Quat.axisAngle(Vec3.up, math.pi)).normalized;
      position = at;
      rotation = turn;
    } else {
      position = at;
    }
    if (c.length(index: index) == 0) _playing = false;
  }

  @override
  void onStart() {
    super.onStart();
    _playing = playOnStart;
    _place();
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (!_playing || !_hasTrack) return;
    final length = container.length(index: index);
    final step = speed * world.time.delta;
    var d = _distance + (_back ? -step : step);
    var ended = false;
    switch (loop) {
      case SplineLoop.once:
        if (d >= length) (d, ended) = (length, true);
      case SplineLoop.loop:
        if (d >= length && length > 0) d %= length;
      case SplineLoop.pingPong:
        if (d >= length) {
          d = 2 * length - d;
          _back = true;
        }
        if (d <= 0) {
          d = -d;
          _back = false;
        }
        d = d.clamp(0.0, length);
    }
    _distance = d;
    _place();
    if (ended) {
      _playing = false;
      onComplete?.call();
    }
  }

  @override
  String toString() => 'SplineAnimate(${_distance.toStringAsFixed(2)} m)';
}
