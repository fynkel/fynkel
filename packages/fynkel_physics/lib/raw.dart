/// The native physics world, by ids: what package:fynkel_physics builds
/// its colliders and rigidbodies on, and what its tests and the bench
/// measure. A game uses `package:fynkel_physics/fynkel_physics.dart`.
library;

export 'src/raw.dart' show BodyKind, PhysicsException, PhysicsStatus, RawPhysics;
