/// The native physics world, by ids, for package:fynkel_physics's own
/// tests and the bench. Each `RawPhysics()` is a world of its own, with
/// nothing to do with a game's `world.physics`: it can't reach that one's
/// bodies or colliders. A game uses
/// `package:fynkel_physics/fynkel_physics.dart`, which has
/// [PhysicsException] and its [PhysicsStatus] too.
library;

export 'src/raw.dart' show BodyKind, PhysicsException, PhysicsStatus, RawPhysics;
