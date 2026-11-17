// The game: ten-pin bowling on a lane of real size. The ball waits on the
// foul line: drag it sideways to place it, and flick it up the lane to
// bowl, as hard and as straight as the flick. Ten frames, scored as
// bowling is.

import 'dart:math' as math;

import 'package:flutter/foundation.dart' show ValueNotifier;
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import 'score.dart';

/// The layers: the lane the ball rolls on, the ball, and the pins.
const lane = 0, balls = 8, pins = 9;

/// The lane, as bowling has it: 1.05 m wide, 18.29 m from the foul line
/// (z = 0) to the head pin, and the pit behind the pins.
const laneWidth = 1.05, headPin = 18.29, laneEnd = 19.2;

/// Where the ball waits, and starts: just past the foul line.
const ballStart = -0.3;

class BowlingGame extends GameObject {
  final score = Score();

  /// What the HUD says under the score.
  final message = ValueNotifier('');

  /// Whether the HUD shows how to bowl: until the first ball is bowled.
  final hint = ValueNotifier(true);

  /// Where the finger has been in the gesture under way, in the view's
  /// logical pixels, for the HUD to draw; empty with no gesture.
  final trail = ValueNotifier<List<Vec2>>(const []);

  /// The rolling ball, and the one that waits on the foul line to be
  /// bowled, where it's placed ([aimX]).
  Ball? ball;
  late final GameObject waiting;
  double _aimX = 0;
  late final GameObject _aimLine;

  final _pins = <Pin>[];
  var _rolling = false;
  var _ballDone = false;
  var _gutter = false;

  /// Whether a roll is under way, from the flick to the pins counted.
  bool get rolling => _rolling;

  /// The pins standing on the deck.
  Iterable<Pin> get standing => _pins.where((p) => !p.isDestroyed && !p.down);

  /// Where across the lane the waiting ball is.
  double get aimX => _aimX;
  set aimX(double x) {
    _aimX = x.clamp(-laneWidth / 2 + Ball.radius, laneWidth / 2 - Ball.radius);
    waiting.position = Vec3(_aimX, Ball.radius, ballStart);
  }

  @override
  void onLoad() {
    super.onLoad();
    // 100 steps a second: a ball rolling at 9 m/s turns 80 times a second,
    // and physics turns a body no more than a quarter turn a step.
    time.fixedDelta = 1 / 100;
    // Its own look, from before a new world had a sky and a sun shadows:
    // the camera's clear colour behind, an ambient light, no shadows.
    world.sky = null;
    add(GameObject(components: [Light.sun(direction: Vec3(0.3, -1, -0.5), intensity: 2.5, shadows: false)]));
    add(GameObject(components: [Light.ambient(color: Color.rgb(0.75, 0.8, 1), intensity: 0.7)]));
    add(Alley());
    add(ChaseCamera(this));
    waiting = add(
      GameObject(
        name: 'waiting ball',
        transform: Transform(position: Vec3(0, Ball.radius, ballStart), scale: Vec3.one * (Ball.radius * 2)),
        components: [MeshRenderer(Mesh.sphere(), Ball.look)],
      ),
    );
    final glow = Material(color: Color.rgb(1, 0.85, 0.3), emissive: Color.rgb(1, 0.8, 0.2), emissiveIntensity: 2);
    _aimLine = add(GameObject(name: 'aim', components: [MeshRenderer(Mesh.cube(), glow)]))..active = false;
    addComponent(Bowler());
    rack();
  }

  /// Sets up all ten pins, where they stand, and takes away the old ones.
  void rack() {
    for (final pin in _pins) {
      pin.destroy();
    }
    _pins.clear();
    for (var row = 0; row < 4; row++) {
      for (var i = 0; i <= row; i++) {
        final at = Vec3((i - row / 2) * 0.3048, 0, -headPin - row * 0.264);
        _pins.add(add(Pin(at)));
      }
    }
  }

  /// Shows where a ball bowled [angle] radians to the right of straight
  /// would go, down to the pins; null hides it.
  void aim(double? angle) {
    _aimLine.active = angle != null;
    if (angle == null) return;
    final length = (headPin + ballStart) / math.cos(angle);
    final way = Vec3(math.sin(angle), 0, -math.cos(angle));
    _aimLine.transform = Transform(
      position: Vec3(_aimX, 0.004, ballStart) + way * (length / 2),
      rotation: Quat.axisAngle(Vec3.up, -angle),
      scale: Vec3(0.025, 0.004, length),
    );
  }

  /// Bowls the waiting ball at [speed] metres a second, [angle] radians to
  /// the right of straight down the lane. Then, once it's gone and the pins
  /// have settled, counts them.
  void roll({required double speed, required double angle}) {
    if (_rolling) return;
    if (score.isOver) {
      score.clear();
      rack();
    }
    _rolling = true;
    _ballDone = false;
    _gutter = false;
    hint.value = false;
    waiting.active = false;
    aim(null);
    message.value = '${speed.toStringAsFixed(1)} m/s';
    final velocity = Vec3(math.sin(angle), 0, -math.cos(angle)) * speed;
    ball = add(Ball(waiting.position, velocity));
    startRoutine(() async {
      final rolled = time.sinceStart;
      await waitUntil(() => _ballDone || time.sinceStart - rolled > 8);
      // The pins fall for a while after the ball has gone.
      await wait(2);
      _count();
    });
  }

  /// The ball went into the pit.
  void ballDone() => _ballDone = true;

  /// The ball fell in a gutter: a gutter ball, unless it had hit pins
  /// first and fell in after them.
  void gutter() {
    if (_pins.any((p) => !p.isDestroyed && p.down)) return;
    _gutter = true;
    message.value = 'Gutter';
  }

  void _count() {
    final down = _pins.where((p) => !p.isDestroyed && p.down).toList();
    final frame = score.frame;
    score.add(down.length);
    for (final pin in down) {
      pin.destroy();
    }
    ball?.destroy();
    ball = null;
    if (score.isOver) {
      message.value = 'Game over: ${score.total}. Flick to play again';
    } else {
      message.value = switch (score.lastMark) {
        'X' => 'Strike!',
        '/' => 'Spare!',
        _ when _gutter && down.isEmpty => 'Gutter',
        _ => '${down.length} down',
      };
      // A new frame, or all ten down in the tenth: a new rack.
      if (score.frame != frame || standing.isEmpty) rack();
    }
    waiting.active = true;
    _rolling = false;
  }
}

/// The bowler's hand: a drag sideways places the waiting ball, a flick up
/// the view bowls it, as fast as the flick and leaning as it leans. While
/// the finger is down, its trail and the ball's way show. The arrows and
/// space do the same on a keyboard.
class Bowler extends Behaviour {
  late final BowlingGame game = gameObject as BowlingGame;

  /// What the gesture under way does, once it's clear: places or bowls.
  bool? _placing;

  /// A drag up let go: bowled if a swipe comes with it, else too slow.
  var _letGo = false;

  /// From a flick's way on the view to the ball's on the lane: a flick
  /// leaning 45° bowls about 7° off straight.
  static double angleOf(Vec2 way) => math.atan2(way.x, -way.y) * 0.15;

  /// From a flick's speed on the view, in pixels a second, to the ball's.
  static double speedOf(double pixels) => (pixels / 250).clamp(3.0, 9.0);

  @override
  void onStart() {
    super.onStart();
    on(input.onDragStart, (finger) {
      if (game.rolling) return;
      final way = finger.totalDelta;
      _placing = way.x.abs() > way.y.abs();
      _letGo = false;
      game.trail.value = [finger.startPosition.toVec2(), finger.position.toVec2()];
    });
    on(input.onDrag, (finger) {
      if (_placing == null) return;
      game.trail.value = [...game.trail.value.skip(game.trail.value.length > 24 ? 1 : 0), finger.position.toVec2()];
      if (_placing!) {
        final hit = physics.raycast(world.camera!.screenPointToRay(finger.position), layers: LayerMask.of([lane]));
        if (hit != null) game.aimX = hit.point.x;
      } else {
        final way = finger.totalDelta;
        game.aim(way.y < 0 ? angleOf(way) : null);
      }
    });
    on(input.onDragEnd, (finger) {
      if (_placing == false) _letGo = true;
      _placing = null;
      game.aim(null);
      startRoutine(() async {
        await wait(0.3);
        if (_placing == null) game.trail.value = const [];
      });
    });
    on(input.onSwipe, (swipe) {
      if (!_letGo || swipe.direction.y > -0.5) return;
      _letGo = false;
      game.roll(speed: speedOf(swipe.speed), angle: angleOf(swipe.direction));
    });
    on(input.onKeyDown, (press) {
      if (game.rolling) return;
      switch (press.key) {
        case KeyCode.arrowLeft:
          game.aimX -= 0.05;
        case KeyCode.arrowRight:
          game.aimX += 0.05;
        case KeyCode.space:
          game.roll(speed: 7.5, angle: 0);
        default:
      }
    });
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // A drag up let go with no swipe: too slow to bowl.
    if (_letGo) {
      _letGo = false;
      game.message.value = 'Flick faster to bowl';
    }
  }
}

/// The lane, its gutters and the pit behind the pins, and the triggers in
/// the gutters and the pit that say the ball's roll is over.
class Alley extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    final wood = Material(color: Color.rgb(0.85, 0.65, 0.4), roughness: 0.25);
    final dark = Material(color: Color.rgb(0.2, 0.2, 0.24), roughness: 0.8);
    // Waxed wood: the ball slides on it before it rolls.
    const waxed = PhysicsMaterial(dynamicFriction: 0.1);
    final half = laneWidth / 2;
    // The approach, the lane and the pin deck, its top at y = 0.
    add(_block('lane', Vec3(0, -0.05, -(laneEnd - 4) / 2), Vec3(laneWidth, 0.1, laneEnd + 4), wood, waxed));
    for (final side in [-1.0, 1.0]) {
      // A gutter, 8 cm lower, and its trigger.
      final x = side * (half + 0.12);
      add(_block('gutter', Vec3(x, -0.13, -laneEnd / 2), Vec3(0.24, 0.1, laneEnd), dark, waxed));
      add(Gutter(Vec3(x, 0, -laneEnd / 2 - 1), Vec3(0.24, 0.1, laneEnd - 2)));
      // The side walls (kickbacks), by the pins and along the pit.
      add(_block('wall', Vec3(side * (half + 0.29), 0.3, -laneEnd + 1), Vec3(0.1, 0.8, 4.4), dark));
      // The capping beyond the gutters, 2 cm above the lane: a ball in a
      // gutter stays in it.
      add(_block('capping', Vec3(side * (half + 0.24 + 1.8), -0.03, -laneEnd / 2), Vec3(3.6, 0.1, laneEnd + 8), dark));
    }
    // The pit: a floor below the deck, a cushion at its back.
    add(_block('pit', Vec3(0, -0.5, -laneEnd - 0.7), Vec3(laneWidth + 0.6, 0.1, 1.4), dark));
    add(_block('cushion', Vec3(0, 0.2, -laneEnd - 1.45), Vec3(laneWidth + 0.6, 1.4, 0.1), dark));
    // The masking over the pins, softly lit, as an alley's is.
    final masking = Material(
      color: Color.rgb(0.2, 0.25, 0.4),
      emissive: Color.rgb(0.2, 0.3, 0.6),
      emissiveIntensity: 0.3,
    );
    add(_block('masking', Vec3(0, 1.3, -laneEnd - 1.5), Vec3(laneWidth + 0.8, 0.8, 0.1), masking));
    add(Pit(Vec3(0, -0.3, -laneEnd - 0.7), Vec3(laneWidth + 0.6, 0.3, 1.4)));
  }

  static GameObject _block(String name, Vec3 at, Vec3 size, Material look, [PhysicsMaterial? material]) => GameObject(
    name: name,
    transform: Transform(position: at, scale: size),
    components: [
      MeshRenderer(Mesh.cube(), look),
      BoxCollider(material: material ?? PhysicsMaterial.standard),
    ],
  );
}

/// A trigger along a gutter: a ball in it rolls on to the pit, but says so.
class Gutter extends GameObject with Triggers {
  Gutter(Vec3 at, Vec3 size)
    : super(
        name: 'gutter trigger',
        transform: Transform(position: at, scale: size),
        components: [BoxCollider(isTrigger: true)],
      );

  @override
  void onTriggerEnter(Collider other) {
    if (other.gameObject is Ball) ancestor<BowlingGame>()!.gutter();
  }
}

/// A trigger in the pit: a ball in it has rolled.
class Pit extends GameObject with Triggers {
  Pit(Vec3 at, Vec3 size)
    : super(
        name: 'pit trigger',
        transform: Transform(position: at, scale: size),
        components: [BoxCollider(isTrigger: true)],
      );

  @override
  void onTriggerEnter(Collider other) {
    if (other.gameObject is Ball) ancestor<BowlingGame>()!.ballDone();
  }
}

/// A 6.5 kg ball, 21.8 cm across, rolling from the start: continuous, so
/// it doesn't pass through a pin between two steps.
class Ball extends GameObject {
  Ball(Vec3 at, Vec3 velocity)
    : super(
        name: 'ball',
        layer: balls,
        transform: Transform(position: at, scale: const Vec3(radius * 2, radius * 2, radius * 2)),
        components: [
          MeshRenderer(Mesh.sphere(), look),
          SphereCollider(),
          // A rolling ball turns at speed / radius: 80 rad/s at 9 m/s, past
          // the 50 a rigidbody may turn by default. Interpolated: drawn
          // where it is between two steps, so it glides, not jitters, on a
          // screen that draws faster than physics steps or out of step.
          Rigidbody(
            mass: 6.5,
            interpolation: RigidbodyInterpolation.interpolate,
            angularDamping: 0,
            maxAngularVelocity: 100,
            collisionDetectionMode: CollisionDetectionMode.continuous,
          ),
        ],
      ) {
    _velocity = velocity;
  }

  static const radius = 0.109;
  static final look = Material(color: Color.rgb(0.15, 0.3, 0.9), roughness: 0.15);
  late final Vec3 _velocity;

  @override
  void onStart() {
    super.onStart();
    // Rolling as it goes, not sliding: turning about the axis across its way.
    get<Rigidbody>()!
      ..linearVelocity = _velocity
      ..angularVelocity = Vec3.up.cross(_velocity) * (1 / radius);
  }
}

/// A pin, 38 cm tall and 1.5 kg, of the shape bowling's rules give, and a
/// collider the hull of it: it stands on its flat foot.
class Pin extends GameObject {
  Pin(Vec3 at)
    : super(
        name: 'pin',
        layer: pins,
        transform: Transform(position: at),
        components: [
          MeshRenderer(shape, white),
          // Lively, as pins are: knocked into each other, they fly.
          MeshCollider(
            mesh: shape,
            convex: true,
            material: const PhysicsMaterial(dynamicFriction: 0.4, bounciness: 0.7),
          ),
          Rigidbody(mass: 1.5),
        ],
      ) {
    add(
      GameObject(
        name: 'stripe',
        transform: Transform(position: Vec3(0, 0.255, 0), scale: Vec3(0.054, 0.014, 0.054)),
        components: [MeshRenderer(Mesh.sphere(), red)],
      ),
    );
  }

  static final white = Material(color: Color.rgb(0.95, 0.95, 0.92), roughness: 0.3);
  static final red = Material(color: Color.rgb(0.8, 0.1, 0.1), roughness: 0.3);

  /// Its radius at heights from its foot to its top, in metres.
  static const profile = [
    (0.0, 0.0),
    (0.0, 0.026),
    (0.02, 0.04),
    (0.06, 0.054),
    (0.11, 0.0605),
    (0.16, 0.055),
    (0.21, 0.038),
    (0.25, 0.025),
    (0.29, 0.029),
    (0.33, 0.032),
    (0.36, 0.025),
    (0.378, 0.012),
    (0.381, 0.0),
  ];

  /// [profile], turned about Y.
  static final shape = () {
    const sides = 20;
    final positions = <Vec3>[], indices = <int>[];
    for (final (y, r) in profile) {
      for (var s = 0; s < sides; s++) {
        final a = 2 * math.pi * s / sides;
        positions.add(Vec3(r * math.cos(a), y, r * math.sin(a)));
      }
    }
    for (var ring = 0; ring < profile.length - 1; ring++) {
      for (var s = 0; s < sides; s++) {
        final a = ring * sides + s, b = ring * sides + (s + 1) % sides;
        indices.addAll([a, b + sides, b, a, a + sides, b + sides]);
      }
    }
    return Mesh.custom(positions: positions, indices: indices);
  }();

  /// Knocked down: tipped past 45°, or off the deck.
  bool get down {
    final up = rotation.rotate(Vec3.up);
    return up.y < math.cos(math.pi / 4) || position.y < -0.05 || position.x.abs() > laneWidth / 2;
  }
}

/// Behind the foul line, looking down the lane; it goes after the ball
/// while it rolls, and stops short of the pins.
class ChaseCamera extends GameObject {
  ChaseCamera(this.game)
    : super(
        name: 'camera',
        components: [Camera(fovY: 45.deg, bloom: const Bloom())],
      );

  final BowlingGame game;
  static final home = Vec3(0, 1.3, 2.8);
  var _z = home.z;

  @override
  void onUpdate() {
    super.onUpdate();
    final ball = game.ball;
    final goal = ball == null || ball.isDestroyed ? home.z : math.max(ball.position.z + 2.6, -11.0);
    // Closes most of the gap in about half a second.
    _z += (goal - _z) * (1 - math.exp(-6 * time.delta));
    final at = Vec3(0, home.y - (home.z - _z) * 0.02, _z);
    transform = Transform(position: at, rotation: Quat.lookRotation(Vec3(0, -1.2, -headPin) - at));
  }
}
