part of 'world.dart';

/// How a [Knot]'s handles are worked out (Unity's `TangentMode`).
enum TangentMode {
  /// Smooth through the knot, its handles worked out from the knots either
  /// side, as Unity's Auto Smooth: a Catmull-Rom curve, weighted by how far
  /// each is, so it doesn't overshoot where knots are unevenly spaced.
  auto,

  /// Straight to the knots either side: a corner.
  linear,

  /// The knot's own handles, [Knot.tangentIn] and [Knot.tangentOut].
  bezier,
}

/// A point a [Spline] goes through (Unity's `BezierKnot`), with its handles
/// and how they're worked out.
final class Knot {
  /// A knot at [position]. With [TangentMode.bezier], the curve leaves it
  /// towards `position + tangentOut` and arrives from `position +
  /// tangentIn`, each a third of the way along its piece of curve; the
  /// other modes work them out. [roll] turns the spline's up around it
  /// there, in radians (`roll: 30.deg`): a road banked on a bend.
  const Knot(
    this.position, {
    this.mode = TangentMode.auto,
    this.tangentIn = Vec3.zero,
    this.tangentOut = Vec3.zero,
    this.roll = 0,
  });

  /// Where it is, in the spline's space.
  final Vec3 position;

  /// How its handles are worked out.
  final TangentMode mode;

  /// Its handles, from [position], with [TangentMode.bezier]: the way the
  /// curve arrives from, and the way it leaves towards.
  final Vec3 tangentIn, tangentOut;

  /// How far up is turned around the curve here, in radians, clockwise
  /// seen going along it.
  final double roll;

  @override
  bool operator ==(Object other) =>
      other is Knot &&
      other.position == position &&
      other.mode == mode &&
      other.tangentIn == tangentIn &&
      other.tangentOut == tangentOut &&
      other.roll == roll;

  @override
  int get hashCode => Object.hash(position, mode, tangentIn, tangentOut, roll);

  @override
  String toString() => 'Knot($position, ${mode.name}${roll == 0 ? '' : ', roll $roll'})';
}

/// A smooth curve through [Knot]s, as Unity's Spline: a road, a river, a
/// rope, the track a cart rides, where fence posts go. Plain Dart: finding
/// a point on it is a few multiplications, so it's as much for moving
/// things along and placing them as for [Mesh.sweep].
///
/// Everything along it is by **distance in metres** ([positionAt],
/// [directionAt], [upAt], [rotationAt]), so things go at an even speed and
/// are evenly spaced: a table of lengths is made again when it changes.
/// Unity's normalised t is `distance / length`. A [closed] spline joins
/// its last knot back to its first, and distances go round.
///
/// **Up** along it is rotation-minimising: it turns as little as it can
/// where the curve does, so a tube doesn't twist; it starts as +Y, and
/// each knot's [Knot.roll] turns it.
///
/// A spline that changes each frame (a rope) can be moved with
/// [setPositions] and swept with [Mesh.setSweep] without making garbage.
final class Spline {
  /// A spline through [knots] (`Spline([])` for none yet); [closed] joins
  /// the last back to the first.
  Spline(List<Knot> knots, {this._closed = false}) {
    for (var i = 0; i < knots.length; i++) {
      add(knots[i]);
    }
  }

  /// A smooth spline through [points] ([TangentMode.auto]).
  factory Spline.smooth(List<Vec3> points, {bool closed = false}) =>
      Spline([for (final p in points) Knot(p)], closed: closed);

  /// Straight lines through [points] ([TangentMode.linear]): a path with
  /// corners.
  factory Spline.linear(List<Vec3> points, {bool closed = false}) =>
      Spline([for (final p in points) Knot(p, mode: TangentMode.linear)], closed: closed);

  // Each knot's position (3), its handles as given (in then out, 6) and
  // its roll; its mode.
  var _count = 0;
  var _positions = Float64List(0);
  var _given = Float64List(0);
  var _rolls = Float64List(0);
  final _modes = <TangentMode>[];
  bool _closed;

  // Changed since the tables were made; how many times it has changed.
  var _dirty = true;
  var _version = 0;

  // Each knot's handles as worked out (in then out, from the knot).
  var _handles = Float64List(0);

  // The table: [_steps] entries a piece of curve (and one at the end),
  // each its distance along, position, direction and up.
  static const _steps = 32;
  var _curves = 0;
  var _distances = Float64List(0);
  var _points = Float64List(0);
  var _directions = Float64List(0);
  var _ups = Float64List(0);
  var _length = 0.0;

  // Where a lookup is: the piece of curve, how far along it (0 to 1), the
  // table entry before it and how far to the next.
  var _curve = 0;
  var _t = 0.0;
  var _entry = 0;
  var _f = 0.0;
  // What an evaluation gives: a position, the way it goes (not unit
  // length), up.
  var _px = 0.0, _py = 0.0, _pz = 0.0;
  var _dx = 0.0, _dy = 0.0, _dz = 0.0;
  var _ux = 0.0, _uy = 0.0, _uz = 0.0;

  // The samples to sweep along, ten floats each, and how many.
  var _path = Float32List(0);
  var _pathCount = 0;
  var _pieces = Int32List(0);
  // A setting for [_sample], as a field: the JIT boxes a double passed to
  // a call it doesn't inline.
  var _perMetre = 0.0;

  // --- Its knots ---

  /// How many knots it has.
  int get knotCount => _count;

  /// Its knots, a new list each time.
  List<Knot> get knots => [for (var i = 0; i < _count; i++) this[i]];

  /// Knot [index].
  Knot operator [](int index) {
    RangeError.checkValidIndex(index, this, 'index', _count);
    final (p, g) = (index * 3, index * 6);
    return Knot(
      Vec3(_positions[p], _positions[p + 1], _positions[p + 2]),
      mode: _modes[index],
      tangentIn: Vec3(_given[g], _given[g + 1], _given[g + 2]),
      tangentOut: Vec3(_given[g + 3], _given[g + 4], _given[g + 5]),
      roll: _rolls[index],
    );
  }

  /// Puts [knot] in place of knot [index].
  void operator []=(int index, Knot knot) {
    RangeError.checkValidIndex(index, this, 'index', _count);
    _write(index, knot);
    _changed();
  }

  /// Adds [knot] at the end.
  void add(Knot knot) => insert(_count, knot);

  /// Puts [knot] before knot [index] ([knotCount] to add it at the end).
  void insert(int index, Knot knot) {
    RangeError.checkValueInInterval(index, 0, _count, 'index');
    _grow(_count + 1);
    _positions.setRange((index + 1) * 3, (_count + 1) * 3, _positions, index * 3);
    _given.setRange((index + 1) * 6, (_count + 1) * 6, _given, index * 6);
    _rolls.setRange(index + 1, _count + 1, _rolls, index);
    _modes.insert(index, knot.mode);
    _count++;
    _write(index, knot);
    _changed();
  }

  /// Takes knot [index] away, and returns it.
  Knot removeAt(int index) {
    final knot = this[index];
    _positions.setRange(index * 3, (_count - 1) * 3, _positions, (index + 1) * 3);
    _given.setRange(index * 6, (_count - 1) * 6, _given, (index + 1) * 6);
    _rolls.setRange(index, _count - 1, _rolls, index + 1);
    _modes.removeAt(index);
    _count--;
    _changed();
    return knot;
  }

  /// Takes every knot away.
  void clear() {
    _count = 0;
    _modes.clear();
    _changed();
  }

  /// Moves every knot, keeping its mode, handles and roll: [positions] is
  /// 3 floats a knot (x, y, z), as many as it has. For a spline that moves
  /// each frame, such as a rope: filling the same list each time makes no
  /// garbage. Throws an [ArgumentError] for the wrong length.
  void setPositions(Float32List positions) {
    if (positions.length != _count * 3) {
      throw ArgumentError.value(positions.length, 'positions.length', 'must be 3 per knot: ${_count * 3}');
    }
    for (var i = 0; i < positions.length; i++) {
      _positions[i] = positions[i];
    }
    _changed();
  }

  /// Whether the last knot joins back to the first.
  bool get closed => _closed;
  set closed(bool value) {
    if (value == _closed) return;
    _closed = value;
    _changed();
  }

  /// A number that changes each time the spline does: a component that
  /// follows it compares it with the one it saw.
  int get version => _version;

  void _write(int index, Knot knot) {
    final (p, g) = (index * 3, index * 6);
    final (a, b, c) = (knot.position, knot.tangentIn, knot.tangentOut);
    for (final (what, v) in [('position', a), ('tangentIn', b), ('tangentOut', c)]) {
      if (!(v.x.isFinite && v.y.isFinite && v.z.isFinite)) throw ArgumentError.value(v, what, 'must be finite');
    }
    if (!knot.roll.isFinite) throw ArgumentError.value(knot.roll, 'roll', 'must be finite');
    _positions
      ..[p] = a.x
      ..[p + 1] = a.y
      ..[p + 2] = a.z;
    _given
      ..[g] = b.x
      ..[g + 1] = b.y
      ..[g + 2] = b.z
      ..[g + 3] = c.x
      ..[g + 4] = c.y
      ..[g + 5] = c.z;
    _rolls[index] = knot.roll;
    _modes[index] = knot.mode;
  }

  void _grow(int count) {
    if (_rolls.length >= count) return;
    final size = math.max(count, _rolls.length * 2);
    Float64List more(Float64List old, int per) => Float64List(size * per)..setAll(0, old);
    _positions = more(_positions, 3);
    _given = more(_given, 6);
    _rolls = more(_rolls, 1);
    _handles = Float64List(size * 6);
  }

  void _changed() {
    _dirty = true;
    _version++;
  }

  // --- Along it ---

  /// How long it is, in metres.
  double get length {
    _update();
    return _length;
  }

  /// The point [distance] metres along it from its first knot: clamped to
  /// its ends, or round again for a [closed] one.
  Vec3 positionAt(double distance) {
    _at(distance);
    _evaluate();
    return Vec3(_px, _py, _pz);
  }

  /// The way it goes [distance] metres along, unit length (Unity's
  /// `EvaluateTangent`, normalised).
  Vec3 directionAt(double distance) {
    _at(distance);
    _evaluate();
    _direction();
    return Vec3(_dx, _dy, _dz);
  }

  /// Which way is up [distance] metres along: at right angles to
  /// [directionAt], turned as little as it can be from the start, and by
  /// the knots' rolls.
  Vec3 upAt(double distance) {
    _at(distance);
    _evaluate();
    _direction();
    _up();
    return Vec3(_ux, _uy, _uz);
  }

  /// The rotation that faces along it [distance] metres along, up as
  /// [upAt]: for an object riding it, its -Z forward along the spline.
  Quat rotationAt(double distance) {
    _at(distance);
    _evaluate();
    _direction();
    _up();
    return Quat.lookRotation(Vec3(_dx, _dy, _dz), up: Vec3(_ux, _uy, _uz));
  }

  /// How far along it knot [index] is, in metres.
  double distanceOfKnot(int index) {
    RangeError.checkValidIndex(index, this, 'index', _count);
    _update();
    if (_curves == 0) return 0;
    return _distances[index * _steps];
  }

  /// The point on it nearest [point] (Unity's `GetNearestPoint`): how far
  /// along it is, and where.
  ({double distance, Vec3 position}) nearest(Vec3 point) {
    _update();
    if (_curves == 0) {
      return (distance: 0, position: _count == 0 ? Vec3.zero : Vec3(_positions[0], _positions[1], _positions[2]));
    }
    // The nearest point of the table's lines, then closer along the curve.
    var best = double.infinity;
    var at = 0.0;
    for (var e = 0; e < _curves * _steps; e++) {
      final (a, b) = (e * 3, e * 3 + 3);
      final (ax, ay, az) = (_points[a], _points[a + 1], _points[a + 2]);
      final (bx, by, bz) = (_points[b] - ax, _points[b + 1] - ay, _points[b + 2] - az);
      final (qx, qy, qz) = (point.x - ax, point.y - ay, point.z - az);
      final span = bx * bx + by * by + bz * bz;
      final s = span == 0 ? 0.0 : ((qx * bx + qy * by + qz * bz) / span).clamp(0.0, 1.0);
      final (rx, ry, rz) = (qx - bx * s, qy - by * s, qz - bz * s);
      final d = rx * rx + ry * ry + rz * rz;
      if (d < best) {
        best = d;
        at = _distances[e] + (_distances[e + 1] - _distances[e]) * s;
      }
    }
    // Golden-section search either side, within a table step.
    final step = _length / (_curves * _steps);
    var (lo, hi) = (at - step, at + step);
    if (!_closed) (lo, hi) = (math.max(lo, 0.0), math.min(hi, _length));
    double far(double d) {
      _at(d);
      _evaluate();
      final (x, y, z) = (_px - point.x, _py - point.y, _pz - point.z);
      return x * x + y * y + z * z;
    }

    const g = 0.6180339887498949;
    for (var i = 0; i < 24; i++) {
      final (a, b) = (hi - (hi - lo) * g, lo + (hi - lo) * g);
      if (far(a) < far(b)) {
        hi = b;
      } else {
        lo = a;
      }
    }
    var distance = (lo + hi) / 2;
    if (_closed) distance %= _length;
    return (distance: distance, position: positionAt(distance));
  }

  // --- Working it out ---
  //
  // What runs each time a spline that moves is swept takes no records and
  // passes no doubles to calls: the JIT boxes both unless it inlines.

  /// Makes the handles and the table again, if it changed.
  void _update() {
    if (!_dirty) return;
    _dirty = false;
    _curves = _count < 2 ? 0 : (_closed ? _count : _count - 1);
    _length = 0;
    if (_curves == 0) return;
    _handlesFor();
    final entries = _curves * _steps + 1;
    if (_distances.length < entries) {
      final size = math.max(entries, _distances.length * 2);
      _distances = Float64List(size);
      _points = Float64List(size * 3);
      _directions = Float64List(size * 3);
      _ups = Float64List(size * 3);
    }
    // Positions and directions.
    for (var e = 0; e < entries; e++) {
      _curve = e == entries - 1 ? _curves - 1 : e ~/ _steps;
      _t = e == entries - 1 ? 1.0 : (e % _steps) / _steps;
      _evaluate();
      final i = e * 3;
      _points[i] = _px;
      _points[i + 1] = _py;
      _points[i + 2] = _pz;
      _directions[i] = _dx;
      _directions[i + 1] = _dy;
      _directions[i + 2] = _dz;
    }
    // Unit length; where the curve stands still (a handle on its knot),
    // the way the table goes there.
    for (var e = 0; e < entries; e++) {
      final i = e * 3;
      var x = _directions[i], y = _directions[i + 1], z = _directions[i + 2];
      var l = math.sqrt(x * x + y * y + z * z);
      if (l < 1e-9) {
        final a = (e > 0 ? e - 1 : 0) * 3, b = (e < entries - 1 ? e + 1 : e) * 3;
        x = _points[b] - _points[a];
        y = _points[b + 1] - _points[a + 1];
        z = _points[b + 2] - _points[a + 2];
        l = math.sqrt(x * x + y * y + z * z);
      }
      if (l == 0) {
        x = 0;
        y = 0;
        z = -1;
        l = 1;
      }
      _directions[i] = x / l;
      _directions[i + 1] = y / l;
      _directions[i + 2] = z / l;
    }
    // Distances along.
    _distances[0] = 0;
    for (var e = 1; e < entries; e++) {
      final i = e * 3;
      final x = _points[i] - _points[i - 3], y = _points[i + 1] - _points[i - 2], z = _points[i + 2] - _points[i - 1];
      _distances[e] = _distances[e - 1] + math.sqrt(x * x + y * y + z * z);
    }
    _length = _distances[entries - 1];
    _rotationMinimising(entries);
  }

  /// Each knot's handles, from its mode and its neighbours.
  void _handlesFor() {
    final n = _count;
    for (var i = 0; i < n; i++) {
      final h = i * 6;
      final mode = _modes[i];
      if (mode == TangentMode.bezier) {
        for (var k = 0; k < 6; k++) {
          _handles[h + k] = _given[h + k];
        }
        continue;
      }
      final hasPrev = _closed || i > 0;
      final hasNext = _closed || i < n - 1;
      final p = (i - 1 + n) % n * 3, q = i * 3, r = (i + 1) % n * 3;
      // Straight towards each neighbour, a third of the way: linear.
      var back = 0.0, on = 0.0;
      for (var k = 0; k < 3; k++) {
        final here = _positions[q + k];
        final b = hasPrev ? _positions[p + k] - here : 0.0;
        final o = hasNext ? _positions[r + k] - here : 0.0;
        _handles[h + k] = b / 3;
        _handles[h + 3 + k] = o / 3;
        back += b * b;
        on += o * o;
      }
      if (mode == TangentMode.auto && hasPrev && hasNext) {
        // Catmull-Rom, weighted: each handle along the line from the knot
        // before to the one after, as long as its own side is.
        final d1 = math.sqrt(back), d2 = math.sqrt(on);
        if (d1 + d2 == 0) continue;
        for (var k = 0; k < 3; k++) {
          final across = _positions[r + k] - _positions[p + k];
          _handles[h + k] = -across * d1 / (d1 + d2) / 3;
          _handles[h + 3 + k] = across * d2 / (d1 + d2) / 3;
        }
      }
    }
  }

  /// The table's ups, each turned as little as it can be from the one
  /// before: double reflection (Wang et al. 2008). A closed spline's are
  /// turned a little more along it to meet its start again.
  void _rotationMinimising(int entries) {
    // Start with +Y (or -Z for a spline that starts straight up), at
    // right angles to the way it starts.
    var tx = _directions[0], ty = _directions[1], tz = _directions[2];
    var rx = 0.0, ry = 1.0, rz = 0.0;
    if (ty > 0.999 || ty < -0.999) {
      ry = 0;
      rz = -1;
    }
    var o = rx * tx + ry * ty + rz * tz;
    rx -= tx * o;
    ry -= ty * o;
    rz -= tz * o;
    var l = math.sqrt(rx * rx + ry * ry + rz * rz);
    rx /= l;
    ry /= l;
    rz /= l;
    _ups[0] = rx;
    _ups[1] = ry;
    _ups[2] = rz;
    for (var e = 0; e < entries - 1; e++) {
      final a = e * 3, b = a + 3;
      final vx = _points[b] - _points[a], vy = _points[b + 1] - _points[a + 1], vz = _points[b + 2] - _points[a + 2];
      final nx = _directions[b], ny = _directions[b + 1], nz = _directions[b + 2];
      final c1 = vx * vx + vy * vy + vz * vz;
      if (c1 > 1e-18) {
        // Reflect the up and the direction in the plane between the two
        // points, then in the one between that direction and the next.
        final dr = 2 / c1 * (vx * rx + vy * ry + vz * rz), dt = 2 / c1 * (vx * tx + vy * ty + vz * tz);
        final lx = rx - dr * vx, ly = ry - dr * vy, lz = rz - dr * vz;
        final wx = nx - (tx - dt * vx), wy = ny - (ty - dt * vy), wz = nz - (tz - dt * vz);
        final c2 = wx * wx + wy * wy + wz * wz;
        final d = c2 > 1e-18 ? 2 / c2 * (wx * lx + wy * ly + wz * lz) : 0.0;
        rx = lx - d * wx;
        ry = ly - d * wy;
        rz = lz - d * wz;
      }
      // Kept at right angles to the next direction, and unit length.
      o = rx * nx + ry * ny + rz * nz;
      rx -= nx * o;
      ry -= ny * o;
      rz -= nz * o;
      l = math.sqrt(rx * rx + ry * ry + rz * rz);
      if (l > 0) {
        rx /= l;
        ry /= l;
        rz /= l;
      }
      tx = nx;
      ty = ny;
      tz = nz;
      _ups[b] = rx;
      _ups[b + 1] = ry;
      _ups[b + 2] = rz;
    }
    if (!_closed || _length == 0) return;
    // How far the end's up is turned from the start's, around the start's
    // direction: taken out a little at a time along the way.
    final sx = _ups[0], sy = _ups[1], sz = _ups[2];
    final fx = _directions[0], fy = _directions[1], fz = _directions[2];
    final last = (entries - 1) * 3;
    final ex = _ups[last], ey = _ups[last + 1], ez = _ups[last + 2];
    final gap = math.atan2(
      fx * (ey * sz - ez * sy) + fy * (ez * sx - ex * sz) + fz * (ex * sy - ey * sx),
      ex * sx + ey * sy + ez * sz,
    );
    if (gap == 0) return;
    for (var e = 1; e < entries; e++) {
      final i = e * 3;
      final angle = gap * _distances[e] / _length;
      final kx = _directions[i], ky = _directions[i + 1], kz = _directions[i + 2];
      final vx = _ups[i], vy = _ups[i + 1], vz = _ups[i + 2];
      final c = math.cos(angle), s = math.sin(angle);
      // Rodrigues: v cos + (k × v) sin, v being at right angles to k.
      _ups[i] = vx * c + (ky * vz - kz * vy) * s;
      _ups[i + 1] = vy * c + (kz * vx - kx * vz) * s;
      _ups[i + 2] = vz * c + (kx * vy - ky * vx) * s;
    }
  }

  /// Finds [distance] along it: [_curve], [_t], [_entry] and [_f].
  void _at(double distance) {
    _update();
    if (_curves == 0) {
      if (_count == 0) throw StateError('a spline with no knots has no points');
      _curve = -1;
      return;
    }
    var d = distance;
    if (_closed && _length > 0) {
      d %= _length;
    } else {
      d = d < 0 ? 0 : (d > _length ? _length : d);
    }
    _d = d;
    _find(0, _curves * _steps);
  }

  // The distance [_find] looks for.
  var _d = 0.0;

  /// Finds [_d] between table entries [lo] and [hi]: for a sample, the
  /// piece it's on, so a knot's own distance stays on the piece before it.
  void _find(int lo, int hi) {
    final d = _d;
    while (hi - lo > 1) {
      final mid = (lo + hi) >> 1;
      if (_distances[mid] <= d) {
        lo = mid;
      } else {
        hi = mid;
      }
    }
    final span = _distances[lo + 1] - _distances[lo];
    var f = span > 0 ? (d - _distances[lo]) / span : 0.0;
    f = f < 0 ? 0.0 : (f > 1 ? 1.0 : f);
    _entry = lo;
    _f = f;
    _curve = lo ~/ _steps;
    if (_curve >= _curves) _curve = _curves - 1;
    _t = (lo - _curve * _steps + f) / _steps;
  }

  /// The position and direction at [_t] on [_curve], into [_px] and
  /// [_dx] and the rest; with one knot, the knot.
  void _evaluate() {
    if (_curve < 0) {
      _px = _positions[0];
      _py = _positions[1];
      _pz = _positions[2];
      _dx = 0;
      _dy = 0;
      _dz = -1;
      return;
    }
    final a = _curve * 3, b = (_curve + 1) % _count * 3;
    final t = _t, u = 1 - t;
    final w0 = u * u * u, w1 = 3 * u * u * t, w2 = 3 * u * t * t, w3 = t * t * t;
    final v0 = 3 * u * u, v1 = 6 * u * t, v2 = 3 * t * t;
    for (var k = 0; k < 3; k++) {
      final p0 = _positions[a + k];
      final p3 = _positions[b + k];
      final p1 = p0 + _handles[a * 2 + 3 + k];
      final p2 = p3 + _handles[b * 2 + k];
      _out[k] = w0 * p0 + w1 * p1 + w2 * p2 + w3 * p3;
      _out[k + 3] = v0 * (p1 - p0) + v1 * (p2 - p1) + v2 * (p3 - p2);
    }
    _px = _out[0];
    _py = _out[1];
    _pz = _out[2];
    _dx = _out[3];
    _dy = _out[4];
    _dz = _out[5];
  }

  final _out = Float64List(6);

  /// [_dx] and the rest made unit length; where the curve stands still
  /// (a handle on its knot), the table's direction there.
  void _direction() {
    final l = math.sqrt(_dx * _dx + _dy * _dy + _dz * _dz);
    if (l < 1e-9) {
      if (_curve < 0) return;
      final i = (_f < 0.5 ? _entry : _entry + 1) * 3;
      _dx = _directions[i];
      _dy = _directions[i + 1];
      _dz = _directions[i + 2];
      return;
    }
    _dx /= l;
    _dy /= l;
    _dz /= l;
  }

  /// Up where [_at] put it, at right angles to the unit direction, rolled:
  /// into [_ux] and the rest.
  void _up() {
    if (_curve < 0) {
      _ux = 0;
      _uy = 1;
      _uz = 0;
      return;
    }
    final i = _entry * 3, j = i + 3;
    final f = _f;
    var x = _ups[i] + (_ups[j] - _ups[i]) * f;
    var y = _ups[i + 1] + (_ups[j + 1] - _ups[i + 1]) * f;
    var z = _ups[i + 2] + (_ups[j + 2] - _ups[i + 2]) * f;
    final o = x * _dx + y * _dy + z * _dz;
    x -= _dx * o;
    y -= _dy * o;
    z -= _dz * o;
    final l = math.sqrt(x * x + y * y + z * z);
    if (l > 1e-9) {
      x /= l;
      y /= l;
      z /= l;
    } else {
      x = _ups[i];
      y = _ups[i + 1];
      z = _ups[i + 2];
    }
    final a = _curve, b = (a + 1) % _count;
    final roll = _rolls[a] + (_rolls[b] - _rolls[a]) * _t;
    if (roll != 0) {
      // Clockwise seen going along it: around the direction, right-handed.
      final c = math.cos(roll), s = math.sin(roll);
      final rx = x * c + (_dy * z - _dz * y) * s;
      final ry = y * c + (_dz * x - _dx * z) * s;
      final rz = z * c + (_dx * y - _dy * x) * s;
      x = rx;
      y = ry;
      z = rz;
    }
    _ux = x;
    _uy = y;
    _uz = z;
  }

  // --- Samples to sweep along ---

  /// Fills [_path] with points along it to sweep along: each piece of
  /// curve cut into [_perMetre] a metre, and more where it bends (one
  /// every 8°), at most [most] in all (but one a piece). At a corner, the
  /// way the path goes is halfway between the ways either side.
  void _sample(int most) {
    _update();
    if (_curves == 0 || _length == 0) {
      throw ArgumentError('a spline needs at least two knots apart to sweep along; this has $_count');
    }
    if (_pieces.length < _curves) _pieces = Int32List(math.max(_curves, _pieces.length * 2));
    const bendEach = 8 * math.pi / 180;
    var total = 0;
    for (var c = 0; c < _curves; c++) {
      final from = c * _steps, to = from + _steps;
      var bend = 0.0;
      for (var e = from; e < to; e++) {
        final a = e * 3, b = a + 3;
        var dot = _directions[a] * _directions[b] + _directions[a + 1] * _directions[b + 1];
        dot += _directions[a + 2] * _directions[b + 2];
        bend += math.acos(dot < -1 ? -1.0 : (dot > 1 ? 1.0 : dot));
      }
      final byLength = ((_distances[to] - _distances[from]) * _perMetre).ceil();
      final byBend = (bend / bendEach).ceil();
      final n = byLength > byBend ? byLength : byBend;
      _pieces[c] = n < 1 ? 1 : n;
      total += _pieces[c];
    }
    if (total > most) {
      // Each piece its share of [most], rounded as they go so the shares
      // add up to it.
      var share = 0.0;
      var given = 0;
      for (var c = 0; c < _curves; c++) {
        share += _pieces[c] * most / total;
        final n = share.round() - given;
        _pieces[c] = n < 1 ? 1 : n;
        given += _pieces[c];
      }
      total = given;
    }
    _pathCount = total + 1;
    if (_path.length < _pathCount * 10) _path = Float32List(math.max(_pathCount * 10, _path.length * 2));
    var at = 0;
    for (var c = 0; c < _curves; c++) {
      final start = _distances[c * _steps], end = _distances[(c + 1) * _steps];
      final n = _pieces[c];
      for (var k = c == 0 ? 0 : 1; k <= n; k++) {
        _d = start + (end - start) * k / n;
        _find(c * _steps, (c + 1) * _steps);
        _evaluate();
        _direction();
        // At a knot between two pieces, halfway between their ways. (A
        // table entry on a knot has the way the piece after it starts;
        // the last has the way the last piece ends.)
        if (k == n && (c < _curves - 1 || _closed)) {
          _mitre(c + 1 == _curves ? 0 : (c + 1) * _steps);
        } else if (k == 0 && _closed) {
          _mitre(_curves * _steps);
        }
        _up();
        _path[at] = _px;
        _path[at + 1] = _py;
        _path[at + 2] = _pz;
        _path[at + 3] = _dx;
        _path[at + 4] = _dy;
        _path[at + 5] = _dz;
        _path[at + 6] = _ux;
        _path[at + 7] = _uy;
        _path[at + 8] = _uz;
        _path[at + 9] = _d;
        at += 10;
      }
    }
    if (_closed) {
      // The end where the start is, exactly.
      final last = (_pathCount - 1) * 10;
      for (var k = 0; k < 9; k++) {
        _path[last + k] = _path[k];
      }
    }
  }

  /// The unit direction made halfway between it and table [entry]'s,
  /// where they differ: a corner's.
  void _mitre(int entry) {
    final i = entry * 3;
    final x = _directions[i], y = _directions[i + 1], z = _directions[i + 2];
    if (_dx * x + _dy * y + _dz * z > 0.9998) return;
    final mx = _dx + x, my = _dy + y, mz = _dz + z;
    final l = math.sqrt(mx * mx + my * my + mz * mz);
    if (l == 0) return;
    _dx = mx / l;
    _dy = my / l;
    _dz = mz / l;
  }

  @override
  String toString() => 'Spline($_count knots${_closed ? ', closed' : ''})';
}
