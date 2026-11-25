---
title: "Splines"
description: "Roads, ropes and tracks: smooth curves through points."
---

![A path, a railway, a rope and a fence along splines](/shots/splines.webp)

A `Spline` is a smooth curve through knots, as Unity's: a road, a river,
a rope, a cart's track, where the fence posts go. It's plain Dart, so a
game uses it to move and place things as much as to make meshes.

```dart
// Smooth through every point (Unity's auto tangents), or straight lines
// between them (Spline.linear), or knots with their own handles.
final river = Spline.smooth([Vec3(0, 0, 0), Vec3(8, 0, -4), Vec3(16, 0, 2)]);
final arch = Spline([
  Knot(Vec3(0, 0, 0), mode: TangentMode.bezier, tangentOut: Vec3(0, 3, 0)),
  Knot(Vec3(5, 0, 0), mode: TangentMode.bezier, tangentIn: Vec3(0, 3, 0)),
]);

// Everything along it is by distance in metres.
final halfway = river.positionAt(river.length / 2);
final facing = river.rotationAt(3); // -Z along it, up as upAt
final closest = river.nearest(Vec3(5, 0, 5)); // (distance:, position:)

// A road: a profile swept along it, caps on its ends, UVs in metres:
// u around the profile, v along the path. setSweep sweeps again into the
// same mesh, or into an empty Mesh() the first time.
final road = Mesh.sweep(river, Shape2D.rect(4, 0.3), segmentsPerMetre: 2);
```

### By distance, not t

`positionAt`, `directionAt`, `upAt` and `rotationAt`
take metres along it, so things go at an even speed and are evenly spaced;
Unity's normalised t is `distance / length`. A `closed` spline joins its
last knot to its first and distances go round.

### Up

Up turns as little as it can along the curve (a rotation-minimising
frame), so a tube doesn't twist where it bends; it starts as +Y, and a
knot's `roll` turns it, clockwise seen going along: a banked turn. It keeps
no world up, as Unity's knots do: where the curve climbs as it bends, and
round a closed loop, up comes out rolled a few degrees, so a road swept
along it leans.

For a level road, measure each knot's lean (the angle between `upAt` and
world up, both square to `directionAt`) and set the knots again with `roll:`
minus it.

### Changing it

`spline[i] = Knot(...)`, `add`, `insert`, `removeAt`, or
`setPositions(floats)` for every knot at once; `version` goes up each time.
`mesh.setSweep(spline, shape)` sweeps again into the same mesh: a road whose
knots are dragged, or a rope that swings each frame, which makes no garbage
when it's filled with `setPositions`, and allocates nothing in the engine
either once it has swept one that size. The caps are filled once while the
shape stays the same.

### Samples

`segmentsPerMetre` (2 unless given) and more where it bends, at
most `maxSegments`. A 100 m road with a 12-point profile takes about 0.06 ms
in Rust on a Mac.

## On objects

On objects, as Unity's Splines package has them:

```dart
/// A track round [points] with sleepers along it, and a cart riding it.
List<GameObject> railway(List<Vec3> points, Mesh sleeper, Material wood, GameObject cart) {
  final track = GameObject(components: [
    SplineContainer([Spline.smooth(points, closed: true)]),
    // Sleepers every 35 cm, placed again when the spline changes.
    SplineInstantiate((i) => GameObject(components: [MeshRenderer(sleeper, wood)]), spacing: 0.35),
  ]);
  cart.addComponent(SplineAnimate(track.get<SplineContainer>()!, speed: 2, loop: SplineLoop.loop));
  return [track, cart];
}
```

- **`SplineContainer`** holds splines in its object's space: move the object
  and they move with it. Its `positionAt` and the rest answer in the world,
  distances included: on an object scaled 2, `length()` is twice the spline's,
  and `positionAt(5)` is 5 m along in the world.
- **`SplineInstantiate`** makes copies with its function, hung from its
  object, `spacing` metres apart (along the spline in its own space, so
  scaling the object spaces them out with it) or `count` of them, turned along
  the spline unless `align` is false, `offset` from it.
- **`SplineAnimate`** moves its object along at a `speed` or over `seconds`,
  `once`, `loop` or `pingPong`, turned to face the way it goes; `play`,
  `pause`, `restart`, `distance` and `normalizedTime`. When its container's
  object is destroyed it holds still, as `Follow` does.
- **`world.debug.drawSplines = true`** draws every container's splines and
  knots.

The lab's "Splines" has a road whose knots can be dragged, a rope that
swings between two posts, a fence and a cart on a track
([examples/lab/lib/scenes/splines.dart](https://github.com/fynkel/fynkel/blob/main/examples/lab/lib/scenes/splines.dart)).
