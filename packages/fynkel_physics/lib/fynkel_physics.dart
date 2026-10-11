/// Physics for Fynkel, as Unity has it, on Rapier (docs/stage-5.md).
///
/// Put a [BoxCollider], [SphereCollider] or [CapsuleCollider] on an object
/// and physics knows it; ask what's where with `physics.raycast` and the
/// other queries, from any object or behaviour. Add a [Rigidbody] and
/// physics moves it: gravity, forces, collisions. Callbacks come in the
/// stage's later steps.
library;

export 'src/physics.dart'
    show
        BehaviourPhysics,
        BoxCollider,
        CapsuleCollider,
        CapsuleDirection,
        Collider,
        CollisionDetectionMode,
        ForceMode,
        GameObjectPhysics,
        Physics,
        PhysicsHit,
        PhysicsMaterial,
        PhysicsMaterialCombine,
        PhysicsQueries,
        QueryTriggers,
        Rigidbody,
        RigidbodyConstraints,
        RigidbodyInterpolation,
        SphereCollider,
        WorldPhysics;
export 'src/raw.dart' show PhysicsException, PhysicsStatus;
