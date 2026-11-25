---
title: "Moving things"
description: "Positions, rotations, directions, parents, and the maths a game needs."
---

On objects and behaviours alike:

```dart
class Drone extends Behaviour {
  final _follow = SmoothDamp3(smoothTime: 0.3);

  @override
  void onUpdate() {
    super.onUpdate();
    translate(Vec3.forward * time.delta); // the way it looks
    rotate(Vec3.up, 0.5 * time.delta); // about its own up
    final player = world.first<Pilot>();
    if (player == null) return;
    lookAt(player.position);
    position = _follow.step(position, player.position + Vec3(0, 3, 0), time.delta);
  }
}

class Pilot extends GameObject {}
```

- **As Unity's**: `position` and `rotation` are in the world, whatever the
  object's parents; `localPosition`, `localRotation` and `localScale` are
  relative to its parent, and `lossyScale` is its size in the world.
  `forward`, `right` and `up` are in the world. For a root the two are the
  same.
- `translate` and `rotate` go by the object's own axes; give `relativeTo:
  Space.world` for the world's. `lookAt(point)`, `transformPoint` and
  `inverseTransformPoint` too.
- `parent = other` keeps the object where it is in the world, as Unity's;
  `setParent(other, worldPositionStays: false)` keeps its local transform
  instead, so it moves with its new parent.
- The local transform is one value, a position, a rotation and a scale:
  `transform = Transform(position: …, rotation: …)` sets it at once, and
  `worldTransform` reads the world's.
- **A rotation is a `Quat`**, as in Unity: `Quat.lookRotation(direction)` to
  face a way, `Quat.euler(pitch, yaw, roll)` by angles (radians), and
  `Quat.axisAngle(axis, angle)`. A camera at `eye` looking at `target` is
  `Transform.lookAt(eye, target)`. `rotation.eulerAngles` reads the angles
  back (x the pitch, y the yaw, z the roll, in radians), so
  `rotation.eulerAngles.y` is the way an object faces; `Quat.fromTo(Vec3.up,
  normal)` turns one direction to another, and `Quat.rotateTowards(rotation,
  goal, turnSpeed * time.delta)` turns by steps, never past.
- `v.withY(0.5)` (and `withX`, `withZ`) changes one part, `v.flat` puts it on
  the ground (y = 0) for walking, and `Vec3.all(2)` is a uniform scale.
- **A zero vector has no direction**: `normalized` throws for one, so a vector
  that should never be zero says so where it is. Where one can be, as the way
  to a target the object may have reached, write `normalizedOrZero`, which
  gives zero (as Unity's `normalized`): `position += (target -
  position).normalizedOrZero * speed * time.delta`. `lookAt` the point the
  object is at leaves it as it is. A `Vec2` has `normalized` and
  `normalizedOrZero` too.
- `color.withAlpha(0.5)` is the colour half see-through (Unity's `color.a =
  0.5`).
- Unity's Mathf: `lerp`, `inverseLerp`, `clamp01`, `moveTowards`, `repeat`,
  `pingPong`, `randomInSphere`, `randomOnCircle`, and `Vec3.lerp`,
  `Vec3.moveTowards`, `Quat.slerp`, `Color.lerp`. `SmoothDamp` and
  `SmoothDamp3` keep their velocity between frames.
