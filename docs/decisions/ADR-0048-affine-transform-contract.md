# ADR-0048: Exact affine transforms and local-pose interpolation

**Status:** Accepted and implemented; qualification recorded in the linked report.  
**Date:** 2026-09-30  
**Authority:** User authorization to continue roadmap F1.3.  
**Related:** ADR-0042, ADR-0046; roadmap F1.3/F1.4/F2.  
**Evidence:** [Transform migration](../design/foundation-transform-migration.md).

## Context

Rust composes local TRS matrices through the hierarchy. Dart previously multiplied scales component by component and composed quaternions, then used that approximate pose for points and world-position setters. Rotating a child beneath non-uniform scale introduces shear: a world TRS cannot retain it. The baseline probe measured a 0.7071 metre disagreement.

Native rendering also decomposed previous/current world matrices before interpolation. Even an interpolation endpoint could lose shear. Keep-world reparenting decomposed a derived local matrix without checking whether local TRS could represent it, and changed hierarchy links before discovering invalid results.

## Decision

Local authoring remains position, unit quaternion and scale. Derived world geometry is an affine matrix: `parentWorld * localTRS`. Never reconstruct geometry from a lossy world pose. The SDK's immutable `Affine3` mirrors native affine operations and composes currently visible local values, including staged ancestor and hierarchy edits. Native operations read current ancestors when an operation requires current world coordinates.

Points include translation, scale and shear. Vectors include scale and shear without translation. Directions are explicitly orientation-only and preserve magnitude: compose local quaternions without scale. `Transform.rotation` uses that authored orientation. `Transform.world` is a convenience pose with exact position, authored orientation and signed basis lengths; it is not an exact matrix decomposition. Completed native snapshot TRS remains an approximate matrix decomposition. Use the named matrix API for geometry.

Inverse operations reject non-finite or singular bases. The relative determinant threshold is `1e-8`; there is no zero-divide-to-zero fallback. Forward transformation of a collapsed axis remains valid. Checked TRS conversion rejects zero-length basis columns and shear, with pairwise normalized basis dot products limited to `1e-4`. Reflections are assigned to the X scale. Floating-point results are compared with tolerance across Dart doubles and native f32.

Keep-local reparenting remains the default. Keep-world reparenting must be representable by local TRS under the new parent. Reject unsupported operations before changing links or staging commands; do not silently approximate. The SDK resolves its visible world matrix to local TRS before queuing keep-local plus the resulting transform in the same simulation batch. This preserves immediate reads, including staged edits. General batch rollback remains F2.

Render interpolation stores previous local TRS and parent identity. Interpolate local position/scale and quaternion rotation, then compose parent/child matrices. A parent change snaps that entity to its current world matrix because the old local coordinate space is no longer applicable. Descendants compose from that parent; presentation overrides retain their existing subtree behavior. Projection uses the same extracted matrices.

Physics accepts non-singular world TRS within the same shear tolerance. Unsupported collision transforms emit an invalid-payload diagnostic and leave the physics world; no stale collider remains. Repeated invalid frames do not repeat that diagnostic. Correcting the transform restores the physics entry. Moving rigid bodies and characters must be roots or use positive uniform-scale ancestry, checked before simulation (relative scale tolerance `1e-5` per ancestor). An unchanged writeback matrix is not proof of validity: sync validates it again. Arbitrary affine rigid-body simulation and sheared collision geometry are outside this contract. Static world-TRS colliders retain their existing scale rules.

After a manual fixed sequence, the host snaps interpolation history to the completed local poses before presentation. Rendering and projected anchors then show the completed hierarchy at any accumulator remainder. Clocks and remainder do not change; normal interpolation resumes when the next tick records its previous poses.

## Consequences

Gameplay point conversion, native hierarchy propagation, rendering and projected anchors preserve the same geometry. Game authors retain accessible local TRS controls and orientation helpers. Inverse and keep-world operations can now throw or emit a diagnostic where previous code silently returned incorrect coordinates.

This does not add authored shear, change runtime ABI 3, migrate snapshot records, or resolve conservative bounds (F1.4). Transform caching and the unified staged-state transaction model remain F2/F4. Native systems that explicitly request approximate TRS decomposition retain that approximation; they must not use it as an exact geometry representation.
