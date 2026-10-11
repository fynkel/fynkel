// The scene contacts_alloc_test.dart and contacts_fixed_alloc_test.dart
// count: bodies kept awake on a floor, in a trigger, get a collision and a
// trigger stay each step.

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

/// A floor and a trigger over it, with 20 boxes kept awake on the floor
/// and in the trigger; with [listen], each box and the floor listen.
(World, List<Listener>) scene({required bool betweenFrames, required bool listen}) {
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
          if (listen) Listener(),
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
    if (listen) listeners.add(listener);
    world.add(
      GameObject(
        transform: Transform(position: Vec3(i * 2.0, 0.5, 0)),
        components: [BoxCollider(), Rigidbody(), if (listen) listener],
      ),
    );
  }
  return (world, listeners);
}
