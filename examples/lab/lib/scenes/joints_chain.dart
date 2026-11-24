// Stage 5 step 8: a chain of light links, each on a ball joint (a
// CharacterJoint with its limits freed) to the next, with a heavy weight at
// its end. Drag the weight (or any link) around: a raycast
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
      'Twelve links of 0.2 kg, each on a ball joint to the one above (a CharacterJoint with its limits at 180°, '
      'so it swings and twists every way), and a weight at the end. Drag the weight or a link: it hangs on a '
      'SpringJoint from '
      'an unseen kinematic hand that follows your finger. Make the weight heavier and watch the chain stretch '
      'between its links; more solver iterations hold it tighter. It steps 120 times a second, not 50, so it '
      'holds together while it is dragged.';

  @override
  Vec3 get eye => Vec3(0, 3.2, 8);
  @override
  Vec3 get target => Vec3(0, 2.4, 0);

  late final Camera camera = find('camera')!.get<Camera>()!;
  final links = <GameObject>[];
  GameObject? weight;

  /// What the finger holds: the body, and the spring tying it to the hand.
  late final Rigidbody hand;
  (Rigidbody, SpringJoint)? held;
  Plane? _dragPlane;

  /// The weight's mass, in kilograms: 7.5 times a link's, within the 10
  /// times Unity advises.
  var mass = 1.5;

  static const count = 12, linkLength = 0.3, linkMass = 0.2, radius = 0.25;
  static final hook = Vec3(0, 5, 0);

  @override
  void build() {
    // 120 steps a second, not 50: a chain of joints pulls apart a little at
    // each joint while it's dragged, and more the longer a step is. At 50
    // the links part by up to 6 cm each, and snap back; at 120, about 1.
    time.fixedDelta = 1 / 120;
    add(floorCollider());
    add(shape('hook', Mesh.cube(), const Color(0xFF7A8290), at: hook + Vec3(0, 0.1, 0), size: Vec3(0.6, 0.2, 0.6)));
    hand = addWith(Rigidbody(isKinematic: true), name: 'hand');
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
              // Drawn between physics' steps, so they glide, not jump.
              Rigidbody(mass: linkMass, interpolation: RigidbodyInterpolation.interpolate),
              _ball(links.isEmpty ? null : links.last.get<Rigidbody>()),
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
          Rigidbody(mass: mass, interpolation: RigidbodyInterpolation.interpolate),
          _ball(links.last.get<Rigidbody>()),
        ],
      ),
    );
  }

  /// A ball joint at the top of its object, to [above] (or the world).
  static CharacterJoint _ball(Rigidbody? above) => CharacterJoint(
    connectedBody: above,
    anchor: Vec3(0, 0.5, 0),
    axis: Vec3(0, 1, 0),
    swingAxis: Vec3(1, 0, 0),
    lowTwistLimit: SoftJointLimit(limit: -180.deg),
    highTwistLimit: SoftJointLimit(limit: 180.deg),
    swing1Limit: SoftJointLimit(limit: 180.deg),
    swing2Limit: SoftJointLimit(limit: 180.deg),
  );

  @override
  void onStart() {
    super.onStart();
    on(input.onFingerDown, (finger) => grab(finger.position));
    on(input.onFingerMove, (finger) => drag(finger.position));
    on(input.onFingerUp, (_) => let());
  }

  /// Takes hold of the body under [point], where the ray meets it.
  void grab(ViewPoint point) {
    final hit = physics.raycast(camera.screenPointToRay(point));
    final body = hit?.collider.attachedRigidbody;
    if (hit == null || body == null || body.isKinematic) return;
    let();
    hand.gameObject.position = hit.point;
    // Dragged across the plane facing the camera, through where it was hit.
    _dragPlane = Plane(camera.gameObject.forward, hit.point);
    // A spring that swings what it holds 5 times a second, as Box2D's mouse
    // joint does: stiff for what it lifts, the weight and the chain, so it
    // follows within a few centimetres, and damped to 0.7 of critical, so
    // it doesn't bounce.
    final lifted = mass + count * linkMass, w = 2 * math.pi * 5;
    final spring = SpringJoint(
      connectedBody: hand,
      anchor: body.gameObject.inverseTransformPoint(hit.point),
      connectedAnchor: Vec3.zero,
      spring: lifted * w * w,
      damper: 2 * 0.7 * lifted * w,
    );
    body.gameObject.addComponent(spring);
    held = (body, spring);
    log.add('took hold of the ${body.gameObject.name}');
  }

  /// Moves the hand to where [point] is on the drag plane.
  void drag(ViewPoint point) {
    final plane = _dragPlane;
    if (held == null || plane == null) return;
    final at = camera.screenPointToPlane(point, plane);
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
      '${physics.defaultSolverIterations} solver iterations, ${(1 / time.fixedDelta).round()} steps a second',
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
      (v) {
        physics.defaultSolverIterations = v.round();
        // A chain hanging still is asleep, and sleeps on as it is: wake it
        // to hang by the new number.
        for (final o in [...links, ?weight]) {
          o.get<Rigidbody>()!.wakeUp();
        }
      },
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
    // The step and the solver's iterations are the world's: put them back
    // for the next scene.
    time.fixedDelta = 1 / 50;
    physics.defaultSolverIterations = 6;
    super.onDestroy();
  }
}
