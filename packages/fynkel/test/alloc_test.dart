// A frame without input allocates nothing (docs/stage-4.md §3.6, step 5),
// counted by the VM service's allocation profile. One scenario only: in
// the JIT one scenario's code can box doubles in the next one's.

import 'dart:developer';
import 'dart:isolate' as isolates;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';
import 'package:vm_service/vm_service.dart';
import 'package:vm_service/vm_service_io.dart';

// A constant: `1 / 60` written in the loop is worked out, and boxed, at
// each call there.
const _dt = 1 / 60;

class _Still extends GameObject {
  _Still() : super(components: [MeshRenderer(Mesh.cube(), Material())]);

  @override
  void onUpdate() {
    super.onUpdate();
  }
}

class _Idle extends Behaviour {}

void main() {
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
  Future<Map<String, int>> madeEachFrame(World world, int frames) async {
    // Warm, and compiled: the VM optimizes a function after tens of
    // thousands of calls, and until then boxes its doubles.
    for (var i = 0; i < 100000; i++) {
      world.update(_dt);
    }
    final none = await made(world, 0);
    final some = await made(world, frames);
    return {
      for (final MapEntry(:key, :value) in some.entries)
        if (value - (none[key] ?? 0) >= frames) key: value - (none[key] ?? 0),
    };
  }

  test('a frame without input allocates nothing, with the mouse resting over the view', () async {
    final world = World();
    addTearDown(world.dispose);
    world.input.resize(100, 100);
    world.add(GameObject(components: [Camera()]));
    for (var i = 0; i < 50; i++) {
      world.add(_Still()..addComponent(_Idle()));
    }
    world.input.hover(-1, 50, 50);
    world.input.onFingerHeld.listen((_) {});
    world.input.onHover.listen((_) {});

    expect(await madeEachFrame(world, 50000), isEmpty);
  });
}
