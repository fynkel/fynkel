// Counting what frames allocate, with the VM service's allocation profile.
// Each test file runs one scenario only: in the JIT one scenario's code
// can box doubles in the next one's.

import 'dart:developer';
import 'dart:isolate' as isolates;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';
import 'package:vm_service/vm_service.dart';
import 'package:vm_service/vm_service_io.dart';

// A constant: `1 / 60` written in the loop is worked out, and boxed, at
// each call there.
const _dt = 1 / 60;

/// Connects to the VM service for the file's tests, and gives back
/// [madeEachFrame].
Future<Map<String, int>> Function(World world, int frames) allocations() {
  late VmService service;
  late String isolate;

  setUpAll(() async {
    final info = await Service.controlWebServer(enable: true, silenceOutput: true);
    service = await vmServiceConnectUri(info.serverWebSocketUri.toString());
    isolate = Service.getIsolateId(isolates.Isolate.current)!;
  });

  tearDownAll(() => service.dispose());

  Future<Map<String, int>> made(World world, int frames) async {
    await service.getAllocationProfile(isolate, reset: true);
    for (var i = 0; i < frames; i++) {
      world.update(_dt);
    }
    final profile = await service.getAllocationProfile(isolate);
    return {for (final m in profile.members!) '${m.classRef!.name}': m.instancesAccumulated ?? 0};
  }

  /// The classes [frames] frames made at least one of each of, with how
  /// many, beyond what asking for the count makes (decoding the profile
  /// makes thousands of objects, but not one per frame).
  return (World world, int frames) async {
    // Warm, and compiled: the VM optimizes a function after tens of
    // thousands of calls, and until then boxes its doubles.
    for (var i = 0; i < 100000; i++) {
      world.update(_dt);
    }
    // It compiles on a background thread, which falls behind while the
    // other test files run on every core: wait for it, then run a little
    // more so the optimized code is the code that runs.
    await Future<void>.delayed(const Duration(seconds: 1));
    for (var i = 0; i < 1000; i++) {
      world.update(_dt);
    }
    final none = await made(world, 0);
    final some = await made(world, frames);
    return {
      for (final MapEntry(:key, :value) in some.entries)
        if (value - (none[key] ?? 0) >= frames) key: value - (none[key] ?? 0),
    };
  };
}
