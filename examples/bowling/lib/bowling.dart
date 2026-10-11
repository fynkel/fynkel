// The game: ten-pin bowling on a lane of real size. Swipe up to roll the
// ball: where the swipe starts is where on the lane it starts, and how
// fast and which way it goes, the ball. Ten frames, scored as bowling is.

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

class BowlingGame extends GameObject {
  final score = Score();

  /// What the HUD says under the score.
  final message = ValueNotifier('Swipe up to roll');

  Ball? ball;
  final _pins = <Pin>[];
  var _rolling = false;
  var _ballDone = false;

  /// Whether a roll is under way, from the swipe to the pins counted.
  bool get rolling => _rolling;

  /// The pins standing on the deck.
  Iterable<Pin> get standing => _pins.where((p) => !p.isDestroyed && !p.down);

  @override
  void onLoad() {
    super.onLoad();
    // 100 steps a second: a ball rolling at 9 m/s turns 80 times a second,
    // and physics turns a body no more than a quarter turn a step.
    time.fixedDelta = 1 / 100;
    add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(0.3, -1, -0.5))),
        components: [Light.sun(intensity: 2.5)],
      ),
    );
    add(GameObject(components: [Light.ambient(color: Color.rgb(0.75, 0.8, 1), intensity: 0.7)]));
    add(Alley());
    add(ChaseCamera(this));
    rack();
  }

  @override
  void onStart() {
    super.onStart();
    on(input.onSwipe, (swipe) {
      // Up the view, and not too far sideways.
      if (swipe.direction.y > -0.5) return;
      final start = physics.raycast(
        world.camera!.screenToRay(swipe.finger.startPosition),
        layers: LayerMask.of([lane]),
      );
      final speed = (swipe.speed / 250).clamp(3.0, 9.0);
      roll(x: start?.point.x ?? 0, speed: speed, angle: math.atan2(swipe.direction.x, -swipe.direction.y) * 0.15);
    });
    on(input.onKeyDown, (press) {
      if (press.key == KeyCode.space) roll(x: 0, speed: 7.5, angle: 0);
    });
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

  /// Rolls a ball from [x] on the foul line, at [speed] metres a second,
  /// [angle] radians to the right of straight down the lane. Then, once it's
  /// gone and the pins have settled, counts them.
  void roll({required double x, required double speed, required double angle}) {
    if (_rolling) return;
    if (score.isOver) {
      score.clear();
      rack();
    }
    _rolling = true;
    _ballDone = false;
    message.value = '';
    final at = Vec3(x.clamp(-laneWidth / 2 + Ball.radius, laneWidth / 2 - Ball.radius), Ball.radius, -0.3);
    final velocity = Vec3(math.sin(angle), 0, -math.cos(angle)) * speed;
    ball = add(Ball(at, velocity));
    start(() async {
      final rolled = time.sinceStart;
      await until(() => _ballDone || time.sinceStart - rolled > 8);
      // The pins fall for a while after the ball has gone.
      await wait(2);
      _count();
    });
  }

  /// The ball went into the pit, or a gutter.
  void ballDone() => _ballDone = true;

  void gutter() => message.value = 'Gutter';

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
      message.value = 'Game over: ${score.total}. Swipe to play again';
    } else {
      message.value = switch (score.lastMark) {
        'X' => 'Strike!',
        '/' => 'Spare!',
        _ when message.value.isNotEmpty => message.value,
        _ => '${down.length} down',
      };
      // A new frame, or all ten down in the tenth: a new rack.
      if (score.frame != frame || standing.isEmpty) rack();
    }
    _rolling = false;
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
          MeshRenderer(Mesh.sphere(), Material(color: Color.rgb(0.15, 0.3, 0.9), roughness: 0.15)),
          SphereCollider(),
          // A rolling ball turns at speed / radius: 80 rad/s at 9 m/s, past
          // the 50 a rigidbody may turn by default.
          Rigidbody(
            mass: 6.5,
            angularDamping: 0,
            maxAngularVelocity: 100,
            collisionDetectionMode: CollisionDetectionMode.continuous,
          ),
        ],
      ) {
    _velocity = velocity;
  }

  static const radius = 0.109;
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
  ChaseCamera(this.game) : super(name: 'camera', components: [Camera(fovY: 45)]);

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
