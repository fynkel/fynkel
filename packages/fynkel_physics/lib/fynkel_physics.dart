/// Physics for Fynkel, as Unity has it, on Rapier (docs/stage-5.md).
///
/// Put a [BoxCollider], [SphereCollider] or [CapsuleCollider] on an object
/// and physics knows it; ask what's where with `physics.raycast` and the
/// other queries, from any object or behaviour. Rigidbodies and callbacks
/// come in the stage's later steps.
library;

export 'src/physics.dart'
    show
        BehaviourPhysics,
        BoxCollider,
        CapsuleCollider,
        CapsuleDirection,
        Collider,
        GameObjectPhysics,
        Physics,
        PhysicsHit,
        PhysicsQueries,
        QueryTriggers,
        SphereCollider,
        WorldPhysics;
export 'src/raw.dart' show PhysicsException, PhysicsStatus;
