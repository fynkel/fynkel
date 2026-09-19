/// Fynkel Engine — Dart gameplay API (M1 seed): `World`, `GameObject`,
/// typed events, diagnostics, and the ABI v1 bridge underneath.
library;

export 'generated/schema.g.dart' show SnapshotTransformRecord;
export 'math.dart';
export 'src/diagnostics.dart';
export 'src/events.dart';
export 'src/transform_writer.dart';
export 'src/native/bridge.dart'
    show BridgeStats, CommandLane, FramePlan, NativeLibrary, NativeWorld, QueryKind, StepResult, SubmitResult;
export 'src/wire/buffers.dart' show CommandBatch, WireBuffer;
export 'src/world.dart';
