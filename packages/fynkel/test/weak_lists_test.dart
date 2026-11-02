// Stage 14's DF13: a shader keeps a weak reference to each screen effect
// made with it, and an effect one to each camera it's in. Those of effects
// and cameras gone are let go as more are added, not kept for good: a game
// making an effect for each hit doesn't grow the list without end.

import 'dart:developer';
import 'dart:isolate' as isolates;

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';
import 'package:vm_service/vm_service.dart';
import 'package:vm_service/vm_service_io.dart';

void main() {
  late VmService service;
  late String isolate;
  setUpAll(() async {
    final info = await Service.controlWebServer(enable: true, silenceOutput: true);
    service = await vmServiceConnectUri(info.serverWebSocketUri.toString());
    isolate = Service.getIsolateId(isolates.Isolate.current)!;
  });
  tearDownAll(() => service.dispose());

  /// How many weak references the isolate holds, after collecting.
  Future<int> weakReferences() async {
    await service.getAllocationProfile(isolate, gc: true);
    final classes = await service.getClassList(isolate);
    var total = 0;
    for (final c in classes.classes!) {
      if (c.name == '_WeakReference') {
        total += (await service.getInstances(isolate, c.id!, 0)).totalCount!;
      }
    }
    return total;
  }

  test('a shader and an effect let go of the effects and cameras gone', () async {
    final shader = Shader(
      'fn effect(in: EffectInput) -> vec4<f32> {\n    return vec4<f32>(1.0 - picture(in.uv).rgb, 1.0);\n}\n',
      name: 'invert.wgsl',
    );
    addTearDown(shader.dispose);
    final kept = ScreenEffect(shader);
    final before = await weakReferences();
    // Made over time, collected now and then, as a game's would be.
    for (var round = 0; round < 20; round++) {
      for (var i = 0; i < 1000; i++) {
        ScreenEffect(shader);
        Camera().effects = [kept];
      }
      await service.getAllocationProfile(isolate, gc: true);
    }
    final after = await weakReferences();
    expect(after - before, lessThan(5000), reason: '$before weak references before, $after after');
  });
}
