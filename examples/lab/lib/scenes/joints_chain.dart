// Stage 5 step 8: a chain of light links, hinged each to the next, with a
// heavy weight at its end. Drag the weight (or any link) around: a raycast
// finds it, and a SpringJoint ties it to an unseen kinematic hand that
// follows the finger, as Unity's DragRigidbody does. The heavier the
// weight is than a link, the more a chain of joints stretches.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class JointsChainScene extends LabScene {
  @override
  String get title => 'Joints: a chain and a weight';
  @override
  String get step => 'stage 5 step 8';
  @override
  String get about =>
      'Twelve links of 0.2 kg, each on a HingeJoint to the one above, every other one turned a quarter turn so the '
      'chain swings every way, and a weight at the end. Drag the weight or a link: it hangs on a SpringJoint from '
      'an unseen kinematic hand that follows your finger. Make the weight heavier and watch the chain stretch '
      'between its links; more solver iterations hold it tighter.';

  @override
  Vec3 get eye => Vec3(0, 3.2, 8);
  @override
  Vec3 get target => Vec3(0, 2.4, 0);

  late final GameObject camera = find('camera')!;
  final links = <GameObject>[];
  GameObject? weight;

  /// What the finger holds: the body, and the spring tying it to the hand.
  late final Rigidbody hand;
  (Rigidbody, SpringJoint)? held;
  Plane? _dragPlane;

  /// The weight's mass, in kilograms.
  var mass = 5.0;

  static const count = 12, linkLength = 0.3, linkMass = 0.2, radius = 0.25;
  static final hook = Vec3(0, 5, 0);

  @override
  void build() {
    add(floorCollider());
    add(shape('hook', Mesh.cube(), const Color(0xFF7A8290), at: hook + Vec3(0, 0.1, 0), size: Vec3(0.6, 0.2, 0.6)));
    hand = add(GameObject(name: 'hand', components: [Rigidbody(isKinematic: true)])).get<Rigidbody>()!;
    hang();
  }

  /// Takes the chain away, and hangs a new one, straight down.
  void hang() {
    let();
    for (final o in [...links, ?weight]) {
      o.destroy();
    }
    links.clear();
    for (var i = 0; i < count; i++) {
      links.add(
        add(
          shape(
            'link ${i + 1}',
            Mesh.cube(),
            i.isEven ? const Color(0xFFB0B8C8) : const Color(0xFF8A90A0),
            at: hook - Vec3(0, (i + 0.5) * linkLength, 0),
            size: Vec3(0.08, linkLength, 0.08),
            components: [
              BoxCollider(),
              Rigidbody(mass: linkMass),
              HingeJoint(
                connectedBody: links.isEmpty ? null : links.last.get<Rigidbody>(),
                anchor: Vec3(0, 0.5, 0),
                axis: _axis(i),
              ),
            ],
          ),
        ),
      );
    }
    weight = add(
      shape(
        'weight',
        Mesh.sphere(),
        const Color(0xFFE0A030),
        at: hook - Vec3(0, count * linkLength + radius, 0),
        size: Vec3.one * (radius * 2),
        components: [
          SphereCollider(),
          Rigidbody(mass: mass),
          HingeJoint(connectedBody: links.last.get<Rigidbody>(), anchor: Vec3(0, 0.5, 0), axis: _axis(count)),
        ],
      ),
    );
  }

  /// Every other link turns about X, the others about Z: together, every way.
  static Vec3 _axis(int i) => i.isEven ? Vec3(0, 0, 1) : Vec3(1, 0, 0);

  @override
  void onStart() {
    super.onStart();
    on(input.onFingerDown, (finger) => grab(finger.position));
    on(input.onFingerMove, (finger) => drag(finger.position));
    on(input.onFingerUp, (_) => let());
  }

  /// Takes hold of the body under [point], where the ray meets it.
  void grab(ViewPoint point) {
    final hit = physics.raycast(camera.screenToRay(point));
    final body = hit?.collider.attachedRigidbody;
    if (hit == null || body == null || body.isKinematic) return;
    let();
    hand.gameObject.position = hit.point;
    // Dragged across the plane facing the camera, through where it was hit.
    _dragPlane = Plane(camera.forward, hit.point);
    // Stiff enough to lift the weight and the chain, and nearly critically
    // damped: it follows, not bounces.
    final lifted = mass + count * linkMass;
    final spring = SpringJoint(
      connectedBody: hand,
      anchor: body.gameObject.inverseTransformPoint(hit.point),
      connectedAnchor: Vec3.zero,
      spring: 120 * lifted,
      damper: 15 * lifted,
    );
    body.gameObject.addComponent(spring);
    held = (body, spring);
    log.add('took hold of the ${body.gameObject.name}');
  }

  /// Moves the hand to where [point] is on the drag plane.
  void drag(ViewPoint point) {
    final plane = _dragPlane;
    if (held == null || plane == null) return;
    final at = camera.screenToPlane(point, plane);
    if (at != null) hand.movePosition(at);
  }

  /// Lets go of what's held.
  void let() {
    final (body, spring) = held ?? (null, null);
    held = null;
    _dragPlane = null;
    if (body == null || spring == null || body.gameObject.isDestroyed) return;
    body.gameObject.removeComponent(spring);
    log.add('let go of the ${body.gameObject.name}');
  }

  /// How far apart the joints have pulled, in all: between the hook and the
  /// first link, each link and the next, and the last and the weight.
  ({double total, double most}) get stretch {
    var total = 0.0, most = 0.0;
    var above = hook;
    for (final o in [...links, ?weight]) {
      final gap = (o.transformPoint(Vec3(0, 0.5, 0)) - above).length;
      total += gap;
      most = math.max(most, gap);
      above = o.transformPoint(Vec3(0, -0.5, 0));
    }
    return (total: total, most: most);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final s = stretch;
    readout.value = [
      'the weight is ${mass.toStringAsFixed(1)} kg, ${(mass / linkMass).round()}× a link',
      'the chain has stretched ${(s.total * 100).toStringAsFixed(1)} cm, '
          'at most ${(s.most * 100).toStringAsFixed(1)} cm at a joint',
      '${physics.defaultSolverIterations} solver iterations',
      held == null ? 'drag the weight or a link' : 'holding the ${held!.$1.gameObject.name}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Button('swing it', () {
      weight?.get<Rigidbody>()!.addForce(Vec3(4, 0, -2) * mass, mode: ForceMode.impulse);
      log.add('swung the weight');
    }),
    Slider(
      'the weight (kg)',
      () => mass,
      (v) {
        mass = (v * 2).roundToDouble() / 2;
        weight?.get<Rigidbody>()!.mass = mass;
      },
      min: 0.5,
      max: 50,
    ),
    Slider(
      'solver iterations',
      () => physics.defaultSolverIterations.toDouble(),
      (v) => physics.defaultSolverIterations = v.round(),
      min: 1,
      max: 30,
    ),
    Button('hang it again', () {
      hang();
      log.add('hung it again');
    }),
  ];

  @override
  void onDestroy() {
    // The solver's iterations are the world's: put them back for the next scene.
    physics.defaultSolverIterations = 6;
    super.onDestroy();
  }
}
