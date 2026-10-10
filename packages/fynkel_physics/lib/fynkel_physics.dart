/// Physics for Fynkel, as Unity has it, on Rapier (docs/stage-5.md).
/// Colliders, rigidbodies and the rest come in the stage's later steps;
/// for now there is the native world they're built on
/// (`package:fynkel_physics/raw.dart`).
library;

export 'src/raw.dart' show PhysicsException, PhysicsStatus;
