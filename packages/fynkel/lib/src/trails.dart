part of 'world.dart';

/// A strip left behind its object as it moves, as Unity's Trail Renderer:
/// a ball's streak, a sword's swipe, a coin's glint. A point is added each
/// time the object has gone [minVertexDistance] from the last, and each
/// lives [time] seconds; the strip runs from the object back through
/// them, facing the camera, [widthMultiplier] wide times [widthCurve],
/// coloured by [colorGradient], both from the head (0) to the tail (1).
/// It's drawn with its [material] as particles are: unlit, times its
/// colour and texture (stretched along the trail), [Blending.additive] for
/// a glow, else see-through by alpha.
///
/// The engine adds and drops its points once a frame, with the particles;
/// Dart only sends the settings when they change. Turned off ([enabled]
/// false) or with its object hidden, its points go. It keeps at most 512.
///
/// Each setter sends the settings at once, and throws an [ArgumentError]
/// for one the engine refuses, keeping the old value.
final class TrailRenderer extends Component {
  /// Unity's defaults, but for what's given: five seconds, a point each
  /// 0.1 m, a metre wide, white. Without a [material] nothing is drawn.
  TrailRenderer({
    this._time = 5,
    this._minVertexDistance = 0.1,
    this._widthMultiplier = 1,
    EaseFunction? widthCurve,
    ColorGradient? colorGradient,
    this._emitting = true,
    this._material,
  }) : _widthCurve = widthCurve,
       _widthTable = ParticleSystem._sizes(widthCurve),
       _colorGradient = colorGradient,
       _colorTable = ParticleSystem._colors(colorGradient),
       super._() {
    if (_refused() case final error?) throw error;
  }

  double _time, _minVertexDistance, _widthMultiplier;
  EaseFunction? _widthCurve;
  Float32List? _widthTable;
  ColorGradient? _colorGradient;
  Float32List? _colorTable;
  bool _emitting;
  Material? _material;

  /// How long each point lives, in seconds: how long the trail is behind
  /// a moving object.
  double get time => _time;
  set time(double value) {
    _checkChangeable();
    final old = _time;
    _time = value;
    if (_write() case final failed?) {
      _time = old;
      failed.raise();
    }
  }

  /// How far, in metres, the object goes before another point is added:
  /// smaller is smoother round a curve, with more points.
  double get minVertexDistance => _minVertexDistance;
  set minVertexDistance(double value) {
    _checkChangeable();
    final old = _minVertexDistance;
    _minVertexDistance = value;
    if (_write() case final failed?) {
      _minVertexDistance = old;
      failed.raise();
    }
  }

  /// Its width, in metres, times [widthCurve].
  double get widthMultiplier => _widthMultiplier;
  set widthMultiplier(double value) {
    _checkChangeable();
    final old = _widthMultiplier;
    _widthMultiplier = value;
    if (_write() case final failed?) {
      _widthMultiplier = old;
      failed.raise();
    }
  }

  /// Its width along it, times [widthMultiplier]: any function of 0 (at
  /// the object) to 1 (the tail), `(t) => 1 - t` to taper it. Read at 64
  /// points when it's set. Null for the same all along.
  EaseFunction? get widthCurve => _widthCurve;
  set widthCurve(EaseFunction? value) {
    final (oldF, oldT) = (_widthCurve, _widthTable);
    _set(() => (oldF, oldT), (v) {
      _widthCurve = v.$1;
      _widthTable = v.$2;
    }, (value, ParticleSystem._sizes(value)));
  }

  /// Its colour along it, from the object (0) to the tail (1), times the
  /// [material]'s: [ColorGradient.fadeOut] to fade it away. Null for
  /// white.
  ColorGradient? get colorGradient => _colorGradient;
  set colorGradient(ColorGradient? value) {
    final (oldG, oldT) = (_colorGradient, _colorTable);
    _set(() => (oldG, oldT), (v) {
      _colorGradient = v.$1;
      _colorTable = v.$2;
    }, (value, ParticleSystem._colors(value)));
  }

  /// Adding points as it moves; off, the ones it has live out their time,
  /// so it can be turned off to let a trail end.
  bool get emitting => _emitting;
  set emitting(bool value) {
    _checkChangeable();
    final old = _emitting;
    _emitting = value;
    if (_write() case final failed?) {
      _emitting = old;
      failed.raise();
    }
  }

  /// What it's drawn with; without one, nothing is.
  Material? get material => _material;
  set material(Material? value) {
    _checkChangeable();
    final old = _material;
    _material = value;
    if (_write() case final failed?) {
      _material = old;
      failed.raise();
    }
  }

  /// How many points it has now.
  int get positionCount {
    final object = _live;
    if (object == null || !_enabledFlag) return 0;
    check(fy_trail_count(object.world.pointer, object.id, scratch.count2));
    return scratch.count2.value;
  }

  /// Takes all its points away: after a ball is put back on the tee.
  void clear() {
    _checkChangeable();
    final object = _live;
    if (object == null || !_enabledFlag) return;
    check(fy_trail_clear(object.world.pointer, object.id));
  }

  void _set<T>(T Function() read, void Function(T) write, T value) {
    _checkChangeable();
    final old = read();
    write(value);
    if (_write() case final failed?) {
      write(old);
      failed.raise();
    }
  }

  @override
  void _put(GameObject object) => object._writeTrail(this);

  @override
  void _takeOut(GameObject object) {
    check(fy_trail_remove(object.world.pointer, object.id));
    _forget(object);
  }

  @override
  void _forget(GameObject object) => object.world.maybeSystem<_ParticleWorld>()?._forgetTrail(object, this);

  _Failure? _write() {
    if (_refused() case final error?) return _Failure(error, StackTrace.current);
    if (!_enabledFlag) return null;
    try {
      _live?._writeTrail(this);
      return null;
    } catch (error, stack) {
      return _Failure(error, stack);
    }
  }

  ArgumentError? _refused() {
    if (!_time.isFinite || _time < 0) return ArgumentError.value(_time, 'time', 'must be 0 or more');
    if (!_minVertexDistance.isFinite || _minVertexDistance < 0) {
      return ArgumentError.value(_minVertexDistance, 'minVertexDistance', 'must be 0 or more');
    }
    if (!_widthMultiplier.isFinite || _widthMultiplier < 0) {
      return ArgumentError.value(_widthMultiplier, 'widthMultiplier', 'must be 0 or more');
    }
    if (_widthTable case final t? when t.any((v) => !v.isFinite)) {
      return ArgumentError('widthCurve must give finite numbers from 0 to 1');
    }
    return null;
  }

  @override
  String toString() => 'TrailRenderer(time: $time, widthMultiplier: $widthMultiplier)';
}
