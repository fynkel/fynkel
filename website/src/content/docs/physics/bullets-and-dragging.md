---
title: "Bullets and dragging"
description: "Fast shots that never miss a thin wall, and bodies dragged by a finger."
---

**Bullets** can be done two ways. A bullet that's an object, with a small
trigger collider and a kinematic rigidbody moved with `movePosition`, hears
what it hits in `onTriggerEnter`, and is simple; it can pass through something
thinner than it moves in a step.

Many fast bullets are better as plain data in one manager, which casts a ray
along each one's move every frame and so never misses: no bodies, one query
each. On a Mac, 500 bullets among 100 targets cost about 0.17 ms a frame as
bodies and 0.05 ms as rays (a phone takes a few times longer). Draw either
kind from a `Pool` ([Pooling](/basics/pooling/)) so firing makes nothing new:

```dart
// lib/guns.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// Moves every bullet in one place, as rays: none can skip a thin wall.
class Bullets extends GameObject {
  final hit = Signal<RaycastHit>();
  final _at = <Vec3>[], _was = <Vec3>[], _velocity = <Vec3>[];

  void fire(Vec3 from, Vec3 velocity) {
    _at.add(from);
    _was.add(from);
    _velocity.add(velocity);
  }

  @override
  void onUpdate() {
    super.onUpdate();
    for (var i = _at.length - 1; i >= 0; i--) {
      final to = _at[i] + _velocity[i] * time.delta;
      final path = to - _was[i];
      final struck = physics.raycast(Ray(_was[i], path), maxDistance: path.length);
      if (struck != null || to.length > 100) {
        if (struck != null) hit.emit(struck);
        _at.removeAt(i);
        _was.removeAt(i);
        _velocity.removeAt(i);
      } else {
        _was[i] = _at[i];
        _at[i] = to;
      }
    }
  }
}
```

Each ray starts where the bullet was a frame ago, not where it is, so it
goes over the last move again. A ray doesn't hit what it starts inside:
started where the bullet is, it would miss a target that walked onto the
bullet since the last frame, and miss more often the longer the frames.
A test of just that:

```dart
// test/bullets_test.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';
import 'package:my_game/guns.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('a target that walks onto a bullet is hit', () {
    final world = World();
    addTearDown(world.dispose);
    final bullets = world.add(Bullets());
    final target = world.add(GameObject(components: [BoxCollider()]))
      ..position = Vec3(0, 0, 10);
    final hits = <GameObject>[];
    bullets.hit.listen((hit) => hits.add(hit.gameObject));
    bullets.fire(Vec3.zero, Vec3(0, 0, -30));
    world.update(1 / 60); // the bullet is at z = -0.5
    target.position = Vec3(0, 0, -0.7); // and now inside the target
    world.update(1 / 60);
    expect(hits, [target]);
  });
}
```

## Dragging a body

Dragging a body with a finger, as Unity's old `DragRigidbody`: a ray
finds the body, a `SpringJoint` ties it to a kinematic "hand", and the
hand follows the finger across a plane facing the camera. The spring pulls
the body along, so it still collides, swings and stays joined:

```dart
// lib/drag.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// Lets a finger pick up and carry any dynamic body.
class Dragger extends GameObject {
  Dragger(this.camera) : super(components: [Rigidbody(isKinematic: true)]);

  final Camera camera;
  late final Rigidbody hand = get<Rigidbody>()!;
  SpringJoint? _spring;
  Plane? _plane;

  @override
  void onStart() {
    super.onStart();
    on(input.onFingerDown, (finger) => _grab(finger.position));
    on(input.onFingerMove, (finger) => _drag(finger.position));
    on(input.onFingerUp, (_) => _let());
  }

  void _grab(ViewPoint point) {
    final hit = physics.raycast(camera.screenPointToRay(point));
    final body = hit?.collider.attachedRigidbody;
    if (hit == null || body == null || body.isKinematic) return;
    position = hit.point;
    _plane = Plane(camera.gameObject.forward, hit.point);
    // Stiff and damped for the body's mass: it follows within centimetres.
    _spring = body.gameObject.addComponent(
      SpringJoint(
        connectedBody: hand,
        anchor: body.gameObject.inverseTransformPoint(hit.point),
        connectedAnchor: Vec3.zero,
        spring: body.mass * 1000,
        damper: body.mass * 40,
      ),
    );
  }

  void _drag(ViewPoint point) {
    final plane = _plane;
    final at = plane == null ? null : camera.screenPointToPlane(point, plane);
    if (at != null) hand.movePosition(at);
  }

  void _let() {
    final spring = _spring;
    if (spring != null && !spring.gameObject.isDestroyed) spring.gameObject.removeComponent(spring);
    _spring = null;
    _plane = null;
  }
}
```

The lab's "Joints: a chain and a weight" scene does this, and works out
the spring from the weight it lifts. With an `Orbit` on the camera
([Cameras](/graphics/cameras/)), a drag that carries a body turns the camera as well, and the body
runs away under the finger: set `orbit.controls = false` in `_grab` when
a body is caught, and back to true in `_let`, so a drag on nothing still
turns it.
