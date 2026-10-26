# Stage 13 report: meshes made while the game runs

[The stage 13 plan](stage-13.md). Phases 1 to 3 are merged (#143, #145,
#146). Phase 4, cutting and Sheepdog, was left out on 2026-10-26: the
user's call, so that stage 14 reviews and hardens what's there before
anything more is added.

## The result

A game can now make a mesh and change it while it runs, as in Unity: a
`Mesh` whose channels are set at any time, even every frame; shapes with
settings, a builder, extrusion and lathing; splines, with meshes swept
along them and objects placed and moved along them. **Cutting isn't
there**: `slice` and the booleans were phase 4.

| Phase | What | PR |
|---|---|---|
| 1 | `Mesh()` and its channels, `recalculateNormals`/`Tangents`, `markDynamic`, vertex colours, submeshes and `MeshRenderer.materials`, a `MeshCollider` following its mesh; lab "Meshes that change"; engine ABI 42, physics ABI 14 | #143 |
| 2 | Shapes with settings, `Shape2D`, `Mesh.extrude` with holes, `Mesh.lathe`, `MeshBuilder`, `Mesh.combine`; lab "Building meshes"; ABI 43 | #145 |
| 3 | `Spline`, `Mesh.sweep`/`setSweep`, `SplineContainer`, `SplineInstantiate`, `SplineAnimate`, `world.debug.drawSplines`; lab "Splines"; ABI 44 | #146 |

What each phase built, and its numbers, is under "As built" in the plan.

## Measured

- **Changing a mesh** (§6's first bar): the lab's flag and 10,201-vertex
  water hold 60 fps on the Mac and 59.9 on the Note10+ at low. The
  engine's side of it on the Note10+ is about 1.3 ms a frame against the
  1 ms bar, from 2.1 ms before. **Missed**, and it goes to stage 14.
- **No garbage** changing a dynamic mesh, building with a builder that's
  big enough, or sweeping a rope (tests in Dart and Rust).
- **Sweeping** a 100 m road with a 12-point profile: 1.05 ms on the
  Note10+, against 2 ms. The lab's "Splines" holds 59.9 fps there.
- **A collider following**: 0.23 ms for 1,000 triangles and 2.9 ms for
  10,000, with a step, on the Mac.
- **Nothing lost**: every golden from before stage 13 is the same.

## Not measured

- **The iPhone** for phase 1: its run sent no log lines. Owed, in stage 14.
- **Cutting** (§6's 5 ms and 2 ms bars) and **the game** at each level:
  phase 4, not built.
- **What vertex colours add** to each vertex was planned as 4 bytes
  (48 to 52); not measured in a frame.

## Not in this stage

Phase 4: `slice`, `subtract`, `union`, `intersect`, the lab's "Cutting
meshes", and Sheepdog's fences and path along splines. Cutting comes
back after stage 14; Sheepdog's splines are part of stage 14's "all
together" scene. The rest of §5 stands.

## Next

Stage 14, make it solid ([plan](stage-14.md)): review everything built
so far, fix what's found, and make it fast and easy to use together,
before anything new. Then animation 2 cut down (stage 15) and shipping
(stage 16).
