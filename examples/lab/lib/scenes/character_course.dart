// Stage 5 step 7: a small course for the character controller. Stairs, a
// step too high, a gentle ramp and a steep one, a wall to slide along,
// crates to push and coins to pick up (triggers), with sliders for the
// controller's slope limit and step offset.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';
import 'character_basics.dart';

class CharacterCourseScene extends LabScene {
  @override
  String get title => 'Character controller: a course';
  @override
  String get step => 'stage 5 step 7';
  @override
  String get about =>
      'Walk the course: tap where to go, or WASD and space. Up the stairs (0.2 m steps) but not the 0.5 m step; '
      'up the 25° ramp but not the 55° one (its slopeLimit is 45°); along the wall; push the crates; pick up the '
      'coins, triggers it meets as a kinematic rigidbody would. Change the slope limit and step offset to see '
      'what it can climb.';

  @override
  Vec3 get eye => Vec3(0, 9, 11);
  @override
  Vec3 get target => Vec3(0, 0, 0);

  late final Player player;
  late final Camera camera = find('camera')!.get<Camera>()!;
  final _coins = <GameObject>[];
  var _picked = 0;

  @override
  void build() {
    add(floorCollider());
    // Stairs up to a landing, on the left.
    for (var i = 0; i < 4; i++) {
      add(block('stair ${i + 1}', Vec3(-5, 0, -1.4 - i * 0.6), Vec3(2, 0.2 * (i + 1), 0.6)));
    }
    add(block('landing', Vec3(-5, 0, -4.4), Vec3(2, 0.8, 1.6)));
    add(block('a 0.5 m step', Vec3(-2, 0, -2.5), Vec3(1.5, 0.5, 3)));
    add(ramp('a 25° ramp', 1.2, 25));
    add(ramp('a 55° ramp', 3.4, 55));
    add(block('wall', Vec3(5.5, 0, -0.5), Vec3(0.4, 1.6, 6), rotation: Quat.axisAngle(Vec3.up, 0.3)));
    for (final (i, at) in [Vec3(-1.5, 0.35, 2), Vec3(1.5, 0.35, 2.2), Vec3(3, 0.35, 1)].indexed) {
      add(
        shape(
          'crate ${i + 1}',
          Mesh.cube(),
          const Color(0xFFC08A4A),
          at: at,
          size: Vec3(0.7, 0.7, 0.7),
          components: [BoxCollider(), Rigidbody()],
        ),
      );
    }
    for (final at in [Vec3(-5, 1.6, -4.4), Vec3(-2, 1.0, -2.5), Vec3(4.5, 0.8, 3), Vec3(0, 0.8, 4)]) {
      _coins.add(add(Coin(at, onPicked: _pick)));
    }
    player = add(Player());
    player.transform = Transform(position: Vec3(0, 1.08, 3));
  }

  /// A ramp rising [degrees] towards -Z from z = 0, at [x], 1.5 m wide, 4 m long.
  GameObject ramp(String name, double x, double degrees) {
    final slope = degrees * math.pi / 180;
    const length = 4.0, thick = 0.4;
    // Along -Z, rising: its top face's middle is half its length up the slope.
    final along = Vec3(0, math.sin(slope), -math.cos(slope));
    final down = Vec3(0, -math.cos(slope), -math.sin(slope));
    final middle = Vec3(x, 0, 0) + along * (length / 2) + down * (thick / 2);
    return GameObject(
      name: name,
      transform: Transform(
        position: middle,
        rotation: Quat.axisAngle(Vec3(1, 0, 0), slope),
        scale: Vec3(1.5, thick, length),
      ),
      components: [
        MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF8A90A0))),
        BoxCollider(),
      ],
    );
  }

  void _pick(Coin coin) {
    _picked++;
    log.add('picked up a coin: $_picked of ${_coins.length}');
  }

  @override
  void onStart() {
    super.onStart();
    on(input.onTap, (finger) {
      final hit = physics.raycast(camera.screenPointToRay(finger.position), triggers: QueryTriggers.ignore);
      if (hit == null) return;
      player.walker.target = hit.point;
      log.add('walking to ${hit.collider.gameObject.name}');
    });
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final c = player.controller;
    readout.value = [
      '${c.isGrounded ? 'grounded' : 'in the air'}, ${(player.position.y - 1.08).toStringAsFixed(2)} m up',
      'coins: $_picked of ${_coins.length}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Slider(
      'slope limit (°)',
      () => player.controller.slopeLimit.inDegrees,
      (v) => player.controller.slopeLimit = v.deg,
      max: 80,
    ),
    Slider('step offset (m)', () => player.controller.stepOffset, (v) => player.controller.stepOffset = v, max: 0.8),
    Button('back to the start', () {
      player.walker.target = null;
      player.transform = Transform(position: Vec3(0, 1.08, 3));
      for (final coin in _coins) {
        coin.active = true;
      }
      _picked = 0;
      log.add('back to the start');
    }),
    Toggle('draw colliders', () => physics.drawColliders, (on) => physics.drawColliders = on),
  ];
}

/// A coin to pick up: a trigger, hidden when a character walks into it.
class Coin extends GameObject with Triggers {
  Coin(Vec3 at, {required this.onPicked})
    : super(
        name: 'coin',
        transform: Transform(position: at, scale: Vec3(0.4, 0.4, 0.1)),
        components: [
          MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFFFFD040))),
          SphereCollider(isTrigger: true),
        ],
      );

  final void Function(Coin coin) onPicked;

  @override
  void onUpdate() {
    super.onUpdate();
    rotate(Vec3.up, 2 * time.delta);
  }

  @override
  void onTriggerEnter(Collider other) {
    if (other is! CharacterController) return;
    active = false;
    onPicked(this);
  }
}
