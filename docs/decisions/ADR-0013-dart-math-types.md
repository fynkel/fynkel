# ADR-0013: Dart math types

**Status:** Accepted  
**Date:** 2026-09-19  
**Related:** `docs/blueprint.md`

## Context

The blueprint fixed the coordinate convention (right-handed, Y-up, metres, radians, quaternion `[x,y,z,w]`) but not the Dart types.

## Decision

Fynkel-owned **immutable value types** `Vec2`, `Vec3`, `Vec4`, `Quat`, `Mat4` (and `Transform3`) in `package:fynkel/math.dart`, with operators, matching `glam` semantics and the fixed quaternion layout. No dependency on `vector_math`.

## Alternatives considered

- `vector_math`: familiar to Flutter devs but mutable, column-major, and its conventions would need adapting.

## Consequences

Allocation is under Fynkel's control (const constructors, no hidden mutation). Conversion helpers to/from `vector_math` may be offered in `fynkel_flutter` for widget interop.

## Amendment 2026-09-24 — Euler angles are degrees

Every angle in the API stays in radians except **Euler angles**: `Quat.euler(Vec3)`, `Quat.eulerAngles`, `TransformRef.eulerAngles` / `localEulerAngles`, `TransformRef.rotate` and `rotateAround` take and return degrees, because they are the numbers the Inspector's Rotation row shows and the ones a Unity user types. The order is the Inspector's (about X, then the resulting Y, then the resulting Z: `q = qz · qy · qx`), so a value read in the Inspector and typed into a script is the same rotation. `Quat.axisAngle` keeps radians.
