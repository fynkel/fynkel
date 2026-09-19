// The controller's state machine against a mocked `fynkel_flutter/viewport`
// channel. The Rust side is real (the native-assets hook builds it for
// `flutter test`): a viewport is created headlessly on the host GPU, so
// this also exercises create → register → resize/rebind → detach ordering.

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('fynkel_flutter/viewport');
  final calls = <MethodCall>[];
  var nextId = 7;

  setUp(() {
    calls.clear();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      calls.add(call);
      final args = (call.arguments as Map?)?.cast<String, Object?>() ?? {};
      switch (call.method) {
        case 'registerTexture':
          expect(args['ctx'], isA<int>());
          expect(args['acquireFn'], isA<int>());
          expect(args['setCallbackFn'], isA<int>());
          expect(args['iosurfaces'], isA<List>());
          return nextId++;
        case 'rebindTexture':
          expect(args['textureId'], isA<int>());
          expect((args['iosurfaces'] as List), isNotEmpty);
          return null;
        case 'stats':
          return {'copyCalls': 3, 'copyWithNewFrame': 2, 'copyNoFrame': 1, 'frameReadySignals': 2, 'rebinds': 0};
        case 'unregisterTexture':
          return null;
      }
      throw MissingPluginException(call.method);
    });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null);
  });

  Future<void> settle() async {
    for (var i = 0; i < 10; i++) {
      await Future<void>.delayed(Duration.zero);
    }
  }

  test('abi version matches the native library', () {
    expect(viewportAbiVersion, 2);
  });

  test('create, render, resize, stats, detach', () async {
    final controller = FynkelViewportController(bufferCount: 3, placeholders: false);
    addTearDown(controller.dispose);
    var notified = 0;
    controller.addListener(() => notified++);
    expect(controller.state, ViewportState.detached);
    expect(controller.textureId, isNull);

    controller.requestSize(64, 48);
    expect(controller.state, ViewportState.creating);
    await settle();
    expect(controller.state, ViewportState.attached, reason: '${controller.error}');
    expect(controller.textureId, 7);
    expect(controller.physicalWidth, 64);
    expect(controller.physicalHeight, 48);
    expect(calls.map((c) => c.method), ['registerTexture']);
    expect(notified, 1);

    final world = World.create();
    addTearDown(world.dispose);
    world.frame(1 / 60);
    final slot = controller.render(world);
    expect(slot, isNotNull);
    expect(slot, greaterThanOrEqualTo(0));
    expect(controller.render(world), isNot(renderNoWorld));

    // Same size → no resize; new size → native resize + rebind.
    controller.requestSize(64, 48);
    await settle();
    expect(calls.length, 1);
    controller.requestSize(128, 96);
    expect(controller.state, ViewportState.resizing);
    expect(controller.render(world), isNull, reason: 'no render mid-resize');
    await settle();
    expect(controller.state, ViewportState.attached);
    expect(controller.physicalWidth, 128);
    expect(controller.resizes, 1);
    expect(calls.last.method, 'rebindTexture');
    expect((calls.last.arguments as Map)['textureId'], 7);

    final stats = controller.takeStats();
    expect(stats, isNotNull);
    expect(stats!.framesRendered, greaterThanOrEqualTo(1));
    final platform = await controller.platformStats();
    expect(platform!.copyCalls, 3);

    await controller.detach();
    expect(controller.state, ViewportState.detached);
    expect(controller.textureId, isNull);
    expect(calls.last.method, 'unregisterTexture');
    expect((calls.last.arguments as Map)['textureId'], 7);
    expect(controller.render(world), isNull);
    expect(controller.takeStats(), isNull);

    // Reusable: the next size creates a fresh ring with a fresh texture id.
    controller.requestSize(32, 32);
    await settle();
    expect(controller.state, ViewportState.attached);
    expect(controller.textureId, 8);
    await controller.detach();
  });

  test('detach during create finishes the teardown', () async {
    final controller = FynkelViewportController();
    addTearDown(controller.dispose);
    controller.requestSize(16, 16);
    expect(controller.state, ViewportState.creating);
    await controller.detach();
    expect(controller.state, ViewportState.detached);
    await settle();
    expect(controller.textureId, isNull);
    expect(calls.map((c) => c.method), ['registerTexture', 'unregisterTexture']);
  });

  test('a failed registration leaves the controller failed with the error', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      throw PlatformException(code: 'texture', message: 'could not wrap IOSurfaces');
    });
    final controller = FynkelViewportController();
    addTearDown(controller.dispose);
    controller.requestSize(16, 16);
    await settle();
    expect(controller.state, ViewportState.failed);
    expect(controller.error, isA<PlatformException>());
    expect(controller.textureId, isNull);
    // Further size requests are ignored until detach() resets it.
    controller.requestSize(32, 32);
    expect(controller.state, ViewportState.failed);
    await controller.detach();
    expect(controller.state, ViewportState.detached);
  });

  test('percentiles', () {
    expect(MsPercentiles.of([]).count, 0);
    final p = MsPercentiles.of([5, 1, 3, 2, 4]);
    expect(p.p50, 3);
    expect(p.p99, 5);
    expect(p.count, 5);
    expect(p.format(1), 'p50=3.0 p95=5.0 p99=5.0');
  });
}
