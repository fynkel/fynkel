# Exact affine transform migration (F1.3)

**Date:** 2026-09-30  
**Decision:** [ADR-0048](../decisions/ADR-0048-affine-transform-contract.md)  
**Scope:** SDK spatial operations, native world-pose writes and keep-world reparenting, render interpolation, projection agreement, and physics transform constraints.

## What changed

`packages/fynkel/lib/math.dart` now provides immutable `Affine3`, storing three basis columns and translation. Its composition, point/vector operations, inverse and checked TRS conversion retain shear. Local poses remain the existing generated `Pose`; no second authoritative transform component was introduced.

`Transform.localToWorldMatrix` composes local poses through the visible parent chain. It includes pending spawns, staged ancestor edits and reparents. `worldToLocalMatrix`, world-position writes, point and vector conversion use the exact affine inverse. A singular inverse throws before a setter writes anything.

```dart
final matrix = object.transform.localToWorldMatrix;
final worldPoint = object.transform.transformPoint(localPoint);
final localPointAgain = object.transform.inverseTransformPoint(worldPoint);
final worldDisplacement = object.transform.transformVector(localDisplacement);
final facing = object.transform.transformDirection(Vec3.forward);
```

Directions and `rotation` compose authored local quaternions without scale. Basis vectors from the matrix can therefore differ from orientation directions under non-uniform scale and shear. `lossyScale` measures basis-column lengths, with reflection sign on X. `world` is explicitly a convenience pose, not an exact representation of sheared geometry. The older completed `GameObject.worldTransform` snapshot is still approximate TRS and does not include staged edits.

Native `set_world_pose` now uses current affine ancestors for position and the authored quaternion chain for orientation. It no longer relies on an outdated cached parent or a lossy inverse-matrix decomposition. Native keep-world reparenting validates the new local TRS before mutating hierarchy links. SDK keep-world reparenting computes the same operation from its visible state, stages the result and immediately reflects it in subsequent reads. Cycle and cross-world parenting checks occur before staging.

Render interpolation now retains previous local pose and parent identity. Local interpolation followed by matrix composition retains shear at endpoints and between ticks. Changed parent identity snaps the reparented entity; descendant local interpolation and existing presentation-subtree overrides compose on top. Projected anchors consume the renderer's extracted affine matrix.

## Failure and compatibility rules

- Inversion rejects non-finite matrices, zero basis volume, or relative determinant at most `1e-8`.
- Checked TRS conversion rejects zero-length axes or normalized basis dot products above `1e-4`.
- `setParent(keepWorld: true)` throws for unsupported local shear or a singular parent. Use keep-local parenting or choose a hierarchy whose new local transform is representable.
- Native keep-world failure emits `INVALID_PAYLOAD` without changing links or local pose.
- Physics rejects singular/sheared world transforms, removes existing collision geometry, reports once while continuously invalid, and restores the entry after correction. This can expose previously approximate colliders in existing content.
- Moving rigid bodies should be roots or use positive uniform-scale ancestry. Primitive collider sizing retains existing rules; this work does not add sheared collision shapes or arbitrary affine rigid-body dynamics.
- Runtime ABI remains 3. Rebuild native libraries to obtain the new interpolation and validation behavior.

## Regression evidence

The previous probe's approximately `(1.4142, 1.4142, 0)` SDK result disagreed with native `(1.4142, 0.7071, 0)`. The migrated probe compares the same completed tick using affine SDK composition.

Added tests cover:

- Nested rotated non-uniform scales and reflection, before and after native ticks.
- Point/vector inverse round trips and orientation-only directions.
- Staged ancestor edits followed by exact world-position writes.
- Immediate and completed keep-world reparenting; rejection of shear, singular parents and cycles without staging side effects.
- Native current-ancestor world-pose writes and mutation-free singular rejection.
- Exact sheared render interpolation at zero, half and near-one alpha, including descendant composition and presentation overrides.
- Projected anchor agreement with extracted sheared geometry.
- Physics removal, diagnostic deduplication and recovery after invalid transforms.

Qualification is run by `tools/foundation_baseline.py --only transforms`, using the same 15-command gate as the clock migration: workspace build/check/tests, formatting, strict Clippy, generated-code check, SDK/authoring/Flutter analysis and tests, editor analysis and focused play/scene/hierarchy tests, followed by the original foundation probe.

The [archived qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-transforms.json) records all 15 commands passing on the working tree against `87e3dec8`: **825 Rust tests** (6 ignored), **353 SDK tests**, **16 authoring tests**, **39 Flutter tests**, and **91 focused editor tests**. Analysis, strict Clippy, formatting and code generation checks pass. Raw logs are in `target/foundation-baseline/transform-migration`.

The original probe now reports `(1.41421358, 0.70710675, 0)` from the SDK and `(1.41421366, 0.70710677, 0)` natively: error **7.882e-8 metres**, within the `1e-5` comparison tolerance. `completedTickPositionsDisagree` is false. Identity and clock probe regressions remain passing.

Full editor/reference-game release qualification and the other F0/F1 gates remain separate.

## F1 review follow-up

The pre-F2 review reproduced two gaps despite the original green qualification:
physics could trust its own cached sheared writeback, and manual editor Step
could render the previous pose at zero interpolation alpha.

Physics now validates world TRS on every sync and rejects dynamic/kinematic
bodies and characters with nonpositive or nonuniform ancestor scales before
simulation. Correcting the hierarchy restores the entry; static supported TRS
colliders remain allowed. Regression tests cover cached invalid writeback,
unsafe grandparents, reflection, initial rejection, removal after an ancestor
edit, diagnostic deduplication, recovery and subsequent rotation.

Manual Step now snaps previous local poses to completed poses before
presentation, through the additive `fynkel_world_snap_interpolation` API.
Native extraction and SDK projection regressions cover descendants, zero and
half alpha, repeated Step, unchanged clocks/remainder and resumed regular
interpolation. Existing ABI null/poisoned-world checks cover the new entry point.

The three new physics regressions fail with the original validation restored;
the SDK projection regression fails with the snap call disabled. Both fixes
were restored before the final run. The [review-fix qualification capture](../../benchmarks/foundation/results/2026-09-30-87e3dec8-f1-review-fixes.json)
records all 15 commands passing: **836 Rust tests** (8 ignored), **355 SDK**,
**16 authoring**, **39 Flutter**, and **91 focused editor** tests. Formatting,
strict Clippy, analysis, generated-code checks and the foundation probe pass.
Raw logs: `target/foundation-baseline/f1-review-fixes`.

## Remaining boundaries

The subsequent [F1.4 bounds migration](foundation-bounds-migration.md) implements conservative render/shadow bounds under shear and deformation. Exact matrices alone did not make the previous bounds conservative. F2 owns staged-state unification, explicit presentation policy and general atomic transactions; the existing SDK overlay and simulation batch remain in use. F4 owns revision-based derived-state caching: the SDK matrix currently walks the ancestor chain.

Snapshot TRS, camera orientation extraction, animation/IK decomposition and native consumers requesting approximate TRS remain approximate. They must not be treated as exact world geometry. Zero-scale forward transforms are supported, but approximate TRS consumers are not certified for degenerate bases. Matrix inversion thresholds are practical rejection rules, not a proof of numerical conditioning for extreme scale ranges.
