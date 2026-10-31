/// Physics for Fynkel, as Unity has it, on Rapier (docs/stage-5.md).
///
/// Put a [BoxCollider], [SphereCollider], [CapsuleCollider] or
/// [MeshCollider] on an object and physics knows it; ask what's where with `physics.raycast` and the
/// other queries, from any object or behaviour. Add a [Rigidbody] and
/// physics moves it: gravity, forces, collisions. Mix [Triggers] or
/// [Collisions] into an object or a behaviour to hear what it touches, and
/// say which layers meet with `physics.ignoreLayerCollision`. Walk a
/// player with a [CharacterController]. Join rigidbodies with a
/// [FixedJoint], [HingeJoint], [SpringJoint] or [CharacterJoint].
library;

export 'src/physics.dart'
    show
        BehaviourPhysics,
        BoxCollider,
        CapsuleCollider,
        CapsuleDirection,
        CharacterController,
        CharacterJoint,
        Collider,
        Collision,
        CollisionDetectionMode,
        CollisionFlags,
        Collisions,
        ControllerColliderHit,
        ControllerHits,
        ContactPoint,
        FixedJoint,
        ForceMode,
        GameObjectPhysics,
        HingeJoint,
        Joint,
        JointBreaks,
        JointLimits,
        JointMotor,
        JointSpring,
        MeshCollider,
        Physics,
        RaycastHit,
        PhysicsLayers,
        PhysicsMaterial,
        PhysicsMaterialCombine,
        PhysicsQueries,
        QueryTriggers,
        Rigidbody,
        RigidbodyConstraints,
        RigidbodyInterpolation,
        SoftJointLimit,
        SphereCollider,
        SpringJoint,
        Triggers,
        WorldPhysics;
export 'src/raw.dart' show PhysicsException, PhysicsStatus;
