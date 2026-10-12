// Stage 5 step 6: mesh colliders, the basics. A ball drops onto a rock (a
// model, Kenney's, CC0) and rolls over its real shape, or its hull.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

import '../lab.dart';

class MeshCollidersBasicsScene extends LabScene {
  @override
  String get title => 'Mesh colliders: the basics';
  @override
  String get step => 'stage 5 step 6';
  @override
  String get about =>
      'The rock is a model. MeshCollider.addToParts gave each of its parts a MeshCollider made of its '
      'triangles, so the ball lands on the rock\'s slope and rolls down its real shape. Make them convex and the '
      'ball rolls over the rock\'s hull instead; draw the colliders to see which.';

  late final Rock rock;
  late final GameObject ball;

  @override
  void build() {
    add(floorCollider());
    rock = add(Rock());
    ball = add(
      shape(
        'ball',
        Mesh.sphere(),
        const Color(0xFF3A8EE0),
        size: Vec3(0.6, 0.6, 0.6),
        // Damped, so it rolls to a stop on the floor.
        components: [SphereCollider(), Rigidbody(angularDamping: 3)],
      ),
    );
    drop();
  }

  void drop() {
    ball.transform = Transform(position: Vec3(0.5, 4, 2.2), scale: Vec3(0.6, 0.6, 0.6));
    ball.get<Rigidbody>()!
      ..linearVelocity = Vec3.zero
      ..angularVelocity = Vec3.zero;
  }

  @override
  void onUpdate() {
    super.onUpdate();
    final below = physics.raycast(Ray(ball.position, Vec3(0, -1, 0)));
    final on = below == null ? 'nothing' : (below.collider is MeshCollider ? 'the rock' : 'the floor');
    readout.value = 'the ball is ${ball.position.y.toStringAsFixed(2)} m up, over $on';
  }

  @override
  List<Control> get controls => [
    Button('drop the ball', () {
      drop();
      log.add('dropped');
    }),
    Toggle('convex: the rock\'s hull', () => rock.convex, (on) {
      rock.convex = on;
      log.add(on ? 'the rock is its hull' : 'the rock is its triangles');
    }),
    Toggle('draw colliders', () => physics.drawColliders, (on) => physics.drawColliders = on),
  ];
}

/// A rock from a model, with a MeshCollider on each of its parts.
class Rock extends GameObject {
  Rock()
    : super(
        name: 'rock',
        transform: Transform(scale: Vec3(6, 6, 6)),
      );

  var colliders = <MeshCollider>[];

  /// Whether its colliders are its hull; kept here, as the model loads
  /// after the page first asks.
  bool get convex => _convex;
  set convex(bool value) {
    _convex = value;
    for (final c in colliders) {
      c.convex = value;
    }
  }

  var _convex = false;

  @override
  Future<void> onLoad() async {
    await super.onLoad();
    final model = add(await assets.instantiate('rock.glb'));
    colliders = MeshCollider.addToParts(model, convex: _convex);
  }
}
