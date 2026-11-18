# rapier3d 0.36.1, patched

Rapier 0.36.1 as published on crates.io (Apache-2.0, by Dimforge,
https://github.com/dimforge/rapier), without its examples and tests, and
with two fixes for bodies with frozen axes (`LockedAxes`), found by
playtest game 12 (docs/playtests.md). The workspace's `Cargo.toml` uses
it through `[patch.crates-io]`. The first commit that added this folder
is the crate as published; `git diff` against it shows the fixes.

- `src/dynamics/solver/contact_constraint/contact_constraint_element.rs`,
  `solve_tangent_block`: friction's two directions are solved together
  through the inverse of a 2 × 2 matrix. A body that can be neither
  pushed nor turned along one of them (frozen along Z and turning only
  about Z, a body kept to a plane) makes it singular, and its inverse
  was taken as zero: no friction at all. A singular one now uses its
  pseudo-inverse.
- `src/dynamics/solver/joint_constraint/joint_constraint_helper.rs`,
  `finalize_constraints`: a joint's rows are made independent of each
  other first. A row made of the others (a hinge's along-Z row on bodies
  frozen along Z) was left as float noise and divided by, kicking the
  bodies from nothing. Such a row is now left out.

Fynkel's tests for both are in `crates/fynkel_physics/tests/stacking.rs`.
Drop this folder once a Rapier release fixes both.
