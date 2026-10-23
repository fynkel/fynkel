# Stage 13 plan: meshes made while the game runs

Status: accepted 2026-10-23 ("go and build that"), with the
recommended answer to each decision (§7). Scope and rules: [ADR-0001](decisions/0001-fynkel-2-code-first.md).
Stage 12 is done ([report](stage-12-report.md)).

A Fynkel game can make a mesh from code once (`Mesh.custom`), but it
can't change it afterwards, and everything beyond a list of triangles is
left to the game: a road along a curve, a rope, a hole cut through a
wall, a log split in two, a level built from blocks. Every one of those
is a few hundred lines of geometry a game shouldn't have to write.
Stage 13 adds **runtime meshes as Unity has them**: a `Mesh` that can be
changed at any time, even every frame; a builder and shapes with
settings; splines, with meshes swept along them; and cutting, by a plane
or by another mesh. It's the basics of making and managing meshes while
the game runs, quickly. **It is not a modelling program** (§5).

**The goal.** Each of these takes a few lines:

```dart
// A road along a curve, rebuilt when its points move.
final path = Spline.smooth([Vec3(0, 0, 0), Vec3(10, 0, 4), Vec3(20, 1, 0)]);
final road = Mesh.sweep(path, Shape2D.rect(width: 4, height: 0.2), uvPerMetre: 0.5);
world.add(GameObject(name: 'road', components: [MeshRenderer(road, asphalt), MeshCollider()]));

// A hole through a cube, and its inside in another material.
final wall = Mesh.box(size: Vec3(4, 3, 0.3)).subtract(Mesh.cylinder(radius: 0.6, height: 1), at: Transform(rotation: Quat.x(90.deg)));

// A log split where the axe hit: two pieces with their cut faces filled.
final (top, bottom) = log.slice(Plane.at(hit, normal: Vec3.up), inside: 1);

// A flag that waves: the same mesh, its vertices moved each frame.
@override
void onUpdate() {
  for (var i = 0; i < _rest.length; i += 3) {
    _moved[i + 2] = _rest[i + 2] + math.sin(time.sinceStart * 4 + _rest[i]) * 0.1 * _rest[i];
  }
  _flag..setPositions(_moved)..recalculateNormals();
}
```

**The road after it**: stage 14, animation 2 cut down to 2D blends,
look-at and foot IK; then stage 15, shipping = 1.0. (Animation 2 was
stage 13 and shipping 14 until meshes came before them, the user's ask
on 2026-10-23.)

## 1. What there is today

- **`Mesh.custom(positions:, normals:, uvs:, indices:)`** and
  `Mesh.raw` (packed lists) make a mesh once: missing normals are
  averaged by angle, tangents are generated with MikkTSpace, a broken
  mesh is refused with a message saying what's wrong. `Mesh.inBackground`
  builds one on a helper isolate. `positions` and `indices` read it
  back.
- **A mesh can't change.** In Rust it's an `Arc<MeshData>` that worlds
  share, and each view uploads it to the GPU once, by its key. To change
  it a game makes a new one and disposes of the old.
- **Six built-in shapes** of fixed size (`Mesh.cube()`, `sphere`,
  `capsule`, `cylinder`, `plane`, `quad`), as Unity's primitives.
  crates/fynkel_mesh already makes the sphere, capsule and cylinder with
  a number of segments, but only the defaults are reachable from Dart.
- **One material per `MeshRenderer`**, no submeshes. A model's parts
  are separate objects, each with its own.
- **No vertex colours.** A vertex is 48 bytes: position, normal,
  tangent, UV.
- **`MeshCollider`** (package:fynkel_physics) takes a mesh, or its
  object's renderer's, as triangles or as a convex hull. It's built when
  added and doesn't follow a change.
- **crates/fynkel_mesh** holds meshes on the CPU with nothing about the
  GPU or the world: the right place for the new geometry, testable
  without a screen.
- **Curves**: there's no spline type. Eases and particle curves are
  functions of 0..1, not paths in space.

## 2. Unity's way, and Fynkel's

| In Unity | In Fynkel after stage 13 |
|---|---|
| `new Mesh()`, then `vertices`, `normals`, `uv`, `colors`, `triangles`; or `SetVertices`, `SetNormals`, `SetUVs`, `SetColors`, `SetTriangles`, `SetIndices`. | `Mesh()`, then `setPositions`, `setNormals`, `setUvs`, `setColors`, `setTriangles`, from lists or packed typed lists. `Mesh.custom` stays as the one-call way. |
| `subMeshCount`, `SetTriangles(tris, submesh)`; `MeshRenderer.materials`, one per submesh. | `setTriangles(tris, submesh: 1)`; `MeshRenderer(mesh, material, materials: [...])`, one per submesh. |
| `RecalculateNormals`, `RecalculateBounds`, `RecalculateTangents`. | The same, `recalculateNormals()` and so on. Bounds follow by themselves. |
| `MarkDynamic`, for a mesh changed often. | `markDynamic()`: its GPU buffers are kept with room to grow and written in place. |
| `Clear`, `vertexCount`, reading `vertices` back. | `clear()`, `vertexCount`, `positions`, `normals`, `uvs`, `colors`, `triangles(submesh)`. |
| `Mesh.CombineMeshes(CombineInstance[])`. | `Mesh.combine([(mesh, transform), ...])`. |
| `Mesh.MeshData` and jobs, to build off the main thread. | `Mesh.inBackground(() => ...)`, as now, for any of this stage's operations. |
| Reassigning `MeshCollider.sharedMesh` after a change. | A `MeshCollider` follows its mesh's changes by itself, on the next physics step. |
| ProBuilder's shapes with settings (cube, cylinder, cone, torus, stairs, arch, pipe, plane…). | `Mesh.box`, `Mesh.sphere`, `Mesh.cylinder`, `Mesh.cone`, `Mesh.torus`, `Mesh.grid`, `Mesh.disc`, `Mesh.capsule`, with sizes and segments. The no-argument shapes stay as they are. |
| ProBuilder's extrude; a lathe in many tools. | `Mesh.extrude(Shape2D, depth:)` (with holes), `Mesh.lathe(profile, sides:)`. |
| The Splines package: `SplineContainer`, `BezierKnot`, tangent modes (auto, linear, Bézier), `Evaluate`, `GetLength`, `SplineUtility.GetNearestPoint`. | `Spline` (knots with Unity's tangent modes, closed or open), `positionAt`, `tangentAt`, `upAt`, `length`, `nearest`; `SplineContainer`, a component holding splines in its object's space. |
| `SplineExtrude` (a tube along a spline), `SplineInstantiate`, `SplineAnimate`. | `Mesh.sweep(spline, shape)` for any profile (a tube, a road, a rail, a fence's beam); `SplineInstantiate`; `SplineAnimate`. |
| ProBuilder's experimental Boolean (CSG); slicing is left to assets from the store. | `subtract`, `union`, `intersect` on closed meshes, and `slice(plane)`, built in, with the cut faces in a material of their own. |

## 3. Design decisions

### 3.1 A mesh that changes

- **The same `Mesh`, a new version.** Each change copies the new data
  into the engine's mesh and bumps its version. Worlds keep sharing it;
  each view sees the version changed and uploads it again on its next
  frame. Nothing is sent per frame for a mesh that didn't change.
- **Changes are gathered and sent once a frame.** `setPositions` then
  `recalculateNormals` then `setUvs` in the same frame are one upload,
  not three. Reading a mesh back (`positions`) sees every change made so
  far.
- **Dynamic meshes** (`markDynamic`) keep GPU buffers with room to grow
  (half again), and a change is written into them in place: no new
  buffer, no new bind group, while it fits. A mesh that isn't marked
  gets exact-size buffers, as now.
- **No garbage.** A game passes the same `Float32List` each frame; the
  engine copies from it. The allocation tests change a 10,000-vertex
  mesh every frame and count Dart's and Rust's allocations.
- **Vertex colours** (decision 3): a fifth attribute, RGBA as four bytes,
  white when a mesh has none, multiplied into the base colour, as
  Unity's particle and unlit shaders do. A material's own shader reads
  it as `in.color`.
- **Submeshes**: index ranges in one mesh, one draw each, each with its
  own material from `MeshRenderer.materials`. Instancing groups by mesh,
  submesh and material, so many of one thing is still one draw per
  submesh.
- **Skinned meshes don't change** in this stage (§5).

### 3.2 Making meshes: a builder, shapes, and the heavy work in Rust

- **`MeshBuilder`** is plain Dart: `addTriangle`, `addQuad`,
  `addMesh(mesh, transform)`, `submesh(n)`, `color`, then `build()` for a
  new mesh or `applyTo(mesh)` to replace one's contents. It grows typed
  lists, so building in a loop each frame makes no garbage once they're
  big enough. `flat: true` gives each triangle its own vertices, for a
  faceted look; otherwise vertices are shared as given.
- **The geometry is Rust** (decision 1), in crates/fynkel_mesh:
  shapes, extrusion, lathe, sweeping, combining, slicing and booleans.
  It's fast on a phone, it's tested without a screen, and it runs on any
  thread, so `Mesh.inBackground` covers all of it.
- **Shapes with settings** are made in Rust and come back as a `Mesh`:
  sizes, segments, and caps on or off. UVs are in metres where it makes
  sense (a box's faces, a grid), so textures tile at the same size
  whatever its dimensions; `uvPerMetre` scales them.
- **`Shape2D`**: an outline in a plane, with holes (`rect`, `circle`,
  `polygon`, `rounded`). Extruding fills its ends by triangulating it
  (ear clipping, holes joined to the outside), as fonts and maps do.

### 3.3 Splines

- **`Spline` is plain Dart math** (decision 4): a game uses it for
  movement and placement as much as for meshes, and evaluating a point
  is a few multiplications. Knots have Unity's tangent modes: auto
  (Catmull-Rom, through every knot), linear, and Bézier with handles.
- **Distance, not just t.** `positionAt` and the rest take a distance
  in metres along the spline (through a length table built when it
  changes), so things move at an even speed and fences are evenly
  spaced. Unity's normalised t is there too.
- **Up**: a spline carries a roll per knot; the frame along it is
  rotation-minimising, so a tube doesn't twist where the curve turns.
- **`Mesh.sweep(spline, shape)`** goes to Rust with the spline's
  samples: a profile repeated along it, with caps, UVs along its length
  in metres, and more samples where it bends (`segmentsPerMetre`, and a
  most). A rope that moves each frame sweeps again into the same
  dynamic mesh.
- **`SplineContainer`** is a component: splines in its object's space,
  drawn with the debug lines when `world.debug.drawSplines` is on.
  `SplineInstantiate` places copies of an object along one;
  `SplineAnimate` moves its object along one, as Unity's do.

### 3.4 Cutting

- **Slicing by a plane** is exact and quick: triangles across the plane
  are split, each side keeps its own, and the cut is filled with the
  triangulated outlines it leaves (with holes). The fill goes in its own
  submesh (`inside:`), so a cut log shows wood rings, not bark.
- **Booleans** (`subtract`, `union`, `intersect`) take two closed meshes
  (decision 2): every edge shared by exactly two triangles. A mesh that
  isn't closed is refused with a message saying so, not cut wrongly.
  The result keeps each side's UVs and submeshes; the other mesh's faces
  inside the result go in the `inside:` submesh.
- **How** (decision 2): a BSP tree of each mesh, clipped against the
  other (the classic method of csg.js), written in Rust in
  crates/fynkel_mesh. It's a few hundred lines, needs no native library,
  and is robust for the meshes games cut: a few thousand triangles,
  boxes, cylinders, walls. It makes more triangles than needed, so the
  result is cleaned up: vertices welded, slivers merged where they lie
  flat.
- **Colliders follow**: the pieces of a slice can be given a
  `Rigidbody` and a convex `MeshCollider` and fall, which is what cutting
  fruit or breaking a plank wants.

### 3.5 Colliders

- **A `MeshCollider` follows its mesh**: when its version changes, the
  physics package rebuilds the collider on its next step. Rebuilding a
  triangle collider costs (Rapier builds a tree of the triangles), so
  it's measured (§6) and the guide says how often is fine. A mesh that
  changes every frame (a flag, water) shouldn't have one.

### 3.6 Tests without a screen

- **Geometry in Rust**, tested plainly: a box minus a cylinder has the
  volume it should and stays closed; a slice's two halves add up to the
  whole's volume and both are closed; a sweep along a straight spline
  is the extrusion it should be; normals point outwards; a refused mesh
  says why.
- **Goldens** of each lab page's meshes, drawn.
- **In Dart**, `vertexCount`, `positions`, `triangles` and the spline's
  numbers, in `dart test`, without a view.

## 4. Phases

Each phase is one PR or a few, with its lab page, its guide section,
its tests, and plain words. A phase is done when its PRs are merged with
CI green and it's been seen on the Mac, the iPhone and the Note10+.

### Phase 1: a mesh that changes

- `Mesh()`, `setPositions`, `setNormals`, `setUvs`, `setColors`,
  `setTriangles`, `clear`, reading each back; `recalculateNormals`,
  `recalculateTangents`; `markDynamic`; changes sent once a frame.
- Vertex colours in the vertex and the shaders (decision 3).
- Submeshes and `MeshRenderer.materials`.
- `MeshCollider` follows its mesh.
- The lab: "Meshes that change", a flag waving, a grid of water
  rippling where it's tapped, ground dug where it's tapped (with its
  collider following), vertex colours painted by height.

*Done when* a 10,000-vertex grid changed every frame holds 60 on all
three at low, with no garbage, and every golden from before is the same.

### Phase 2: building meshes

- `MeshBuilder`; `Mesh.combine`.
- Shapes with settings: `box`, `sphere`, `cylinder`, `cone`, `torus`,
  `grid`, `disc` (and a ring), `capsule`.
- `Shape2D`, `Mesh.extrude` with holes, `Mesh.lathe`.
- The lab: "Building meshes", each shape with its settings as sliders, a
  letter extruded with its holes, a vase turned from a profile, and a
  little town of houses built from a builder and combined into one mesh.

*Done when* the shapes, the extrusion and the lathe match their goldens
and their tests, and a combined town is one draw.

### Phase 3: splines

- `Spline`, knots and tangent modes, by distance and by t, `nearest`,
  roll and a rotation-minimising frame.
- `Mesh.sweep` with caps and UVs in metres.
- `SplineContainer`, `SplineInstantiate`, `SplineAnimate`,
  `world.debug.drawSplines`.
- The lab: "Splines", a road whose knots can be dragged (rebuilt as they
  move), a rope that sags between two posts and swings, a fence of posts
  placed along a curve, a cart riding a closed track.

*Done when* the road, the rope and the fence match their goldens, and
dragging a knot of a 100 m road rebuilds it within a frame on the
Note10+.

### Phase 4: cutting, a game, and measuring

- `slice(plane, inside:)`; `subtract`, `union`, `intersect`.
- The lab: "Cutting meshes", a hole through a cube that moves with its
  cylinder, a wall with windows punched into it where it's tapped, and
  fruit cut by a swipe, the halves falling.
- A game uses it (decision 6): Sheepdog's fences along splines, and a
  worn path to the pen swept along one.
- The measurements (§6), the guide's "Meshes in code" section and §22's
  Unity rows, the report.

*Done when* §6's bars are met and the game runs as fast as before on all
three devices.

## 5. Not in this stage

A modelling program in any form: editing tools, selecting faces or
edges, sculpting, bevels and chamfers, subdivision, smoothing, UV
unwrapping, remeshing, decimation and LOD generation. Booleans on meshes
that aren't closed, and on meshes of tens of thousands of triangles.
Changing a skinned mesh. Marching cubes and voxels, meshes made on the
GPU, and saving a mesh to a file. They go in the guide's §21; voxels
are the first I'd add if a game asks.

## 6. Measurements

On the Mac, the iPhone 16 Pro Max and the Note10+ (profile builds):

- **Changing a mesh**: a 10,000-vertex grid, its positions and normals
  changed each frame, costs under 1 ms a frame on the Note10+, Dart's
  side and the upload together, and holds 60 at low on all three.
- **No garbage** in a frame that changes a dynamic mesh, builds with a
  builder that's big enough, or sweeps a rope.
- **Sweeping**: a 100 m road with a 12-point profile, rebuilt, in under
  2 ms on the Note10+.
- **Cutting**: a box minus a cylinder of 64 sides in under 5 ms on the
  Note10+, and slicing a 2,000-triangle mesh in under 2 ms. A bigger cut
  belongs in `Mesh.inBackground`; the guide says where the line is.
- **A collider following**: how long a `MeshCollider` of 1,000 and
  10,000 triangles takes to rebuild, measured and said.
- **Nothing lost**: a mesh that never changes draws as now, and every
  golden from before stage 13 is the same. What vertex colours add to
  each vertex is measured and said.
- **The game** at each level as fast as now (phase 4).

## 7. Decisions to make

1. **The geometry in Rust, the builder and splines in Dart**
   (*recommended*: cutting, sweeping and shapes are heavy loops that are
   fast in Rust and run off the UI thread; a builder and a spline are
   light, and plain Dart is easier to read and debug), rather than all in
   Dart (simple, but a boolean in Dart is slow, and slow in debug
   builds), or all in Rust (fast, but a builder's every call would cross
   into native code).
2. **Booleans on closed meshes only, by our own BSP in Rust**
   (*recommended*: no native library, robust for the meshes games cut,
   and a refused mesh says why), rather than the Manifold library (more
   robust and faster on big meshes, but C++ to build for iOS and
   Android) or no booleans (slicing alone). If BSP isn't good enough for
   a hole in a cube and windows in a wall, slicing still ships and
   booleans wait.
3. **Vertex colours in every vertex** (*recommended*: 4 bytes more,
   48 to 52, one vertex layout and no new pipelines), rather than a
   second buffer only for meshes that have them (no cost for the rest,
   but twice the pipeline variants).
4. **Splines as Unity's**: knots with auto, linear and Bézier tangents,
   by distance as well as t, a `SplineContainer` component, `SplineInstantiate`
   and `SplineAnimate` (*recommended*).
5. **A `MeshCollider` follows its mesh by itself** (*recommended*: what
   a game expects), rather than reassigning it as in Unity.
6. **Phase 4 in Sheepdog**: its fences along splines and a worn path to
   the pen swept along one (*recommended*: one game proves it, as stage 12), rather than
   a new small game built around cutting, or the lab alone.
7. **Animation 2 and shipping move one stage on**, to 14 and 15
   (*recommended*: the user's order).
