---
title: "Cameras"
description: "Follow, orbit and look-at rigs, blends, shake, and cameras kept out of walls."
---

**Cameras** work as Cinemachine's. Put a `VirtualCamera` on objects; the
enabled one with the highest `priority` drives the view's camera, and a
change of priority blends to it (as does destroying the live one's
object: from where the view is to the next):

```dart
class CameraRig extends GameObject {
  CameraRig(GameObject player) : _player = player;

  final GameObject _player;
  late final VirtualCamera close, wide;

  @override
  void onLoad() {
    super.onLoad();
    add(GameObject(components: [Camera()])); // the view's camera
    close = addWith(
      VirtualCamera(priority: 1),
      components: [Follow(_player, offset: Vec3(0, 3, 6), damping: 0.2), LookAt(_player)],
    );
    wide = addWith(VirtualCamera(fovY: 70.deg), components: [Orbit(_player, distance: 15)]);
  }

  void zoomOut() {
    world.cameras.defaultBlend = Blend(1.5, ease: Ease.inOutSine);
    wide.priority = 2;
  }

  void hit() => world.cameras.shake(0.3);
}
```

## The camera itself

A `Camera` has `fovY` (radians: `60.deg`), `near`, `far`, `clearColor`,
`orthographic` with an `orthographicSize`, a `cullingMask` ([Layers and debug
drawing](/graphics/layers-and-debug/)), and `showSky` ([Sky and
fog](/graphics/sky-and-fog/)). The view draws from the active one,
`world.camera` (Unity's `Camera.main`).

Keep `near` below `far`: a view drawing from a camera that isn't throws an
`ArgumentError` naming it, each frame, rather than drawing nothing. It's
checked only then, so setting `near` and then `far` is fine.

## Shake and blends

`world.cameras.shake(strength, seconds:, frequency:)` (0.4 s unless told, as
an object's own `shake`), `setBlend(blend, from:, to:)`, `live`. A shake
moves the picture, not the camera: its transform stays where the game or the
virtual camera put it, so moving it during a shake never drifts.
`world.cameras.shakeOf(camera)` says how far it's moved now; world widgets
([Widgets in the world](/flutter/world-widgets/)) move with it.

## Rigs

Rigs are behaviours: `Follow(target, offset:, space:, damping:,
lookAhead:)`, `LookAt(target)`, and `Orbit(target)`, which the player turns
with a drag and zooms with a pinch or the wheel. A rig works on a plain
camera too, without a `VirtualCamera`. `Follow`'s `offset` is in the world
unless `space:` says otherwise, as Cinemachine's binding modes:
`FollowSpace.target` turns it with the target, so the camera stays behind a
car, and `FollowSpace.targetYaw` turns it only as the target turns left and
right.

They follow the target's own rotation: a car whose body has its rotation
locked, turning only its model, is followed by the model.

### Orbit's place is three fields

`Orbit`'s place is three fields, for code to set too (a reset button, a
turntable: `orbit.yaw += 0.5 * time.delta`): `distance` in metres (8); `yaw`
in radians, 0 on the target's +Z side and growing to the right; and `pitch`,
0 level and positive above, looking down (0.4).

`minPitch`/`maxPitch` (−0.2, 1.45) and `minDistance`/`maxDistance` (1, 100)
clamp them each frame, `offset` moves the point circled off the target's
position. `controls = false` stops drags, pinches and the wheel moving it,
and `dragSpeed` is how far a drag turns it (radians a logical pixel).

The wheel scrolled down moves it away (`input.scroll.y` is positive up;
Flutter's `scrollDelta.dy`, and a test's `scrollBy`, positive down).

### The rigs run after every onLateUpdate

The rigs run after every `onLateUpdate` ([The frame and the
lifecycle](/basics/lifecycle/): late updates, then cameras, then drawing),
so a camera with a rig is the rig's to place: move it in `onLateUpdate` and
the rig puts it back. Change the rig's fields there instead; the rig reads
them right after.

### A camera that doesn't go through walls

Add `CameraCollision()`
(fynkel_physics) beside the rig. Each frame, after the rig, it casts a
sphere from the point the camera looks at out to the camera, and brings the
camera in front of the first collider in the way, on the same line, so it
still looks at the target. It comes in at once and goes back out over
`damping` (0.3 s); it never stops at the target's own colliders or at a
trigger, and `layers:` leaves out more (glass, a bush).

`radius` (0.2 m) is the room it keeps; `minDistance` (0.3 m) the nearest it
comes; `dampingIn` (0) eases it in over seconds instead of at once, smoother
past a post but letting the post hide the player for that moment.

`isPulledIn` says whether something's in the way, and `distance` how near
the camera is: in a narrow passage, side-on, it comes right up to the
player, who then fills the view, so hide the model below a metre or so.

The point is the rig's: an `Orbit`'s target plus its `offset`, a `LookAt`'s,
or a `Follow`'s target, unless `target:` and `offset:` are given. An
`Orbit`'s `distance` stays what the player zoomed to. There's an example [below](#a-third-person-camera).  It's a `CameraExtension`: a behaviour whose `adjust(dt)`
runs after the rigs and before the view goes where the virtual camera is.

Make your own for anything else a camera must keep to (above the water,
inside the level's bounds): change `position` and `rotation` there; the rig
starts again from its own place next frame.

### Between the screen and the world

On a `Camera`, such as
`world.camera!`, `screenPointToRay(point)`, `screenPointToPlane(point,
Plane.ground)` and `worldToScreenPoint(point)`; `world.raycast(ray)` gives
what a ray hits first (a `RayHit`: `gameObject`, `point`, `normal`,
`distance`). It tests the meshes as drawn: a big mesh cast at many times
between changes gets a tree over its triangles, so 50 rays into 3,000
objects and a 50,000-triangle ground take 1.5 ms on a Mac; a mesh changed
every frame and cast at often is cheaper with a collider and
`physics.raycast`.

### A camera that follows a physics body

A camera that follows a physics body sees each fixed step's jump: a body
steps 50 or 100 times a second, the screen draws at 60 or 120, and the
camera moves smoothly, so the body seems to jitter. Give the body
`interpolation: RigidbodyInterpolation.interpolate`
([Rigidbodies](/physics/rigidbodies/)), as in Unity.

## A third-person camera

A third-person camera that keeps out of walls, and a camera extension of
the game's own:

```dart
// lib/follow_camera.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

GameObject followCamera(GameObject player) => GameObject(
  components: [
    Camera(),
    Orbit(player, distance: 6, offset: Vec3(0, 1.5, 0)),
    CameraCollision(layers: LayerMask.except([5])), // 5: the glass
    AboveWater(),
  ],
);

/// Keeps the camera above the water, at y 0.
class AboveWater extends CameraExtension {
  @override
  void adjust(double dt) {
    if (position.y < 0.2) position = Vec3(position.x, 0.2, position.z);
  }
}
```
