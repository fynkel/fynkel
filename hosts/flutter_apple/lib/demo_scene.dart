// The demo scene in Dart, written the way a game would be on package:fynkel:
// spawn GameObjects, then move them from fixedUpdate. Mirrors
// hosts/headless/src/scene.rs (same LCG, same motion) so the three hosts
// show the same world.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

class _Satellite {
  _Satellite(this.radius, this.phase, this.angularSpeed, this.spinSpeed, this.height, this.scale);
  final double radius, phase, angularSpeed, spinSpeed, height, scale;
  GameObject? object;
}

class DemoScene {
  DemoScene({int satellites = 24, int seed = 42}) {
    final rng = _Lcg(seed ^ 0x9E3779B97F4A7C15);
    for (var i = 0; i < satellites; i++) {
      _satellites.add(_Satellite(
        1.6 + rng.unit() * 2.2 + (i ~/ 12) * 0.35,
        rng.unit() * 2 * math.pi,
        0.3 + rng.unit() * 0.9,
        -2.0 + rng.unit() * 4.0,
        -0.6 + rng.unit() * 1.2,
        0.18 + rng.unit() * 0.22,
      ));
    }
  }

  final List<_Satellite> _satellites = [];
  GameObject? hub;
  bool _satellitesSpawned = false;

  int get expectedEntities => 1 + _satellites.length;
  bool get isReady => hub?.isAlive == true && _satellites.every((s) => s.object?.isAlive == true);

  /// Gameplay for the tick about to be simulated.
  void fixedUpdate(World world, int tick) {
    final t = tick * world.fixedDt;
    final hub = this.hub ??= world.spawn(name: 'hub');
    if (hub.isPending) return; // resolved by next tick's Spawned event
    if (!_satellitesSpawned) {
      _satellitesSpawned = true;
      for (final s in _satellites) {
        s.object = world.spawn(transform: _satelliteLocal(s, t), parent: hub);
      }
    }
    // Hot path: reusable writer, no Transform/Vec3/Quat per write (C1).
    hub.writeLocal(_w.identity().setRotationAxisAngle(0, 1, 0, t * 0.8).rotateAxisAngle(1, 0, 0, t * 0.35).setUniformScale(0.8));
    for (final s in _satellites) {
      final o = s.object;
      if (o != null && !o.isPending) o.writeLocal(_satelliteInto(s, t, _w));
    }
  }

  final TransformWriter _w = TransformWriter();

  static Pose _satelliteLocal(_Satellite s, double t) => _satelliteInto(s, t, TransformWriter()).toTransform();

  static TransformWriter _satelliteInto(_Satellite s, double t, TransformWriter w) {
    final a = s.phase + s.angularSpeed * t;
    return w
        .setPosition(math.cos(a) * s.radius, s.height, math.sin(a) * s.radius)
        .setRotationAxisAngle(0, 1, 0, s.spinSpeed * t)
        .setUniformScale(s.scale);
  }
}

/// 64-bit LCG (Knuth MMIX), wrapping like the Rust one.
class _Lcg {
  _Lcg(this._state);
  int _state;

  int next() {
    _state = _state * 6364136223846793005 + 1442695040888963407;
    return _state;
  }

  double unit() => ((next() >>> 40) & 0xFFFFFF) / (1 << 24);
}
