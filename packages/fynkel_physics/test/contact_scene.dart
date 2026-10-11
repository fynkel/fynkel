// The scene contacts_alloc_test.dart and contacts_fixed_alloc_test.dart
// count: bodies kept awake on a floor, in a trigger, get a collision and a
// trigger stay each step, standing still.

import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:test/test.dart';

final class Listener extends Behaviour with Triggers, Collisions {
  var stays = 0;

  @override
  void onTriggerStay(Collider other) => stays++;

  @override
  void onCollisionStay(Collision collision) => stays++;
}

/// A floor and a trigger over it, with 20 boxes on the floor and in the
/// trigger, each a listening rigidbody with every axis frozen and never
/// asleep: each step it gets a collision stay and a trigger stay, and,
/// standing still, isn't moved (a body that moves makes its new transform
/// each step, step 4's cost, not the contacts').
(World, List<Listener>) scene({required bool betweenFrames}) {
  final world = World();
  addTearDown(world.dispose);
  world.physics
    ..stepBetweenFrames = betweenFrames
    // Never asleep: a sleeping pair has no stay.
    ..sleepThreshold = 0;
  world
    ..add(
      GameObject(
        transform: Transform(position: const Vec3(0, -0.5, 0)),
        components: [
          BoxCollider(size: const Vec3(100, 1, 100)),
          Listener(),
        ],
      ),
    )
    ..add(
      GameObject(
        transform: Transform(position: const Vec3(0, 0.5, 0)),
        components: [BoxCollider(size: const Vec3(100, 2, 100), isTrigger: true)],
      ),
    );
  final listeners = <Listener>[];
  for (var i = 0; i < 20; i++) {
    final listener = Listener();
    listeners.add(listener);
    world.add(
      GameObject(
        // A little into the floor, so they touch.
        transform: Transform(position: Vec3(i * 2.0, 0.49, 0)),
        components: [
          BoxCollider(),
          Rigidbody(constraints: RigidbodyConstraints.freezeAll),
          listener,
        ],
      ),
    );
  }
  return (world, listeners);
}
