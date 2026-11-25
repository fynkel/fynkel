---
title: "Colliders"
description: "Shapes that are solid: boxes, spheres, capsules and meshes."
---

Physics is a package, `fynkel_physics`, on Rapier. Add it to the
pubspec, next to `fynkel`, and a game without it ships none of it:

```sh
flutter pub add fynkel_physics
```

## Shapes

Colliders are behaviours, as Unity's. Each has `center` and `isTrigger`, and
their defaults fit the mesh of the same shape:

| Collider | Arguments | Fits |
| --- | --- | --- |
| `BoxCollider` | `size:` | `Mesh.cube()` |
| `SphereCollider` | `radius:` | `Mesh.sphere()` |
| `CapsuleCollider` | `radius:, height:, direction:` | `Mesh.capsule()` |
| `MeshCollider` | `mesh:, convex:` | Any mesh |

Their sizes are in the object's own space, and they follow its world
position, rotation and scale (a sphere takes the largest axis). A collider
counts while it's enabled and its object shown. A collider with no
[`Rigidbody`](/physics/rigidbodies/) is static; a game moves it by moving
its object.

## Friction and bounce

Each collider has a `material`: `PhysicsMaterial(dynamicFriction:,
bounciness:, frictionCombine:, bounceCombine:)`, Unity's 0.6 and 0 by
default. There's one friction, sliding or not: no `staticFriction`.

Two touching colliders' values are put together by the
`PhysicsMaterialCombine` of either (`average` unless set; `minimum`,
`multiply`, `maximum`, the higher in that order wins). Ice that stays
slippery against anything is `PhysicsMaterial(dynamicFriction: 0,
frictionCombine: PhysicsMaterialCombine.minimum)`.

## Mesh colliders

**A `MeshCollider`** takes a mesh's own shape: `MeshCollider(mesh:)`, or with
no mesh its object's `MeshRenderer`'s. Its triangles suit the ground and what
stands on it (static, or on a kinematic rigidbody); with `convex: true` it's
the mesh's hull, which a falling body can be (on a dynamic rigidbody a mesh is
its hull either way).

The object's scale stretches it on each axis. When its mesh changes ([Meshes
that change](/assets/meshes/)) it follows by itself on the next step. A
model's parts each get one with `MeshCollider.addToParts`:

```dart
// lib/rock.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A rock model that balls roll over, along its real shape.
class Rock extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    MeshCollider.addToParts(add(await assets.instantiate('rock.glb')));
  }
}
```

## Floors without bumps

**A floor a ball rolls across fast is one mesh.** Where two colliders meet
edge to edge, a box beside a box or a ramp, a fast ball crossing the seam can
catch the edge of the next and hop (as in Unity): a ball at 5 m/s across two
flat boxes jumps at 0.7 m/s.

Within one `MeshCollider` the engine smooths the edges between its triangles,
so build such a floor's collider from its top faces only, flats and slopes
alike, with `Mesh.combine` of `Mesh.plane`s, and keep boxes, if you like their
look, for the `MeshRenderer`s alone.

The pieces must meet edge to edge, sharing each edge whole: where one piece's
edge meets the middle of another's (a T), or two meet at a sharp crest, the
ball still hops, so split a long piece where a shorter one meets it, and round
a crest.

Combining whole boxes doesn't help: the faces where they touch stay inside the
mesh and bump it worse.

```dart
// lib/green.dart
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_physics/fynkel_physics.dart';

/// A green of two flats and a ramp up between them, as one collider.
class Green extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    final ground = Mesh.combine([
      CombineInstance(Mesh.plane(), position: Vec3(0, 0, -0.6), scale: Vec3(1.6, 1, 2.8)),
      CombineInstance(
        Mesh.plane(),
        position: Vec3(0, 0.1, -2.9),
        rotation: Quat.axisAngle(Vec3.right, 0.2 / 1.8),
        scale: Vec3(1.6, 1, 1.81),
      ),
      CombineInstance(Mesh.plane(), position: Vec3(0, 0.2, -4.5), scale: Vec3(1.6, 1, 1.4)),
    ]);
    add(GameObject(components: [MeshCollider(mesh: ground)]));
  }
}
```

A ball that hits a wall while it rolls climbs it a little, its spin
against the wall's friction; give walls a `PhysicsMaterial` with no
friction if it shouldn't.
