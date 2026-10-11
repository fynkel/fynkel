// Stage 5 step 5: triggers and collisions, the basics. A ball falls
// through a yellow trigger, which lights up while the ball is in it, and
// lands on the floor; the log says what each heard.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class ContactsBasicsScene extends LabScene {
  @override
  String get title => 'Triggers and collisions: the basics';
  @override
  String get step => 'stage 5 step 5';
  @override
  String get about =>
      'The yellow slab is a trigger: the ball falls through it, and it lights up while the ball is in it (the slab '
      'is "with Triggers"). The ball\'s Bounce behaviour is "with Collisions": it hears the floor. Turn the trigger '
      'off and the slab catches the ball instead.';

  late final GameObject ball;
  late final Zone zone;

  @override
  void build() {
    add(floorCollider());
    zone = add(Zone(log));
    ball = add(
      shape('ball', Mesh.sphere(), const Color(0xFF3A8EE0), components: [SphereCollider(), Rigidbody(), Bounce(log)]),
    );
    drop();
  }

  void drop() {
    ball.transform = Transform(position: Vec3(0, 5, 0));
    ball.get<Rigidbody>()!.linearVelocity = Vec3.zero;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    readout.value = [
      'the ball is ${ball.position.y.toStringAsFixed(2)} m up',
      'in the slab: ${zone.inside.isEmpty ? 'nothing' : zone.inside.join(', ')}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('drop the ball', () {
      drop();
      log.add('dropped');
    }),
    Toggle('the slab is a trigger', () => zone.collider.isTrigger, (on) => zone.collider.isTrigger = on),
  ];
}

/// A yellow slab that's a trigger, green while something is in it.
class Zone extends GameObject with Triggers {
  Zone(this.log)
    : super(
        name: 'slab',
        transform: Transform(position: Vec3(0, 2, 0), scale: Vec3(3, 0.2, 3)),
        components: [
          MeshRenderer(Mesh.cube(), Material(color: _empty)),
          BoxCollider(isTrigger: true),
        ],
      );

  static const _empty = Color(0xFFE0C23A);
  static const _full = Color(0xFF4CC26B);

  final Log log;
  final inside = <String>[];

  BoxCollider get collider => get<BoxCollider>()!;
  Material get _material => get<MeshRenderer>()!.material!;

  @override
  void onTriggerEnter(Collider other) {
    inside.add(other.gameObject.name ?? '?');
    _material.color = _full;
    log.add('slab: onTriggerEnter(${other.gameObject.name})');
  }

  @override
  void onTriggerExit(Collider other) {
    inside.remove(other.gameObject.name ?? '?');
    if (inside.isEmpty) _material.color = _empty;
    log.add('slab: onTriggerExit(${other.gameObject.name})');
  }
}

/// Says what the ball hits, and how hard.
class Bounce extends Behaviour with Collisions {
  Bounce(this.log);

  final Log log;

  @override
  void onCollisionEnter(Collision collision) {
    final speed = collision.relativeVelocity.length.toStringAsFixed(1);
    log.add('ball: onCollisionEnter(${collision.gameObject.name}) at $speed m/s');
  }

  @override
  void onCollisionExit(Collision collision) => log.add('ball: onCollisionExit(${collision.gameObject.name})');
}
