// Stage 13 phase 3: splines. A road swept along a spline whose knots are
// dragged, rebuilt as they move; a rope that sags between two posts and
// swings, swept again each frame; a fence of posts placed along a curve
// (SplineInstantiate) with rails swept along it; and a cart riding a
// closed track (SplineAnimate).

import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter/widgets.dart' show BuildContext, Widget;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../lab.dart';

/// `--dart-define=LAB_QUALITY=low` (or medium, high) starts the scene at
/// that level, for measuring; the device's own pick otherwise.
const _startQuality = String.fromEnvironment('LAB_QUALITY');

/// A road's cross-section, 12 points, [width] across: a cambered top with
/// a kerb each side, and a flat underside.
Shape2D roadProfile(double width) {
  final h = width / 2;
  final k = width / 6 * 0.1;
  return Shape2D([
    Vec2(-h, -k * 2),
    Vec2(0, -k * 2),
    Vec2(h, -k * 2),
    Vec2(h, k * 1.5),
    Vec2(h - k * 2, k * 1.5),
    Vec2(h - k * 3, k * 0.5),
    Vec2(h / 3, k * 0.8),
    Vec2(0, k),
    Vec2(-h / 3, k * 0.8),
    Vec2(-h + k * 3, k * 0.5),
    Vec2(-h + k * 2, k * 1.5),
    Vec2(-h, k * 1.5),
  ]);
}

class SplinesScene extends LabScene {
  /// The whole page; with [still], nothing moves on its own (for its
  /// golden): the rope hangs as it starts and the cart stays put.
  SplinesScene({this.still = false});

  final bool still;

  @override
  String get title => 'Splines';
  @override
  String get step => 'stage 13 phase 3';
  @override
  String get about =>
      'ROAD: Mesh.sweep(spline, shape) with a 12-point profile; drag the white knots and it is swept again into '
      'the same mesh (setSweep) as they move. '
      'ROPE: 16 points hanging between two posts, stepped each frame, given to its spline with setPositions and '
      'swept again, making no garbage. '
      'FENCE: posts placed a metre apart along a curve by SplineInstantiate, its two rails swept along the same '
      'spline. '
      'TRACK: a closed spline, its rails swept and its sleepers placed along it; the cart rides it with '
      'SplineAnimate. "draw splines" turns on world.debug.drawSplines. "time a 100 m road" sweeps a 100 m road '
      'again every frame for two seconds and gives the median.';

  @override
  Vec3 get eye => Vec3(0, 5.2, 7.4);
  @override
  Vec3 get target => Vec3(0, 0, -0.2);

  late final Road road;
  late final Rope rope;
  late final GameObject fence;
  late final GameObject track;
  late final GameObject cart;
  Timing? _timing;
  var _fps = '–';
  var _timed = 'not timed yet';

  @override
  void build() {
    if (_startQuality.isNotEmpty) world.quality = Quality.levels.firstWhere((q) => q.name == _startQuality);
    road = add(Road());
    for (var i = 0; i < road.spline.knotCount; i++) {
      add(KnotHandle(road, i));
    }
    rope = add(Rope(still: still)..transform = Transform(position: Vec3(-3.6, 0, 2.6)));

    // The fence: posts a metre apart, two rails along the same spline.
    final fenceLine = Spline.smooth([Vec3(1.2, 0, 3.4), Vec3(2.6, 0, 2.2), Vec3(4.2, 0, 2.8), Vec3(5.6, 0, 1.6)]);
    final wood = Material(color: const Color(0xFFB08458), roughness: 0.9);
    final post = Mesh.cube();
    fence = add(
      GameObject(
        name: 'fence',
        components: [
          SplineContainer([fenceLine]),
          SplineInstantiate(
            (i) => GameObject(
              name: 'post $i',
              children: [
                GameObject(
                  transform: Transform(position: Vec3(0, 0.45, 0), scale: Vec3(0.12, 0.9, 0.12)),
                  components: [MeshRenderer(post, wood)],
                ),
              ],
            ),
            spacing: 1,
          ),
        ],
        children: [
          for (final y in [0.4, 0.75])
            GameObject(
              name: 'rail',
              transform: Transform(position: Vec3(0, y, 0)),
              components: [MeshRenderer(Mesh.sweep(fenceLine, Shape2D.rect(0.05, 0.1), segmentsPerMetre: 4), wood)],
            ),
        ],
      ),
    );

    // The track: a closed loop over a hill, rails and sleepers along it.
    final loop = Spline.smooth([
      for (var i = 0; i < 8; i++)
        Vec3(
          2.6 * math.cos(i * math.pi / 4),
          0.05 + 0.35 * (1 + math.sin(i * math.pi / 4 * 2)),
          1.5 * math.sin(i * math.pi / 4),
        ),
    ], closed: true);
    final steel = Material(color: const Color(0xFFB8BCC4), metallic: 0.8, roughness: 0.35);
    final sleeper = Mesh.cube();
    final timber = Material(color: const Color(0xFF6B4A30), roughness: 1);
    track = add(
      GameObject(
        name: 'track',
        transform: Transform(position: Vec3(2.8, 0, -1.6)),
        components: [
          SplineContainer([loop]),
          SplineInstantiate(
            (i) => GameObject(
              name: 'sleeper $i',
              children: [
                GameObject(
                  transform: Transform(position: Vec3(0, -0.06, 0), scale: Vec3(0.8, 0.05, 0.12)),
                  components: [MeshRenderer(sleeper, timber)],
                ),
              ],
            ),
            spacing: 0.35,
          ),
        ],
        children: [
          for (final side in [-0.25, 0.25])
            GameObject(
              name: 'rail',
              components: [
                MeshRenderer(
                  Mesh.sweep(loop, Shape2D.circle(0.035, segments: 8, centre: Vec2(side, 0)), segmentsPerMetre: 6),
                  steel,
                ),
              ],
            ),
        ],
      ),
    );
    cart = add(
      GameObject(
        name: 'cart',
        components: [SplineAnimate(track.get<SplineContainer>()!, speed: 1.6, playOnStart: !still, startAt: 2)],
        children: [
          shape('body', Mesh.cube(), const Color(0xFFD8343C), at: Vec3(0, 0.22, 0), size: Vec3(0.45, 0.3, 0.7)),
        ],
      ),
    );
  }

  // With [printRates], the 100 m road is timed by itself once the page
  // has settled, for a run on a device with no one to tap.
  var _timedOnce = false;

  @override
  void onUpdate() {
    super.onUpdate();
    if (printRates && !_timedOnce && time.sinceStart > 3) {
      _timedOnce = true;
      _timeRoad();
    }
    final timing = _timing;
    if (timing != null && timing.step()) {
      _timed = timing.result;
      log.add('a 100 m road: $_timed');
      if (printRates) {
        // ignore: avoid_print
        print('[splines] a 100 m road: $_timed');
      }
      timing.destroy();
      _timing = null;
    }
    readout.value = [
      'road ${road.spline.length.toStringAsFixed(1)} m, ${road.mesh.indexCount ~/ 3} triangles, swept again in '
          '${_ms(road.lastSweep)} (sent in ${_ms(world.meshChangeTime.inMicroseconds)})',
      'rope ${rope.mesh.indexCount ~/ 3} triangles each frame in ${_ms(rope.work)}; '
          'fence ${fence.get<SplineInstantiate>()!.copies.length} posts; cart at '
          '${cart.get<SplineAnimate>()!.distance.toStringAsFixed(1)} m',
      if (_timing != null) 'timing a 100 m road…' else 'a 100 m road: $_timed',
      '$_fps fps',
    ].join('\n');
  }

  @override
  Widget view(BuildContext context) => FynkelView(root: this, onStats: (s) => _fps = fps('splines', s));

  String _ms(int micros) => '${(micros / 1000).toStringAsFixed(2)} ms';

  @override
  List<Control> get controls => [
    Toggle('draw splines', () => world.debug.drawSplines, (on) => world.debug.drawSplines = on),
    Toggle('rope swinging', () => rope.swinging, (on) => rope.swinging = on),
    Toggle('cart moving', () => cart.get<SplineAnimate>()!.isPlaying, (on) {
      final ride = cart.get<SplineAnimate>()!;
      on ? ride.play() : ride.pause();
    }),
    Slider(
      'fence spacing',
      () => fence.get<SplineInstantiate>()!.spacing,
      (v) {
        fence.get<SplineInstantiate>()!.spacing = v;
      },
      min: 0.4,
      max: 2,
    ),
    Button('time a 100 m road', _timeRoad),
  ];

  void _timeRoad() {
    if (_timing != null) return;
    _timing = add(Timing());
    log.add('timing a 100 m road for 120 frames');
  }
}

/// A road 1.6 m wide along a spline of five knots, swept again whenever a
/// knot moves.
final class Road extends GameObject {
  Road() : super(name: 'road');

  final spline = Spline.smooth([
    Vec3(-6.2, 0.02, -2.6),
    Vec3(-4.8, 0.02, -0.8),
    Vec3(-3, 0.3, -1.6),
    Vec3(-1.8, 0.02, -3.2),
    Vec3(-0.4, 0.02, -3.8),
  ]);
  final _profile = roadProfile(1.6);
  late final Mesh mesh;
  var _version = -1;

  /// What the last sweep took, in microseconds.
  var lastSweep = 0;
  final _watch = Stopwatch();

  @override
  void onLoad() {
    super.onLoad();
    mesh = Mesh.sweep(spline, _profile, segmentsPerMetre: 4);
    _version = spline.version;
    addComponent(MeshRenderer(mesh, Material(color: const Color(0xFF55585E), roughness: 0.85)));
    addComponent(SplineContainer([spline]));
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (spline.version == _version) return;
    _watch
      ..reset()
      ..start();
    mesh.setSweep(spline, _profile, segmentsPerMetre: 4);
    lastSweep = _watch.elapsedMicroseconds;
    _version = spline.version;
  }
}

/// A white ball on a road's knot: dragged over the ground, it moves the
/// knot.
final class KnotHandle extends GameObject {
  KnotHandle(this.road, this.index) : super(name: 'knot $index');

  final Road road;
  final int index;
  static final _ball = Mesh.sphere();

  @override
  void onLoad() {
    super.onLoad();
    transform = Transform(position: road.spline[index].position + Vec3(0, 0.25, 0), scale: Vec3(0.3, 0.3, 0.3));
    addComponent(MeshRenderer(_ball, Material(color: const Color(0xFFF2EEE6))));
  }

  @override
  bool onDragStart(Finger finger) => true;

  @override
  void onDrag(Finger finger) {
    final camera = ancestor<SplinesScene>()!.find('camera')!.get<Camera>()!;
    final ray = camera.screenPointToRay(finger.position);
    final height = road.spline[index].position.y;
    if (ray.direction.y.abs() < 1e-6) return;
    final t = (height - ray.origin.y) / ray.direction.y;
    if (t <= 0) return;
    final at = ray.origin + ray.direction * t;
    placeKnot(Vec3(at.x.clamp(-7.0, 7.0), height, at.z.clamp(-5.0, 4.0)));
  }

  /// Puts the knot, and the ball, at [at].
  void placeKnot(Vec3 at) {
    road.spline[index] = Knot(at);
    position = at + Vec3(0, 0.25, 0);
  }
}

/// A rope of 16 points hanging between two posts 4 m apart, stepped as a
/// chain of sticks each frame (Verlet), a gust swinging it; given to its
/// spline and swept again into the same mesh each frame.
final class Rope extends GameObject {
  Rope({this.still = false}) : super(name: 'rope');

  final bool still;
  static const points = 16, span = 4.0, slack = 1.18, height = 1.6;

  final mesh = Mesh();
  final spline = Spline([]);
  final _shape = Shape2D.circle(0.035, segments: 8);
  final _now = Float32List(points * 3);
  final _before = Float32List(points * 3);
  var swinging = true;

  /// What the last frame's step and sweep took, in microseconds.
  var work = 0;
  final _watch = Stopwatch();

  static final _post = Mesh.cylinder();

  @override
  void onLoad() {
    super.onLoad();
    // Hanging as a parabola to start with, its length about right.
    final sag = span * math.sqrt(3 * (slack - 1) / 8);
    for (var i = 0; i < points; i++) {
      final u = i / (points - 1);
      _now
        ..[i * 3] = u * span
        ..[i * 3 + 1] = height - 4 * sag * u * (1 - u)
        ..[i * 3 + 2] = 0;
    }
    _before.setAll(0, _now);
    for (var i = 0; i < points; i++) {
      spline.add(Knot(Vec3(_now[i * 3], _now[i * 3 + 1], _now[i * 3 + 2])));
    }
    mesh
      ..setSweep(spline, _shape, segmentsPerMetre: 8, maxSegments: 64)
      ..markDynamic();
    addComponent(MeshRenderer(mesh, Material(color: const Color(0xFFD8C08A), roughness: 0.9)));
    for (final x in [0.0, span]) {
      add(
        GameObject(
          name: 'post',
          // Unity's cylinder, 2 m tall: a little above where the rope's tied.
          transform: Transform(position: Vec3(x, (height + 0.1) / 2, 0), scale: Vec3(0.12, (height + 0.1) / 2, 0.12)),
          components: [MeshRenderer(_post, Material(color: const Color(0xFF8A6A48)))],
        ),
      );
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    if (still || !swinging) {
      work = 0;
      return;
    }
    _watch
      ..reset()
      ..start();
    _step(math.min(time.delta, 1 / 30), time.sinceStart);
    spline.setPositions(_now);
    mesh.setSweep(spline, _shape, segmentsPerMetre: 8, maxSegments: 64);
    work = _watch.elapsedMicroseconds;
  }

  /// Gravity and a gust, then each stick pulled back to its length, the
  /// ends held on the posts.
  void _step(double dt, double t) {
    final (now, before) = (_now, _before);
    final gust = math.sin(t * 1.3) * 3.5;
    for (var i = 1; i < points - 1; i++) {
      for (var a = 0; a < 3; a++) {
        final k = i * 3 + a;
        final v = (now[k] - before[k]) * 0.995;
        before[k] = now[k];
        final push = a == 1 ? -9.8 : (a == 2 ? gust : 0.0);
        now[k] += v + push * dt * dt;
      }
    }
    const stick = span * slack / (points - 1);
    for (var round = 0; round < 12; round++) {
      for (var i = 0; i < points - 1; i++) {
        final (p, q) = (i * 3, i * 3 + 3);
        final dx = now[q] - now[p], dy = now[q + 1] - now[p + 1], dz = now[q + 2] - now[p + 2];
        final d = math.sqrt(dx * dx + dy * dy + dz * dz);
        if (d == 0) continue;
        final f = (d - stick) / d;
        // The ends stay on their posts.
        final (wp, wq) = i == 0 ? (0.0, 1.0) : (i == points - 2 ? (1.0, 0.0) : (0.5, 0.5));
        now[p] += dx * f * wp;
        now[p + 1] += dy * f * wp;
        now[p + 2] += dz * f * wp;
        now[q] -= dx * f * wq;
        now[q + 1] -= dy * f * wq;
        now[q + 2] -= dz * f * wq;
      }
    }
  }
}

/// A 100 m road with a 12-point profile, a knot moved and the road swept
/// again every frame for 120 frames: the median of the sweeps, and of the
/// meshes sent, which [step] gives when it's done.
final class Timing extends GameObject {
  // Out of sight under the floor, but drawn and sent as any mesh is.
  Timing()
    : super(
        name: '100 m road',
        transform: Transform(position: Vec3(0, -4, 0)),
      );

  final spline = Spline.smooth([for (var i = 0; i <= 10; i++) Vec3(i * 10.0 - 50, 0, math.sin(i * 0.9) * 8 - 40)]);
  final _profile = roadProfile(6);
  late final mesh = Mesh.sweep(spline, _profile, segmentsPerMetre: 2)..markDynamic();
  final _sweeps = <int>[], _sends = <int>[];
  final _watch = Stopwatch();
  var result = '';

  @override
  void onLoad() {
    super.onLoad();
    addComponent(MeshRenderer(mesh, Material(color: const Color(0xFF55585E))));
  }

  /// Moves a knot and sweeps again; true when the 120 frames are done.
  bool step() {
    if (_sweeps.isNotEmpty) _sends.add(world.meshChangeTime.inMicroseconds);
    if (_sweeps.length == 120) {
      int median(List<int> t) => (t..sort())[t.length ~/ 2];
      result =
          'swept in ${(median(_sweeps) / 1000).toStringAsFixed(2)} ms, sent in '
          '${(median(_sends) / 1000).toStringAsFixed(2)} ms (medians, ${mesh.indexCount ~/ 3} triangles)';
      return true;
    }
    final k = spline[5];
    spline[5] = Knot(Vec3(k.position.x, math.sin(_sweeps.length * 0.2) * 3, k.position.z));
    _watch
      ..reset()
      ..start();
    mesh.setSweep(spline, _profile, segmentsPerMetre: 2);
    _sweeps.add(_watch.elapsedMicroseconds);
    return false;
  }
}
