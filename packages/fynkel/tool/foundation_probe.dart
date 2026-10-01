// F0 observations through the real SDK/native runtime. This is a diagnostic
// capture, not a regression suite that requires defects to remain present.
// Run from packages/fynkel: dart run tool/foundation_probe.dart
import 'dart:convert';
import 'dart:math' as math;

import 'package:fynkel/fynkel.dart';

class _WaitProbe extends Behaviour {
  double? startedAt, resumedAt;

  void begin() => startCoroutine(_wait());

  Iterable<Object?> _wait() sync* {
    startedAt = Time.elapsed;
    yield const WaitForSeconds(0.1);
    resumedAt = Time.elapsed;
  }
}

List<double> _point(Vec3 p) => [p.x, p.y, p.z];

Map<String, Object?> _identity(NativeLibrary library) {
  final first = World.create(library: library);
  final second = World.create(library: library);
  try {
    final a = first.spawn(name: 'first-world');
    final b = second.spawn(name: 'second-world');
    first.frame(1 / 60);
    second.frame(1 / 60);
    final oldHandle = a.entity;
    final alias = a.entity == b.entity;
    String? wrongTarget;
    var foreignRejected = false;
    try {
      wrongTarget = second.attach(oldHandle).name;
    } on StateError {
      foreignRejected = true;
    }
    first.dispose();
    final recreated = World.create(library: library);
    try {
      final c = recreated.spawn(name: 'recreated-world');
      recreated.frame(1 / 60);
      return {
        'task': 'F1.1',
        'firstHandle': oldHandle.toString(),
        'secondHandle': b.entity.toString(),
        'simultaneousWorldAlias': alias,
        'foreignHandleResolvesTo': wrongTarget,
        'foreignHandleRejected': foreignRejected,
        'recreatedHandle': c.entity.toString(),
        'destroyedWorldHandleAliasesRecreatedWorld': oldHandle == c.entity,
      };
    } finally {
      recreated.dispose();
    }
  } finally {
    first.dispose();
    second.dispose();
  }
}

Map<String, Object?> _time(NativeLibrary library) {
  final world = World.create(
    library: library,
    fixedDt: 1 / 60,
    maxStepsPerFrame: 4,
  );
  final diagnostics = <String>[];
  world.onDiagnostics = (items) =>
      diagnostics.addAll(items.map((d) => d.code.name));
  try {
    final wait = world.spawn(name: 'wait').addBehaviour(_WaitProbe());
    world.frame(1 / 60);
    world.context.run(wait.begin);
    final before = world.context.time.elapsed;
    final plan = world.frame(0.25);
    final elapsed = world.context.time.elapsed - before;
    final resumedElapsed = wait.resumedAt == null
        ? null
        : wait.resumedAt! - wait.startedAt!;
    return {
      'task': 'F1.2',
      'frameSeconds': 0.25,
      'fixedDt': world.fixedDt,
      'steps': plan.steps,
      'droppedSeconds': plan.droppedSeconds,
      'publicDelta': world.context.time.delta,
      'elapsedAdvance': elapsed,
      'waitRequestedSeconds': 0.1,
      'waitResumed': wait.resumedAt != null,
      'elapsedAtWaitResume': resumedElapsed,
      'deltaDisagreesWithElapsed':
          (world.context.time.delta - elapsed).abs() > 1e-5,
      'waitResumedBeforePublicElapsedDuration':
          resumedElapsed != null && resumedElapsed < 0.1 - 1e-5,
      'diagnostics': diagnostics,
    };
  } finally {
    world.dispose();
  }
}

Map<String, Object?> _transforms(NativeLibrary library) {
  final world = World.create(library: library);
  try {
    final root = world.spawn(
      name: 'scaled',
      transform: const Pose(
        position: Vec3.zero,
        rotation: Quat.identity,
        scale: Vec3(2, 1, 1),
      ),
    );
    final rotated = world.spawn(
      name: 'rotated',
      parent: root,
      transform: Pose(
        position: Vec3.zero,
        rotation: Quat.axisAngle(const Vec3(0, 0, 1), math.pi / 4),
        scale: Vec3.one,
      ),
    );
    final leaf = world.spawn(
      name: 'leaf',
      parent: rotated,
      transform: const Pose(
        position: Vec3(1, 0, 0),
        rotation: Quat.identity,
        scale: Vec3.one,
      ),
    );
    world.frame(1 / 60);
    // No staged writes remain. Compare exact native world position with
    // the SDK's exact affine composition on the same completed tick.
    final sdk = leaf.transform.position;
    final native = leaf.worldTransform!.position;
    final error = (sdk - native).length;
    return {
      'task': 'F1.3',
      'sdkWorldPosition': _point(sdk),
      'nativeWorldPosition': _point(native),
      'positionErrorMetres': error,
      'completedTickPositionsDisagree': error > 1e-5,
    };
  } finally {
    world.dispose();
  }
}

void main() {
  final library = NativeLibrary.open();
  try {
    print(
      const JsonEncoder.withIndent('  ').convert({
        'purpose': 'F0 current implementation observations; true flags identify reproduced disagreements',
        'identity': _identity(library),
        'time': _time(library),
        'transforms': _transforms(library),
      }),
    );
  } finally {
    library.dispose();
  }
}
