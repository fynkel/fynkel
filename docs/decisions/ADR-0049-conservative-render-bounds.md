# ADR-0049: Conservative bounds for affine and animated geometry

**Status:** Accepted and implemented; qualification recorded in the linked report.  
**Date:** 2026-09-30  
**Authority:** User authorization to continue roadmap F1.4.  
**Related:** ADR-0048; roadmap F1.4/F4/F6.  
**Evidence:** [Bounds migration](../design/foundation-bounds-migration.md).

## Context

The renderer used the longest column of a world matrix to scale a mesh sphere. This is conservative for orthogonal TRS columns, but not for a sheared hierarchy. Skinned meshes also kept their cooked bind-pose sphere after joints moved vertices outside it. Both errors affect camera rejection, shadow-caster rejection, shadow-fit tightening and object light selection.

The vertex stage can additionally run a custom shader hook with arbitrary displacement. There is no declared envelope for that displacement, so a finite bind-pose bound cannot justify rejecting such a draw.

## Decision

For a static sphere under affine linear map A, bound maximum stretch by `sqrt(max_i sum_j abs((A^T A)[i,j]))`. This is an upper bound on the largest singular value, and reduces to the largest axis scale for orthogonal columns. Calculate the Gram matrix in double precision and expand the final sphere outward for float rounding. Handle reflection and collapsed axes without inversion. Positive constant homogeneous scale is normalized; non-affine or non-finite inputs fail open.

At mesh upload, the renderer builds a bind-space sphere for the vertices influenced by each joint. Include a vertex in every joint group with a positive weight. Store this metadata beside the GPU mesh, with the same residency lifetime. Each frame, before culling, transform those spheres using the draw's actual extracted palette and complete world placement, including extra worlds, then union them. A sphere containing every transformed influence contains their nonnegative weighted blends. This avoids per-frame CPU skinning of vertices.

Use the resulting world sphere for camera and shadow rejection, caster-fit tightening, outline eligibility and object light selection. Recompute it from the interpolated/presentation palette used by the GPU; do not reuse a completed-tick animation bound for a different render pose.

Missing/invalid influence metadata, an invalid palette or a custom vertex hook without a displacement envelope produces an unbounded result. Keep the draw and caster eligible. Do not feed infinities into cascade fitting: if an unbounded caster participates, retain the normal camera-range shadow fit instead of tightening it to finite caster bounds. Existing shadow distance/range policy still applies.

Expose `FrameStats.bounds_joints` and `bounds_unbounded` so the update work and conservative fallback are observable. Do not add authored padding guesses or a second manually maintained animation bound.

## Cost and consequences

Building influence groups is a one-time mesh-upload operation, O(V × 4 × log J) with the current ordered map, and O(J) persistent metadata. Per-frame work is O(J) per skinned draw and allocates no temporary influence arrays. It runs before visibility rejection, so even off-screen rigs pay for a current bound. Static transforms stay O(1).

Bounds may be loose, especially for widely separated joints. This trades additional draws for correctness. Revision caching, tighter envelopes and declared shader displacement limits can be evaluated in F4/F6 without weakening the conservative contract. The qualification report records a synthetic CPU cost measurement; it is not a whole-frame performance guarantee.

No disk format, entity layout or runtime ABI change is required. GPU skinning, rendering and projection semantics stay the same. This decision concerns visibility bounds; it does not introduce affine collision geometry or alter the physics restrictions from ADR-0048.
