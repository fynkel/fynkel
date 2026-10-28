// Curves, not points (stage 14, phase 4): what a frame costs against the
// number of objects, lamps, particles, sounds and bodies, on the game's
// thread (world.profile) and drawing it (a headless view), to find what
// grows faster than it should. Each count is a fresh world, run a second
// and then timed for two, the median frame kept.
//
// The growth is how the cost over an empty world's goes from the second
// count to the last, 4 times it: 1 is in step with the count (4 times the
// cost), 2 is with its square. Anything clearly over 1 fails. (The first
// count's cost over the empty world's is too small to read it from.)
//
// Off by default; CI's long runs set it:
//
//   FYNKEL_CURVES=1 flutter test test/curves_test.dart

import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_audio/fynkel_audio.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:lab/lab.dart';
import 'package:lab/scenes/sound_3d.dart' show buzz;

Future<Uint8List> files(String asset) async => File('assets/$asset').readAsBytesSync();

final on = Platform.environment['FYNKEL_CURVES'] == '1';

/// Over this growth a curve fails: 1 is in step with the count, and the
/// medians of a busy machine move a cost by a third or so either way.
const tooSteep = 1.35;

/// A cost over the empty world's under this many milliseconds, at the
/// count the growth is read from, is within a frame's noise: too small to
/// read a growth from, and the curve isn't held to one.
const tooSmall = 0.08;

void main() {
  final results = <String>[];
  tearDownAll(() {
    if (results.isEmpty) return;
    // ignore: avoid_print
    print('\n${results.join('\n')}');
  });

  for (final curve in curves) {
    test(
      '${curve.name}: the cost grows no faster than the count',
      () async {
        final empty = await measure(curve, 0);
        final points = [for (final n in curve.counts) (n: n, cost: await measure(curve, n))];
        String ms(double v) => v.toStringAsFixed(3);
        for (final (kind, pick) in [('game', (Cost c) => c.game), ('drawn', (Cost c) => c.drawn)]) {
          final base = pick(empty);
          final from = points[1], to = points.last;
          final first = pick(from.cost) - base, last = pick(to.cost) - base;
          final growth = first > 0 && last > 0 ? math.log(last / first) / math.log(to.n / from.n) : double.nan;
          final line =
              '${curve.name.padRight(14)} $kind: empty ${ms(base)} ms; '
              '${points.map((p) => '${p.n} ${ms(pick(p.cost))}').join(', ')} ms; '
              'growth ${growth.toStringAsFixed(2)}';
          final known = kind == 'game' ? curve.known : null;
          results.add(known == null ? line : '$line (known: $known)');
          if (first < tooSmall || known != null) continue;
          expect(growth, lessThan(tooSteep), reason: line);
        }
      },
      skip: on ? false : 'set FYNKEL_CURVES=1 to run it',
      timeout: Timeout.none,
    );
  }
}

typedef Cost = ({double game, double drawn});

/// A count of something put into a scene, and the counts to time it at.
typedef Curve = ({String name, List<int> counts, void Function(CurveScene scene, int n) make, String? known});

final curves = <Curve>[
  // Objects drawn and moved by the game each frame.
  // Past 4,000 or so their data no longer fits the cache, and each costs
  // more: the growth is read from above that.
  (name: 'moving objects', counts: [2000, 4000, 16000, 32000], make: (s, n) => s.cubes(n, moving: true), known: null),
  // Objects drawn and left where they are: a frame should cost little
  // more for them than for none. On the game's thread it grows faster
  // than they do, as the updates visit each one three times (DC4).
  (name: 'still objects', counts: [2000, 4000, 16000, 32000], make: (s, n) => s.cubes(n), known: 'DC4'),
  // Lamps over 1,000 objects.
  (
    name: 'lamps',
    counts: [8, 16, 32, 64],
    make: (s, n) {
      s.cubes(1000);
      s.lamps(n);
    },
    known: null,
  ),
  (name: 'particles', counts: [1250, 2500, 5000, 10000], make: (s, n) => s.particles(n), known: null),
  (name: 'sounds', counts: [4, 8, 16, 32], make: (s, n) => s.sounds(n), known: null),
  (name: 'bodies', counts: [100, 200, 400, 800], make: (s, n) => s.bodies(n), known: null),
];

/// The median frame of [curve]'s scene with [n] of its things, on the
/// game's thread and drawn: the lowest of three tries, each in a fresh
/// world, as the benchmarks keep their best (a busy machine only adds).
Future<Cost> measure(Curve curve, int n) async {
  var best = (game: double.infinity, drawn: double.infinity);
  for (var i = 0; i < 3; i++) {
    final c = await _measure(curve, n);
    best = (game: math.min(best.game, c.game), drawn: math.min(best.drawn, c.drawn));
  }
  return best;
}

Future<Cost> _measure(Curve curve, int n) async {
  final world = World(source: files);
  final view = HeadlessView(world, width: 256, height: 256);
  try {
    final scene = world.add(CurveScene(curve, n));
    await scene.loaded;
    for (var i = 0; i < 60; i++) {
      world.update(1 / 60);
      if (i % 10 == 0) view.read(view.frame());
    }
    // What's timed is there: the particles all alive, the bodies awake.
    if (scene._particles case final particles?) {
      expect(particles.particleCount, greaterThan(n * 0.9), reason: 'particles alive');
    }
    final game = <int>[], drawn = <int>[];
    final watch = Stopwatch();
    for (var i = 0; i < 120; i++) {
      world.update(1 / 60);
      game.add(world.profile.total.inMicroseconds);
      watch
        ..reset()
        ..start();
      view.read(view.frame());
      drawn.add(watch.elapsedMicroseconds);
    }
    expect(scene._bodies.where((b) => b.isSleeping), isEmpty, reason: 'bodies awake');
    double median(List<int> t) => (t..sort())[t.length ~/ 2] / 1000;
    return (game: median(game), drawn: median(drawn));
  } finally {
    view.dispose();
    world.dispose();
  }
}

/// The lab's stage (camera, sun, floor) with [curve]'s things in it.
class CurveScene extends LabScene {
  CurveScene(this.curve, this.n);

  final Curve curve;
  final int n;

  @override
  String get title => curve.name;
  @override
  String get step => 'stage 14';
  @override
  String get about => '';
  @override
  List<Control> get controls => const [];

  @override
  Vec3 get eye => Vec3(0, 30, 40);

  final _moving = <GameObject>[];
  final _clips = <AudioClip>[];
  ParticleSystem? _particles;
  final _bodies = <Rigidbody>[];
  var _t = 0.0;

  @override
  void build() {
    if (n > 0) curve.make(this, n);
  }

  /// [n] cubes in a square on the floor; [moving], each moved every frame.
  void cubes(int n, {bool moving = false}) {
    final side = math.sqrt(n).ceil();
    final mesh = Mesh.cube();
    final material = Material(color: const Color(0xFF8090A0));
    for (var i = 0; i < n; i++) {
      final cube = add(
        GameObject(
          transform: Transform(
            position: Vec3((i % side - side / 2) * 0.6, 0.2, (i ~/ side - side / 2) * 0.6),
            scale: Vec3(0.3, 0.3, 0.3),
          ),
          components: [MeshRenderer(mesh, material)],
        ),
      );
      if (moving) _moving.add(cube);
    }
  }

  /// [n] point lamps over the floor.
  void lamps(int n) {
    for (var i = 0; i < n; i++) {
      final a = i / n * math.pi * 2;
      add(
        GameObject(
          transform: Transform(position: Vec3(math.cos(a) * 8, 1, math.sin(a) * 8)),
          components: [Light.point(color: const Color(0xFFFFC080), intensity: 3, range: 4)],
        ),
      );
    }
  }

  /// One system of [n] particles, full from the start.
  void particles(int n) {
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 2, 0)),
        components: [
          _particles = ParticleSystem(
            maxParticles: n,
            prewarm: true,
            startLifetime: const Range(2, 2),
            startSpeed: const Range(1, 3),
            startSize: const Range(0.05, 0.1),
            emission: Emission(rateOverTime: n / 2),
            shape: ParticleShape.sphere(radius: 1),
          ),
        ],
      ),
    );
  }

  /// [n] sounds looping in 3D, each moved every frame.
  void sounds(int n) {
    final clip = buzz();
    _clips.add(clip);
    for (var i = 0; i < n; i++) {
      _moving.add(
        add(
          GameObject(
            transform: Transform(position: Vec3(i * 0.5, 1, 0)),
            components: [AudioSource(clip: clip, loop: true, spatialBlend: 1, volume: 0.01)],
          ),
        ),
      );
    }
  }

  /// [n] boxes in piles four high, kept awake, stepped in the frame.
  void bodies(int n) {
    world.physics
      ..stepBetweenFrames = false
      ..sleepThreshold = 0;
    add(floorCollider());
    final side = math.sqrt(n / 4).ceil();
    final mesh = Mesh.cube();
    final material = Material(color: const Color(0xFFC08060));
    for (var i = 0; i < n; i++) {
      final column = i ~/ 4;
      add(
        GameObject(
          transform: Transform(
            position: Vec3((column % side - side / 2) * 1.2, 0.25 + (i % 4) * 0.55, (column ~/ side - side / 2) * 1.2),
            scale: Vec3(0.5, 0.5, 0.5),
          ),
          components: [MeshRenderer(mesh, material), BoxCollider(), _bodies.add2(Rigidbody())],
        ),
      );
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _t += time.delta;
    for (var i = 0; i < _moving.length; i++) {
      final p = _moving[i].position;
      _moving[i].position = Vec3(p.x, 0.2 + 0.1 * math.sin(_t * 3 + i), p.z);
    }
  }

  @override
  void onDestroy() {
    super.onDestroy();
    for (final clip in _clips) {
      clip.dispose();
    }
  }
}

extension<T> on List<T> {
  /// Adds [item] and gives it back.
  T add2(T item) {
    add(item);
    return item;
  }
}
