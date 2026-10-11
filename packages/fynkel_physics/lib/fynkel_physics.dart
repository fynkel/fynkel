/// Physics for Fynkel, as Unity has it, on Rapier (docs/stage-5.md).
///
/// Put a [BoxCollider], [SphereCollider] or [CapsuleCollider] on an object
/// and physics knows it; ask what's where with `physics.raycast` and the
/// other queries, from any object or behaviour. Add a [Rigidbody] and
/// physics moves it: gravity, forces, collisions. Mix [Triggers] or
/// [Collisions] into an object or a behaviour to hear what it touches, and
/// say which layers meet with `physics.ignoreLayerCollision`.
library;

export 'src/physics.dart'
    show
        BehaviourPhysics,
        BoxCollider,
        CapsuleCollider,
        CapsuleDirection,
        Collider,
        Collision,
        CollisionDetectionMode,
        Collisions,
        ContactPoint,
        ForceMode,
        GameObjectPhysics,
        Physics,
        PhysicsHit,
        PhysicsLayers,
        PhysicsMaterial,
        PhysicsMaterialCombine,
        PhysicsQueries,
        QueryTriggers,
        Rigidbody,
        RigidbodyConstraints,
        RigidbodyInterpolation,
        SphereCollider,
        Triggers,
        WorldPhysics;
export 'src/raw.dart' show PhysicsException, PhysicsStatus;
