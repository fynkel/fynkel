/// Advanced host, generated-code and native integration surface.
/// These are the same types used by gameplay; this library adds no alternate
/// storage, mutation path, or lifetime authority.
library;

export 'gameplay.dart';

export 'src/native/bridge.dart'
    show
        BridgeStats,
        FramePlan,
        NativeLibrary,
        NativeWorld,
        QueryKind,
        StepResult;
export 'src/native/fynkel_bindings.g.dart' show FynkelEntity;
export 'src/wire/buffers.dart' show RecordBatch, WireBuffer;
export 'src/wire/support.dart' show WireRecord;
export 'generated/schema.g.dart'
    show
        ComponentCodec,
        componentCodecs,
        componentIds,
        AnimateCommandRecord,
        AudioCommandRecord,
        BodyCommandRecord,
        PlaySoundCommandRecord,
        PhysicsQueryRecord,
        EntityInfoRecord,
        SnapshotTransformRecord;
