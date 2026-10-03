/// Ordinary gameplay API: one World, component and mutation contract.
/// Host/native integrations use `advanced.dart`, which includes this API.
library;

export 'generated/schema.g.dart'
    show
        AlphaMode,
        AnimateOp,
        AnimationBinding,
        AnimationEndedEvent,
        AnimationMarkerEvent,
        Animator,
        AnimatorLayerStateRecord,
        AnimatorParameterKind,
        AnimatorParameterRecord,
        AnimatorStateEnteredEvent,
        AnimatorStateExitedEvent,
        AnimatorStateRecord,
        AudioBus,
        AudioListener,
        AudioOp,
        AudioSource,
        AudioStateRecord,
        AudioStatsRecord,
        BodyOp,
        BodyStateRecord,
        BodyType,
        Camera,
        CharacterController,
        CharacterStateRecord,
        Collider,
        ColliderShape,
        DirectionalLight,
        Emitter,
        Environment,
        ExposureMode,
        Follow,
        Layer,
        LifetimeDespawn,
        LookAtCamera,
        MeshRenderer,
        MotionSpace,
        Name,
        Oscillate,
        PbrMaterial,
        PhysicsHitRecord,
        PhysicsQueryKind,
        PlaybackEndedEvent,
        PointLight,
        RigidBody,
        Rotator,
        Projection,
        ProjectedPointRecord,
        ProjectionAnchorRecord,
        Settings,
        Skin,
        SpotLight,
        SystemKind,
        SystemTimeRecord,
        Tags,
        ThirdPersonCamera,
        TriggerZone,
        Volume,
        VolumeShape,
        componentIds;
export 'src/animation.dart'
    show AnimationAssets, AnimatorParameter, AnimatorState, AnimatorStateInfo;
export 'src/clip_data.dart';
export 'src/audio.dart' show AudioAssets, AudioState, AudioStats;
export 'src/asset_index.dart';
export 'src/asset_ref.dart';
export 'src/context.dart';
export 'math.dart';
export 'src/diagnostics.dart';
export 'src/editor_channel.dart';
export 'src/events.dart';
export 'src/gizmos.dart';
export 'src/input.dart';
export 'src/layers.dart';
export 'src/mesh_data.dart';
export 'src/model.dart';
export 'src/modules.dart';
export 'src/pool.dart';
export 'src/physics.dart'
    show
        BodyState,
        CharacterState,
        CollisionFlags,
        PhysicsHit,
        PhysicsSettings,
        RigidbodyConstraints;
export 'src/systems.dart';
export 'src/recording.dart';
export 'src/settings.dart';
export 'src/transform_writer.dart';
export 'src/native/bridge.dart' show BridgeStats, FramePlan, StepResult;
export 'src/world.dart' hide bindWrittenData, specOfObject, writingReferences;

export 'src/entity_ref.dart' show EntityRef;
