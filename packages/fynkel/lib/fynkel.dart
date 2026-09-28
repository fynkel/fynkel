/// Fynkel Engine — Dart gameplay API: `World`, `GameObject`, `Behaviour`
/// with its lifecycle, `GameContext` services, typed events, diagnostics,
/// and the ABI v1 bridge underneath.
library;

export 'generated/schema.g.dart'
    show
        AlphaMode,
        AnimateCommandRecord,
        AnimateOp,
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
        AudioBusCommandRecord,
        AudioCommandRecord,
        AudioListener,
        AudioOp,
        AudioSource,
        AudioStateRecord,
        AudioStatsRecord,
        BodyCommandRecord,
        BodyOp,
        BodyStateRecord,
        BodyType,
        Camera,
        CharacterController,
        CharacterStateRecord,
        Collider,
        ColliderShape,
        ComponentCodec,
        DirectionalLight,
        Emitter,
        EntityInfoRecord,
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
        PhysicsQueryRecord,
        PlaySoundCommandRecord,
        PlaybackEndedEvent,
        PointLight,
        RigidBody,
        Rotator,
        Projection,
        ProjectedPointRecord,
        ProjectionAnchorRecord,
        Settings,
        Skin,
        SnapshotTransformRecord,
        SpotLight,
        SystemKind,
        SystemTimeRecord,
        Tags,
        ThirdPersonCamera,
        TriggerZone,
        Volume,
        VolumeShape,
        componentCodecs,
        componentIds;
export 'src/animation.dart' show AnimationAssets, AnimatorParameter, AnimatorState, AnimatorStateInfo;
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
export 'src/physics.dart' show BodyState, CharacterState, CollisionFlags, PhysicsHit, PhysicsSettings, RigidbodyConstraints;
export 'src/systems.dart';
export 'src/recording.dart';
export 'src/settings.dart';
export 'src/transform_writer.dart';
export 'src/native/bridge.dart'
    show BridgeStats, CommandLane, FramePlan, NativeLibrary, NativeWorld, QueryKind, StepResult, SubmitResult;
export 'src/wire/buffers.dart' show CommandBatch, RecordBatch, WireBuffer;
export 'src/wire/support.dart' show WireRecord;
export 'src/world.dart' hide bindWrittenData, specOfObject, writingReferences;
