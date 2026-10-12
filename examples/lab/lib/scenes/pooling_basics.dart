// Stage 6: pooling, the basics. A fountain of balls from a Pool: each one
// flies, falls and goes back to the pool, to be thrown again. A marker
// follows one ball through an ObjectHandle, which goes null when the ball
// goes back, even though the pool soon throws it again.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

import '../lab.dart';

/// Flies from where it's thrown, falls, and after a while goes back to
/// its pool: [release] (or, made without a pool, is destroyed).
class Ball extends GameObject {
  Ball(Material material)
    : super(
        name: 'ball',
        transform: Transform(scale: Vec3(0.25, 0.25, 0.25)),
        components: [MeshRenderer(Mesh.sphere(), material)],
      );

  var velocity = Vec3.zero;
  var age = 0.0;

  // Each time it's thrown: the pool's reset set velocity and position.
  @override
  void onEnable() {
    super.onEnable();
    age = 0;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final dt = time.delta;
    velocity += Vec3(0, -9.8 * dt, 0);
    position += velocity * dt;
    age += dt;
    if (age > 1.6) release();
  }
}

class PoolingBasicsScene extends LabScene {
  @override
  String get title => 'Pooling: the basics';
  @override
  String get step => 'stage 6';
  @override
  String get about =>
      'A fountain of balls from a Pool: get hands one out, release takes it back for the next. With the pool, '
      '"made" stops growing; without, each ball is made and destroyed. The yellow cube follows one ball '
      'through an ObjectHandle: when that ball goes back, the handle is null, and the cube picks another.';

  @override
  Vec3 get eye => Vec3(0, 3, 9);
  @override
  Vec3 get target => Vec3(0, 1.5, 0);

  final _random = math.Random(1);
  late final Material _ballMaterial = Material(color: const Color(0xFF4CA6E0));
  late final balls = Pool(this, () {
    made++;
    return Ball(_ballMaterial);
  });

  var pooled = true;
  var made = 0;
  var _wait = 0.0;
  Ball? _last;
  ObjectHandle<Ball>? followed;
  late final GameObject marker;

  @override
  void build() {
    marker = add(
      GameObject(
        name: 'marker',
        transform: Transform(scale: Vec3(0.15, 0.15, 0.15)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFFFFD040)))],
      ),
    );
  }

  /// Throws a ball up from the middle, a little to one side.
  void throwBall() {
    final angle = _random.nextDouble() * 2 * math.pi;
    void reset(Ball ball) {
      ball.position = Vec3(0, 0.3, 0);
      ball.velocity = Vec3(math.cos(angle) * 1.5, 7, math.sin(angle) * 1.5);
    }

    if (pooled) {
      _last = balls.get(reset);
    } else {
      made++;
      _last = add(Ball(_ballMaterial));
      reset(_last!);
    }
  }

  @override
  void onUpdate() {
    super.onUpdate();
    _wait -= time.delta;
    while (_wait <= 0) {
      _wait += 0.08;
      throwBall();
    }
    if (followed?.object == null && _last != null) {
      if (followed != null) log.add('the ball the cube followed went back: its handle is null');
      followed = ObjectHandle(_last!);
    }
    if (followed?.object case final ball?) marker.position = ball.position + Vec3(0, 0.35, 0);
    readout.value = 'active ${balls.countActive}   waiting ${balls.countInactive}   made $made';
  }

  @override
  List<Control> get controls => [
    Toggle('use the pool', () => pooled, (on) {
      pooled = on;
      log.add(on ? 'balls come from the pool' : 'each ball is made, then destroyed');
    }),
    Button('prewarm 40', () {
      balls.prewarm(40);
      log.add('balls.prewarm(40): 40 waiting, made ahead');
    }),
    Button('clear the waiting ones', () {
      final waiting = balls.countInactive;
      balls.clear();
      log.add('balls.clear(): $waiting waiting balls destroyed');
    }),
  ];
}
