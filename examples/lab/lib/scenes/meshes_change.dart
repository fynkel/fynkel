// Stage 13 phase 1: meshes that change. A flag waving, in two colours
// (two submeshes, MeshRenderer.materials); water rippling where it's
// tapped, 10,201 vertices changed every frame and coloured by height; and
// ground dug where it's tapped, its MeshCollider following, so balls
// dropped on it roll into the holes.

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

class MeshesChangeScene extends LabScene {
  /// The whole page; with [still], nothing moves on its own (for its
  /// golden): the flag and the water hold their first shape.
  MeshesChangeScene({this.still = false});

  /// Whether the flag and the water stay as they're made.
  final bool still;

  @override
  String get title => 'Meshes that change';
  @override
  String get step => 'stage 13 phase 1';
  @override
  String get about =>
      'FLAG: a Mesh() whose positions are set each frame, its normals worked out by the engine; its top and bottom '
      'halves are two submeshes (setTriangles with submeshStarts), each drawn with its own material '
      '(MeshRenderer.materials). '
      'WATER: 101 × 101 = 10,201 vertices, markDynamic, its heights and vertex colours (setColors) changed every '
      'frame; tap it to drop a ripple. '
      'GROUND: tap it to dig, the dirt showing by height; its MeshCollider follows the mesh by itself, so the '
      'balls ("drop balls") roll into the holes. '
      'The readout gives what changing them and sending them takes each frame (world.meshChangeTime).';

  @override
  Vec3 get eye => Vec3(0, 3.6, 5.4);
  @override
  Vec3 get target => Vec3(0, 0.6, 0);

  late final Flag flag;
  late final Water water;
  late final Ground ground;
  final _balls = <GameObject>[];

  // Over the last two seconds: the game working the shapes out, setting
  // them on the meshes, and the meshes sent; and the medians they came to.
  final _work = <int>[], _setting = <int>[], _sending = <int>[];
  var _shown = (work: 0, setting: 0, sending: 0);
  var _fps = '–';

  @override
  void build() {
    if (_startQuality.isNotEmpty) world.quality = Quality.levels.firstWhere((q) => q.name == _startQuality);
    flag = add(Flag(still: still)..transform = Transform(position: Vec3(-3.4, 0, -0.6)));
    add(
      shape('pole', Mesh.cylinder(), const Color(0xFFB8BCC4), at: Vec3(-3.4, 1.1, -0.6), size: Vec3(0.06, 2.2, 0.06)),
    );
    water = add(Water(still: still)..transform = Transform(position: Vec3(0, 0.3, 0)));
    // The basin under it.
    add(shape('basin', Mesh.cube(), const Color(0xFF1B3A4A), at: Vec3(0, 0.125, 0), size: Vec3(3.2, 0.25, 3.2)));
    ground = add(Ground()..transform = Transform(position: Vec3(3.4, 0, 0)));
  }

  @override
  void onStart() {
    super.onStart();
    final camera = find('camera')!.get<Camera>()!;
    on(input.onTap, (finger) {
      final ray = camera.screenPointToRay(finger.position);
      final hit = world.physics.raycast(ray);
      if (hit != null && hit.collider.gameObject == ground) {
        ground.dig(hit.point);
        log.add('dug at ${hit.point.x.toStringAsFixed(1)}, ${hit.point.z.toStringAsFixed(1)}');
        return;
      }
      // The water has no collider (it changes every frame): where the ray
      // meets its level.
      if (ray.direction.y < 0) {
        final t = (water.position.y - ray.origin.y) / ray.direction.y;
        final at = ray.origin + ray.direction * t;
        if (water.ripple(at)) log.add('a ripple');
      }
    });
  }

  void dropBalls() {
    final random = math.Random(_balls.length);
    for (var i = 0; i < 6; i++) {
      _balls.add(
        add(
          shape(
            'ball',
            Mesh.sphere(),
            Color.rgb(
              0.5 + random.nextDouble() * 0.5,
              0.3 + random.nextDouble() * 0.5,
              0.3 + random.nextDouble() * 0.6,
            ),
            at: ground.position + Vec3(random.nextDouble() * 2.4 - 1.2, 2 + i * 0.3, random.nextDouble() * 2.4 - 1.2),
            size: Vec3(0.24, 0.24, 0.24),
            components: [SphereCollider(radius: 0.5), Rigidbody(mass: 0.2)],
          ),
        ),
      );
    }
    // Those that rolled off, gone.
    for (final ball in [..._balls]) {
      if (ball.position.y < -2) {
        ball.destroy();
        _balls.remove(ball);
      }
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // The frame before's sending, as this frame's hasn't happened yet.
    _work.add(flag.work + water.work);
    _setting.add(flag.setting + water.setting);
    _sending.add(world.meshChangeTime.inMicroseconds);
    if (_work.length == 120) {
      int median(List<int> times) => (times..sort())[60];
      _shown = (work: median(_work), setting: median(_setting), sending: median(_sending));
      for (final times in [_work, _setting, _sending]) {
        times.clear();
      }
      if (printRates) {
        // ignore: avoid_print
        print('[meshes] the game ${_ms(_shown.work)}, setting ${_ms(_shown.setting)}, sending ${_ms(_shown.sending)}');
      }
    }
    readout.value = [
      'flag ${flag.mesh.vertexCount} vertices, water ${water.mesh.vertexCount}, ground ${ground.mesh.vertexCount} '
          '(${ground.mesh.indexCount ~/ 3} triangles, its collider made again ${ground.digs} times)',
      'the flag and the water a frame (medians): the game working them out ${_ms(_shown.work)}, setting them '
          '${_ms(_shown.setting)}, sending them ${_ms(_shown.sending)}   $_fps fps',
    ].join('\n');
  }

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('meshes', s));

  String _ms(int micros) => '${(micros / 1000).toStringAsFixed(2)} ms';

  @override
  List<Control> get controls => [
    Toggle('flag waving', () => flag.waving, (on) => flag.waving = on),
    Toggle('water moving', () => water.moving, (on) => water.moving = on),
    Button('drop balls', dropBalls),
    Button('fill the ground in', () {
      ground.fill();
      log.add('filled in');
    }),
  ];
}

/// A grid of [columns] × [rows] squares, the first vertex row by row:
/// two triangles a square, facing up (+y) when x goes right and z towards
/// the viewer. The indices go in [into] from [at].
void gridTriangles(Uint32List into, int columns, int rows, {int at = 0, int first = 0}) {
  final across = columns + 1;
  for (var z = 0; z < rows; z++) {
    for (var x = 0; x < columns; x++) {
      final a = first + z * across + x;
      into
        ..[at++] = a
        ..[at++] = a + across
        ..[at++] = a + 1
        ..[at++] = a + 1
        ..[at++] = a + across
        ..[at++] = a + across + 1;
    }
  }
}

/// A flag 2 m by 1.2 m on its pole, waving: red above, white below, as two
/// submeshes, each with its material. Seen from both sides.
final class Flag extends GameObject {
  Flag({this.still = false}) : super(name: 'flag');

  final bool still;
  static const columns = 32, rows = 20, width = 2.0, height = 1.2;

  final mesh = Mesh();
  final _positions = Float32List((columns + 1) * (rows + 1) * 3);
  var waving = true;

  /// What the last frame's change took, in microseconds: working the
  /// new shape out, and giving it to the mesh.
  var work = 0, setting = 0;

  final _watch = Stopwatch();

  @override
  void onLoad() {
    super.onLoad();
    final uvs = Float32List((columns + 1) * (rows + 1) * 2);
    for (var r = 0; r <= rows; r++) {
      for (var c = 0; c <= columns; c++) {
        final i = r * (columns + 1) + c;
        uvs[i * 2] = c / columns;
        uvs[i * 2 + 1] = r / rows;
      }
    }
    // Rows from the top down, so the first half of the squares is the top.
    final indices = Uint32List(columns * rows * 6);
    gridTriangles(indices, columns, rows);
    _shape(0);
    mesh
      ..setPositions(_positions)
      ..setUvs(uvs)
      ..setTriangles(indices, submeshStarts: Uint32List.fromList([columns * rows * 3]))
      ..markDynamic();
    addComponent(
      MeshRenderer(mesh)
        ..materials = [
          Material(color: const Color(0xFFD8343C), roughness: 0.8, doubleSided: true),
          Material(color: const Color(0xFFF2EEE6), roughness: 0.8, doubleSided: true),
        ],
    );
  }

  /// The cloth at [t] seconds: a wave running out from the pole, bigger
  /// towards the free end.
  void _shape(double t) {
    var i = 0;
    for (var r = 0; r <= rows; r++) {
      final y = 2.15 - r / rows * height;
      for (var c = 0; c <= columns; c++) {
        final u = c / columns;
        final x = u * width;
        final wave = math.sin(x * 2.6 - t * 5 + r * 0.08) * 0.16 * u + math.sin(x * 5.1 - t * 7.3) * 0.04 * u;
        _positions
          ..[i++] = x
          ..[i++] = y - u * u * 0.08
          ..[i++] = wave;
      }
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (still || !waving) {
      work = setting = 0;
      return;
    }
    _watch
      ..reset()
      ..start();
    _shape(time.sinceStart);
    work = _watch.elapsedMicroseconds;
    mesh.setPositions(_positions);
    setting = _watch.elapsedMicroseconds - work;
  }
}

/// Water 3 m across, 101 × 101 vertices, rippling: a height field stepped
/// each frame, its heights and colours given to the mesh each frame.
final class Water extends GameObject {
  Water({this.still = false}) : super(name: 'water');

  final bool still;
  static const squares = 100, size = 3.0;
  static const across = squares + 1;

  final mesh = Mesh();
  var moving = true;

  /// What the last frame's change took, in microseconds: working the
  /// new shape out, and giving it to the mesh.
  var work = 0, setting = 0;

  // Two height fields, now and before, and what goes to the mesh.
  var _now = Float32List(across * across);
  var _before = Float32List(across * across);
  final _positions = Float32List(across * across * 3);
  final _colors = Uint8List(across * across * 4);
  final _watch = Stopwatch();
  final _random = math.Random(3);
  var _sinceDrop = 0.0;

  @override
  void onLoad() {
    super.onLoad();
    for (var z = 0; z < across; z++) {
      for (var x = 0; x < across; x++) {
        final i = (z * across + x) * 3;
        _positions[i] = x / squares * size - size / 2;
        _positions[i + 2] = z / squares * size - size / 2;
      }
    }
    final indices = Uint32List(squares * squares * 6);
    gridTriangles(indices, squares, squares);
    // A few ripples to start with, the same each time.
    for (var i = 0; i < 4; i++) {
      _drop(10 + _random.nextInt(80), 10 + _random.nextInt(80), 0.12);
    }
    // Spread out a little before the first frame.
    for (var i = 0; i < 30; i++) {
      _step();
    }
    _write();
    mesh
      ..setPositions(_positions)
      ..setColors(_colors)
      ..setTriangles(indices)
      ..markDynamic();
    addComponent(MeshRenderer(mesh, Material(roughness: 0.15)));
  }

  /// A ripple where [at] (in the world) is, if it's on the water.
  bool ripple(Vec3 at) {
    final x = ((at.x - position.x + size / 2) / size * squares).round();
    final z = ((at.z - position.z + size / 2) / size * squares).round();
    if (x < 2 || z < 2 || x > squares - 2 || z > squares - 2) return false;
    _drop(x, z, 0.18);
    return true;
  }

  void _drop(int x, int z, double depth) {
    for (var dz = -2; dz <= 2; dz++) {
      for (var dx = -2; dx <= 2; dx++) {
        final d = math.sqrt(dx * dx + dz * dz);
        if (d <= 2.2) _now[(z + dz) * across + x + dx] -= depth * (1 - d / 2.5);
      }
    }
  }

  /// One step of the waves: each point pulled to its neighbours' mean,
  /// a little damped (the old two-buffer ripple).
  void _step() {
    final (now, before) = (_now, _before);
    for (var z = 1; z < squares; z++) {
      for (var x = 1; x < squares; x++) {
        final i = z * across + x;
        final mean = (now[i - 1] + now[i + 1] + now[i - across] + now[i + across]) * 0.5;
        before[i] = (mean - before[i]) * 0.985;
      }
    }
    _before = now;
    _now = before;
  }

  /// The heights into the positions, and colours by height: deep blue in
  /// the troughs, pale on the crests.
  void _write() {
    final (now, positions, colors) = (_now, _positions, _colors);
    for (var i = 0; i < across * across; i++) {
      final h = now[i];
      positions[i * 3 + 1] = h * 0.5;
      final light = (0.5 + h * 6).clamp(0.0, 1.0);
      colors
        ..[i * 4] = (40 + 170 * light).toInt()
        ..[i * 4 + 1] = (110 + 120 * light).toInt()
        ..[i * 4 + 2] = (170 + 80 * light).toInt()
        ..[i * 4 + 3] = 255;
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (still || !moving) {
      work = setting = 0;
      return;
    }
    _watch
      ..reset()
      ..start();
    // A drop now and then, so it never goes flat.
    _sinceDrop += time.delta;
    if (_sinceDrop > 1.2) {
      _sinceDrop = 0;
      _drop(10 + _random.nextInt(80), 10 + _random.nextInt(80), 0.1);
    }
    _step();
    _write();
    work = _watch.elapsedMicroseconds;
    mesh
      ..setPositions(_positions)
      ..setColors(_colors);
    setting = _watch.elapsedMicroseconds - work;
  }
}

/// Ground 3 m across that's dug where it's tapped: grass on top, dirt
/// where it's dug, by vertex colour; its sides a second submesh. Its
/// MeshCollider follows it.
final class Ground extends GameObject {
  Ground() : super(name: 'ground');

  static const squares = 48, size = 3.0, top = 0.6;
  static const across = squares + 1;
  static const _grid = across * across;
  // Each side's vertices: along its edge at the top and at the bottom.
  static const _sideVertices = 4 * across * 2;

  final mesh = Mesh();
  final _heights = Float32List(_grid);
  final _positions = Float32List((_grid + _sideVertices) * 3);
  final _colors = Uint8List((_grid + _sideVertices) * 4);

  /// How many times it was dug, and so its collider made again.
  var digs = 0;

  @override
  void onLoad() {
    super.onLoad();
    _heights.fillRange(0, _grid, top);
    final indices = Uint32List(squares * squares * 6 + 4 * squares * 6);
    gridTriangles(indices, squares, squares);
    // The sides: for each, a strip of squares from the top edge down.
    var at = squares * squares * 6;
    for (var side = 0; side < 4; side++) {
      final first = _grid + side * across * 2;
      for (var k = 0; k < squares; k++) {
        final (t0, t1, b0, b1) = (first + k, first + k + 1, first + across + k, first + across + k + 1);
        indices
          ..[at++] = t0
          ..[at++] = b0
          ..[at++] = t1
          ..[at++] = t1
          ..[at++] = b0
          ..[at++] = b1;
      }
    }
    _write();
    mesh
      ..setPositions(_positions)
      ..setColors(_colors)
      ..setTriangles(indices, submeshStarts: Uint32List.fromList([squares * squares * 6]));
    addComponent(
      MeshRenderer(mesh)
        ..materials = [Material(roughness: 0.9), Material(color: const Color(0xFF6B4A30), roughness: 1)],
    );
    addComponent(MeshCollider());
  }

  /// A crater where [at] (in the world) is, 0.45 m across and up to
  /// 0.3 m deep, never through the bottom, nor at the edges.
  void dig(Vec3 at) {
    final cx = (at.x - position.x + size / 2) / size * squares;
    final cz = (at.z - position.z + size / 2) / size * squares;
    const reach = 0.45 / size * squares;
    for (var z = 1; z < squares; z++) {
      for (var x = 1; x < squares; x++) {
        final d = math.sqrt((x - cx) * (x - cx) + (z - cz) * (z - cz)) / reach;
        if (d >= 1) continue;
        final i = z * across + x;
        final dug = 0.3 * (1 - d * d);
        _heights[i] = math.max(0.05, _heights[i] - dug * 0.6);
      }
    }
    digs++;
    _write();
    mesh
      ..setPositions(_positions)
      ..setColors(_colors);
  }

  /// Flat again.
  void fill() {
    _heights.fillRange(0, _grid, top);
    digs++;
    _write();
    mesh
      ..setPositions(_positions)
      ..setColors(_colors);
  }

  void _write() {
    for (var z = 0; z < across; z++) {
      for (var x = 0; x < across; x++) {
        final i = z * across + x;
        final h = _heights[i];
        _positions
          ..[i * 3] = x / squares * size - size / 2
          ..[i * 3 + 1] = h
          ..[i * 3 + 2] = z / squares * size - size / 2;
        // Grass on top, dirt as it's dug, darker deeper.
        final grass = ((h - (top - 0.06)) / 0.06).clamp(0.0, 1.0);
        final deep = ((top - h) / top).clamp(0.0, 1.0);
        final dirt = (r: 150 - 70 * deep, g: 105 - 50 * deep, b: 70 - 30 * deep);
        _colors
          ..[i * 4] = (dirt.r + (90 - dirt.r) * grass).toInt()
          ..[i * 4 + 1] = (dirt.g + (170 - dirt.g) * grass).toInt()
          ..[i * 4 + 2] = (dirt.b + (70 - dirt.b) * grass).toInt()
          ..[i * 4 + 3] = 255;
      }
    }
    // The sides, each looking out: its edge's points, then the same below.
    // Front (z = +size/2), right (x = +), back (z = -), left (x = -), each
    // ordered so its triangles face out.
    const half = size / 2;
    for (var side = 0; side < 4; side++) {
      final first = _grid + side * across * 2;
      for (var k = 0; k < across; k++) {
        final s = k / squares * size - half;
        final (x, z, edge) = switch (side) {
          0 => (s, half, squares * across + k),
          1 => (half, -s, (squares - k) * across + squares),
          2 => (-s, -half, squares - k),
          _ => (-half, s, k * across),
        };
        for (final (n, y) in [(first + k, _heights[edge]), (first + across + k, 0.0)]) {
          _positions
            ..[n * 3] = x
            ..[n * 3 + 1] = y
            ..[n * 3 + 2] = z;
          _colors.fillRange(n * 4, n * 4 + 4, 255);
        }
      }
    }
  }
}
