// Stage 5 step 7: the character controller, the basics. A player with a
// CharacterController walks where you tap, or with the keys: up the low
// step, not up the high one, and it pushes the crate.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class CharacterBasicsScene extends LabScene {
  @override
  String get title => 'Character controller: the basics';
  @override
  String get step => 'stage 5 step 7';
  @override
  String get about =>
      'The player has a CharacterController, and each frame its Walker calls move(). Tap the floor to walk there, '
      'or use WASD or the arrows, and space to jump. It walks up the low step (under its stepOffset, 0.3 m) but '
      'not the high one, and pushes the crate.';

  late final Player player;
  late final GameObject crate;
  late final Camera camera = find('camera')!.get<Camera>()!;

  @override
  void build() {
    add(floorCollider());
    add(block('low step, 0.25 m', Vec3(-2.5, 0, -1), Vec3(2.5, 0.25, 2)));
    add(block('high step, 0.6 m', Vec3(2.5, 0, -1), Vec3(2.5, 0.6, 2)));
    crate = add(
      shape(
        'crate',
        Mesh.cube(),
        const Color(0xFFC08A4A),
        size: Vec3(0.8, 0.8, 0.8),
        components: [BoxCollider(), Rigidbody(mass: 2)],
      ),
    );
    player = add(Player());
    reset();
  }

  void reset() {
    player.walker.target = null;
    player.transform = Transform(position: Vec3(0, 1.08, 3.5));
    crate.transform = Transform(position: Vec3(0, 0.4, 1.5), scale: Vec3(0.8, 0.8, 0.8));
    crate.get<Rigidbody>()!
      ..linearVelocity = Vec3.zero
      ..angularVelocity = Vec3.zero;
  }

  @override
  void onStart() {
    super.onStart();
    on(input.onTap, (finger) {
      final hit = physics.raycast(camera.screenPointToRay(finger.position));
      if (hit == null) return;
      player.walker.target = hit.point;
      log.add('walking to ${hit.collider.gameObject.name}');
    });
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final c = player.controller;
    final flags = [
      for (final (flag, name) in [
        (CollisionFlags.below, 'below'),
        (CollisionFlags.sides, 'sides'),
        (CollisionFlags.above, 'above'),
      ])
        if (c.collisionFlags.has(flag)) name,
    ];
    readout.value = [
      c.isGrounded ? 'grounded' : 'in the air',
      'hit: ${flags.isEmpty ? 'nothing' : flags.join(', ')}',
      'going ${c.velocity.length.toStringAsFixed(2)} m/s',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('back to the start', () {
      reset();
      log.add('back to the start');
    }),
    Toggle('draw colliders', () => physics.drawColliders, (on) => physics.drawColliders = on),
  ];
}

/// A grey block [size] across, standing on the floor at [at], solid.
GameObject block(String name, Vec3 at, Vec3 size, {Quat rotation = Quat.identity}) => GameObject(
  name: name,
  transform: Transform(position: at + Vec3(0, size.y / 2, 0), rotation: rotation, scale: size),
  components: [
    MeshRenderer(Mesh.cube(), Material(color: const Color(0xFF7A8290))),
    BoxCollider(),
  ],
);

/// A player: a CharacterController 2 m tall, and a [Walker] to move it.
class Player extends GameObject {
  Player() : super(name: 'player', components: [CharacterController(), Walker()]) {
    add(
      GameObject(
        name: 'body',
        transform: Transform(scale: Vec3(1, 2, 1)),
        components: [MeshRenderer(Mesh.sphere(), Material(color: const Color(0xFF3A8EE0)))],
      ),
    );
    // Its nose, to see which way it faces.
    add(
      GameObject(
        name: 'nose',
        transform: Transform(position: Vec3(0, 0.5, -0.5), scale: Vec3(0.2, 0.2, 0.3)),
        components: [MeshRenderer(Mesh.cube(), Material(color: const Color(0xFFFFD040)))],
      ),
    );
  }

  CharacterController get controller => get<CharacterController>()!;
  Walker get walker => get<Walker>()!;
}

/// Walks its object's CharacterController with the 'move' axis, or to
/// [target], and jumps with 'jump': gravity is its own to add, as in
/// Unity.
class Walker extends Behaviour {
  /// Where to walk to, if anywhere.
  Vec3? target;

  double speed = 3;
  double jumpSpeed = 5;
  var _fall = 0.0;

  @override
  void onUpdate() {
    super.onUpdate();
    final controller = get<CharacterController>()!;
    final keys = input.axis2('move');
    // Up on the axis is away from the camera: -Z.
    var direction = Vec3(keys.x, 0, -keys.y);
    if (direction.length > 0.1) {
      target = null;
    } else if (target case final target?) {
      final to = Vec3(target.x - position.x, 0, target.z - position.z);
      if (to.length < 0.1) {
        this.target = null;
      } else {
        direction = to.normalized;
      }
    }
    if (controller.isGrounded) {
      _fall = -1;
      if (input.action('jump').wasPressed) _fall = jumpSpeed;
    }
    _fall -= 9.81 * time.delta;
    final hit = controller.move(direction * speed * time.delta + Vec3(0, _fall * time.delta, 0));
    // A head hitting the ceiling stops going up.
    if (hit.has(CollisionFlags.above) && _fall > 0) _fall = 0;
    if (direction.length > 0.1) rotation = Quat.lookRotation(direction);
  }
}
