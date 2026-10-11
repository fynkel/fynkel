// Stage 5 step 3: physics queries, the basics. A cube, a ball and a pill
// with colliders, and a ray (or a sphere) sweeping across them.

import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

/// The ball's layer, when "ball on layer 1" is on.
const ballLayer = 1;

class PhysicsQueriesBasicsScene extends LabScene {
  @override
  String get title => 'Physics queries: the basics';
  @override
  String get step => 'stage 5 step 3';
  @override
  String get about =>
      'The cube, the ball and the pill each have a collider (drawn in green; yellow for a trigger). The yellow line '
      'is physics.raycast, sweeping across them: the dot is where it hits, the short line the normal there. Try a '
      'sphere cast, a trigger, a layer the ray skips, and moving the cube: the ray finds it where it went at once.';

  late final GameObject cube, ball, pill;
  var sphereCast = false;
  var rayHitsBall = true;

  static const _yellow = Color(0xFFFFE000);

  @override
  void build() {
    cube = add(shape('cube', Mesh.cube(), const Color(0xFFE0613A), at: Vec3(-2, 0.5, 0), components: [BoxCollider()]));
    ball = add(
      shape('ball', Mesh.sphere(), const Color(0xFF3A8EE0), at: Vec3(0, 0.5, 0), components: [SphereCollider()]),
    );
    // No capsule mesh: a stretched sphere shows where the pill is.
    pill = add(
      GameObject(
        name: 'pill',
        transform: Transform(position: Vec3(2, 1, 0)),
        components: [CapsuleCollider(radius: 0.5, height: 2)],
        children: [shape('pill mesh', Mesh.sphere(), const Color(0xFF4CC26B), size: Vec3(1, 2, 1))],
      ),
    );
  }

  @override
  void onStart() {
    super.onStart();
    physics.drawColliders = true;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    // Back and forth across the three, 4 m in front of them.
    final x = 3.5 * math.sin(time.sinceStart * 0.8);
    final ray = Ray(Vec3(x, 0.5, 4), Vec3(0, 0, -1));
    final layers = rayHitsBall ? LayerMask.all : LayerMask.except([ballLayer]);
    final hit = sphereCast
        ? physics.sphereCast(ray, 0.3, maxDistance: 8, layers: layers)
        : physics.raycast(ray, maxDistance: 8, layers: layers);
    final end = hit == null ? ray.at(8) : ray.at(hit.distance);
    debug.line(ray.origin, end, color: _yellow);
    if (sphereCast) debug.sphere(end, 0.3, color: _yellow);
    if (hit != null) {
      debug.sphere(hit.point, 0.06, color: _yellow);
      debug.ray(hit.point, hit.normal * 0.5, color: _yellow);
    }
    final near = physics.overlapSphere(Vec3(0, 0.5, 0), 2.2);
    readout.value = [
      if (hit == null)
        'it hits nothing'
      else
        'it hits the ${hit.gameObject.name} at ${hit.distance.toStringAsFixed(2)} m',
      'within 2.2 m of the ball: ${near.map((c) => c.gameObject.name).join(', ')}',
      'colliders: ${physics.colliderCount}',
    ].join('\n');
  }

  @override
  List<Control> get controls => [
    Toggle('sphere cast', () => sphereCast, (on) => sphereCast = on),
    Toggle('draw colliders', () => physics.drawColliders, (on) => physics.drawColliders = on),
    Toggle('cube is a trigger', () => cube.get<BoxCollider>()!.isTrigger, (on) {
      cube.get<BoxCollider>()!.isTrigger = on;
    }),
    Toggle('queries hit triggers', () => physics.queriesHitTriggers, (on) => physics.queriesHitTriggers = on),
    Toggle('ball on layer 1, the ray skips it', () => !rayHitsBall, (on) {
      rayHitsBall = !on;
      ball.layer = on ? ballLayer : 0;
    }),
    Button('move the cube', () {
      final up = cube.transform.position.y < 1;
      cube.position = Vec3(-2, up ? 2 : 0.5, 0);
      log.add(up ? 'the cube is up, out of the ray' : 'the cube is back');
    }),
  ];
}
