/// Fynkel Engine — Dart gameplay API: `World`, `GameObject`, `Behaviour`
/// with its lifecycle, `GameContext` services, typed events, diagnostics,
/// and the ABI v1 bridge underneath.
library;

export 'generated/schema.g.dart'
    show
        AlphaMode,
        BodyCommandRecord,
        BodyOp,
        BodyStateRecord,
        BodyType,
        Camera,
        Collider,
        ColliderShape,
        ComponentCodec,
        DirectionalLight,
        Environment,
        ExposureMode,
        MeshRenderer,
        Name,
        PbrMaterial,
        PhysicsHitRecord,
        PhysicsQueryKind,
        PhysicsQueryRecord,
        PointLight,
        RigidBody,
        Projection,
        SnapshotTransformRecord,
        SpotLight,
        componentCodecs,
        componentIds;
export 'src/asset_ref.dart';
export 'src/context.dart';
export 'math.dart';
export 'src/diagnostics.dart';
export 'src/events.dart';
export 'src/input.dart';
export 'src/physics.dart' show BodyState, PhysicsHit, PhysicsSettings;
export 'src/recording.dart';
export 'src/transform_writer.dart';
export 'src/native/bridge.dart'
    show BridgeStats, CommandLane, FramePlan, NativeLibrary, NativeWorld, QueryKind, StepResult, SubmitResult;
export 'src/wire/buffers.dart' show CommandBatch, RecordBatch, WireBuffer;
export 'src/wire/support.dart' show WireRecord;
export 'src/world.dart';
