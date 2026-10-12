// The scale check of stage 6 (docs/stage-6.md, step 5): a world of many
// objects of many kinds, busy the way a game is. Groups of ten objects (a
// block and nine parts) stand in a square; each group is one of 50 kinds,
// a mesh, a material and one of five ways to behave:
//
// - still: nothing runs;
// - spin: the block turns, carrying its parts;
// - bob: each part moves up and down on its own;
// - beat: the parts listen to a signal the root sends every second, and
//   punch their scale when it comes;
// - blink: a routine hides one part and shows it again, now and then, as a
//   pool does.
//
// Its settings are `--dart-define`s: OBJECTS (10,000); CHURN, how many
// groups a second are destroyed and made anew (0), for the long runs; and
// ONLY, one behaviour for every kind.

import 'dart:math' as math;

import 'package:flutter/widgets.dart' show runApp;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() => runApp(FynkelApp(Scale.new));

const _objects = int.fromEnvironment('OBJECTS', defaultValue: 10000);
const _churn = int.fromEnvironment('CHURN', defaultValue: 0);

// ONLY=bob (or still, spin, beat, blink) gives every kind that behaviour,
// to time each alone.
const _only = String.fromEnvironment('ONLY');

// SHAPE=cube draws every kind as a cube, to see what the spheres cost; SHAPE=low
// draws the spheres with 720 triangles (as Unity's 768) in place of the
// built-in one's 2,208.
const _shape = String.fromEnvironment('SHAPE');

// AWAY=true turns the camera to look away: everything is still drawn (there is
// no culling), but no pixel is filled.
const _away = bool.fromEnvironment('AWAY');

/// How many kinds there are, and how a kind behaves.
const kinds = 50;

enum Behave { still, spin, bob, beat, blink }

/// A kind's look and behaviour, from its number.
class Kind {
  Kind(this.index)
    : behave = _only.isEmpty ? Behave.values[index % Behave.values.length] : Behave.values.byName(_only),
      mesh = index.isEven || _shape == 'cube'
          ? Mesh.cube()
          : _shape == 'low'
          ? _lowSphere
          : Mesh.sphere(),
      material = Material(color: _hue(index / kinds));

  final int index;
  final Behave behave;
  final Mesh mesh;
  final Material material;

  static Color _hue(double h) {
    double f(double n) {
      final k = (n + h * 6) % 6;
      return 1 - math.max(0, math.min(math.min(k, 4 - k), 1)).toDouble() * 0.75;
    }

    int c(double v) => (v * 255).round();
    return Color(0xFF000000 | c(f(5)) << 16 | c(f(3)) << 8 | c(f(1)));
  }
}

/// A sphere of radius 0.5 with 24 segments and 16 rings.
final Mesh _lowSphere = () {
  const segments = 24, rings = 16;
  final positions = <Vec3>[];
  for (var r = 0; r <= rings; r++) {
    final v = r / rings * math.pi;
    for (var s = 0; s <= segments; s++) {
      final u = s / segments * 2 * math.pi;
      positions.add(Vec3(math.sin(v) * math.cos(u), math.cos(v), math.sin(v) * math.sin(u)) * 0.5);
    }
  }
  final indices = <int>[];
  for (var r = 0; r < rings; r++) {
    for (var s = 0; s < segments; s++) {
      final a = r * (segments + 1) + s, b = a + segments + 1;
      if (r > 0) indices.addAll([a, a + 1, b]);
      if (r < rings - 1) indices.addAll([a + 1, b + 1, b]);
    }
  }
  return Mesh.custom(positions: positions, normals: [for (final p in positions) p.normalized], indices: indices);
}();

class Scale extends GameObject {
  Scale({this.objects = _objects, this.churn = _churn}) : super(name: 'scale');

  /// How many objects, in groups of ten.
  final int objects;

  /// How many groups a second are destroyed and made anew.
  final int churn;

  /// Sent every second; the beat kinds listen to it.
  final beat = Signal<int>();

  late final List<Kind> _kinds = [for (var i = 0; i < kinds; i++) Kind(i)];
  final _groups = <Group>[];
  late final int _side = math.sqrt(objects / 10).ceil();
  late final GameObject _pivot;
  var _beats = 0;
  var _beatClock = 0.0;
  var _churnDue = 0.0;
  var _next = 0; // the next group replaced
  var _made = 0; // groups made so far, which picks each one's kind

  /// How many groups were destroyed and made anew.
  int replaced = 0;

  int get groups => _groups.length;

  @override
  void onLoad() {
    super.onLoad();
    final reach = _side * 3.0;
    final eye = Vec3(0, reach * 0.55, reach * 0.75);
    _pivot = add(GameObject(name: 'pivot'));
    _pivot.add(
      GameObject(
        name: 'camera',
        transform: Transform(position: eye, rotation: Quat.lookRotation(_away ? eye : -eye)),
        components: [Camera()],
      ),
    );
    add(
      GameObject(
        name: 'sun',
        transform: Transform(rotation: Quat.lookRotation(Vec3(-0.4, -1, -0.3))),
        components: [Light.sun()],
      ),
    );
    add(GameObject(name: 'sky', components: [Light.ambient(intensity: 0.3)]));
    for (var i = 0; i < objects ~/ 10; i++) {
      _groups.add(add(_group(i)));
    }
  }

  Group _group(int slot) {
    final kind = _kinds[_made++ % kinds];
    final x = slot % _side, z = slot ~/ _side;
    final offset = (_side - 1) * 1.5;
    return Group(kind, slot, Vec3(x * 3.0 - offset, 0, z * 3.0 - offset));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _pivot.rotate(Vec3.up, time.delta * 0.1);
    _beatClock += time.delta;
    if (_beatClock >= 1) {
      _beatClock -= 1;
      beat.emit(++_beats);
    }
    if (churn > 0 && _groups.isNotEmpty) {
      _churnDue += churn * time.delta;
      while (_churnDue >= 1) {
        _churnDue -= 1;
        final slot = _next++ % _groups.length;
        _groups[slot].destroy();
        _groups[slot] = add(_group(slot));
        replaced++;
      }
    }
  }
}

/// A block and its nine parts.
class Group extends GameObject {
  Group(this.kind, this.slot, Vec3 position)
    : super(
        name: 'group',
        transform: Transform(position: position),
        components: [MeshRenderer(kind.mesh, kind.material)],
      );

  final Kind kind;
  final int slot;

  @override
  void onLoad() {
    super.onLoad();
    for (var i = 0; i < 9; i++) {
      final angle = i / 9 * 2 * math.pi;
      add(Part(kind, i, Transform(position: Vec3(math.cos(angle), 0.8, math.sin(angle)), scale: Vec3.one * 0.3)));
    }
    if (kind.behave == Behave.blink) startRoutine(_blink);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (kind.behave == Behave.spin) rotate(Vec3.up, time.delta);
  }

  Future<void> _blink() async {
    final random = math.Random(slot);
    final parts = children.whereType<Part>().toList();
    while (true) {
      await wait(1 + random.nextDouble() * 3);
      final part = parts[random.nextInt(parts.length)];
      part.active = false;
      await wait(0.5);
      part.active = true;
    }
  }
}

/// One of a group's nine parts.
class Part extends GameObject {
  Part(this.kind, this.index, Transform transform)
    : super(name: 'part', transform: transform, components: [MeshRenderer(kind.mesh, kind.material)]);

  final Kind kind;
  final int index;
  late final double _y = position.y;
  var _t = 0.0;

  @override
  void onStart() {
    super.onStart();
    if (kind.behave == Behave.beat) {
      on(ancestor<Scale>()!.beat, (_) => punchScale(Vec3.one * 0.1, seconds: 0.3));
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (kind.behave != Behave.bob) return;
    _t += time.delta;
    position = Vec3(position.x, _y + math.sin(_t * 2 + index) * 0.2, position.z);
  }
}
