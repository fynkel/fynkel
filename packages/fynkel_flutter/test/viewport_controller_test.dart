// The controller's state machine against a mocked `fynkel_flutter/viewport`
// channel. The Rust side is real (the native-assets hook builds it for
// `flutter test`): a viewport is created headlessly on the host GPU, so
// this also exercises create → register → resize/rebind → detach ordering.

import 'dart:typed_data';

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
    expect(viewportAbiVersion, 9);
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
    // Debug lines queue for the next render (viewport ABI v4): a NaN
    // segment is dropped; the render that follows draws them (the Rust
    // test `debug_lines.rs` counts them in the frame).
    final lines = Float32List.fromList([
      0, 0, 0, 1, 0, 0, 1, 0, 0, 1, //
      0, 0, 0, double.nan, 0, 0, 1, 0, 0, 1, //
    ]);
    expect(controller.debugLines(lines), 1);
    expect(controller.debugLines(Float32List(0)), 0);
    // The overlay (viewport ABI v6): lines with a width, triangles, the
    // grid; a NaN one is dropped (the Rust test counts them in the frame).
    final overlay = ViewportOverlay()
      ..line([0, 0, 0], [1, 0, 0], [1, 0, 0, 1], width: 3)
      ..line([0, 0, 0], [double.nan, 0, 0], [1, 0, 0, 1])
      ..segments(Float32List.fromList([0, 0, 0, 0, 1, 0, 0, 1, 0, 1]), width: 2, onTop: true)
      ..triangle([0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1, 0.5])
      ..grid = const OverlayGrid();
    expect(overlay.lineCount, 3);
    expect(overlay.lines.sublist(12, 24).sublist(10), [1.5, 0]);
    expect(overlay.lines.sublist(24, 36), [0, 0, 0, 0, 1, 0, 0, 1, 0, 1, 2, 1]);
    expect(controller.overlay(overlay), 3);
    expect(controller.overlay(ViewportOverlay()), 0);
    // Past the first capacity, nothing lost.
    final many = ViewportOverlay();
    for (var i = 0; i < 200; i++) {
      many.line([i.toDouble(), 0, 0], [i.toDouble(), 1, 0], [1, 1, 1, 1]);
    }
    expect(many.lines[199 * 12], 199);
    expect(controller.overlay(many), 200);
    many.clear();
    expect(many.isEmpty, isTrue);
    // The selection outline (viewport ABI v8): entity handles, counted with
    // the rest; part of isEmpty, gone with clear; a non-finite width is
    // refused (the Rust test `debug_lines.rs` checks the picture against
    // the offscreen renderer's).
    final ringed = ViewportOverlay()..outline([1], [2, 3]);
    expect(ringed.isEmpty, isFalse);
    expect(ringed.selectionOutline!.packStyle(), [1, 0.4, 0, 1, 0.37, 0.47, 0.61, 1, 2].map((x) => closeTo(x, 1e-6)));
    expect(controller.overlay(ringed), 3);
    ringed.line([0, 0, 0], [1, 0, 0], [1, 1, 1, 1]);
    expect(controller.overlay(ringed), 4);
    ringed.outline([5], [], selectedColor: [1, 0, 0, 1], width: 4);
    expect(ringed.selectionOutline!.selected, [5]);
    expect(ringed.selectionOutline!.packStyle().sublist(8), [4]);
    expect(controller.overlay(ringed), 2);
    ringed.outline([5], [], width: double.nan);
    expect(controller.overlay(ringed), 1, reason: 'the outline refused, the line kept');
    ringed.outline([], []);
    expect(ringed.selectionOutline, isNull);
    ringed
      ..outline([1], [])
      ..clear();
    expect(ringed.isEmpty, isTrue);
    expect(ringed.selectionOutline, isNull);
    // Extra worlds (viewport ABI v9): native world addresses with a
    // transform (identity by default) and a grey flag, counted with the
    // rest; an address 0 is skipped, a non-finite transform refuses them
    // all; part of isEmpty, gone with clear (the Rust test
    // `extra_worlds.rs` checks the picture against the offscreen
    // renderer's).
    final context = World.create();
    addTearDown(context.dispose);
    context.frame(1 / 60);
    final shifted = <double>[1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 2, 0, 0, 1];
    final worlds = ViewportOverlay()
      ..extraWorld(context.native.handleAddress)
      ..extraWorld(context.native.handleAddress, transform: shifted, gray: true);
    expect(worlds.isEmpty, isFalse);
    expect(worlds.extraWorlds.map((w) => w.flags), [0, OverlayExtraWorld.grayFlag]);
    expect(worlds.packExtraTransforms(), [...OverlayExtraWorld.identity, ...shifted]);
    expect(controller.overlay(worlds), 2);
    worlds.extraWorld(0);
    expect(controller.overlay(worlds), 2, reason: 'address 0 skipped');
    worlds.extraWorld(context.native.handleAddress, transform: [...shifted]..[12] = double.nan);
    expect(controller.overlay(worlds), 0, reason: 'a non-finite transform refuses them all');
    expect(() => worlds.extraWorld(context.native.handleAddress, transform: [1, 0, 0]), throwsArgumentError);
    worlds.clear();
    expect(worlds.isEmpty, isTrue);
    expect(worlds.extraWorlds, isEmpty);
    // A viewport's own camera (viewport ABI v7): it draws, a value that
    // cannot is refused (the last good one stays), null clears; the Rust
    // test `view.rs` checks the picture.
    controller.view = const ViewportView(world: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 5, 1]);
    expect(controller.viewOk, isTrue);
    controller.view = const ViewportView(world: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 5, 1], near: 0);
    expect(controller.viewOk, isFalse);
    controller.view = const ViewportView(world: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1, 5, 1], orthographic: true);
    expect(controller.viewOk, isTrue);
    controller.view = null;
    expect(controller.viewOk, isTrue);
    final slot = controller.render(world);
    expect(slot, isNotNull);
    expect(slot, greaterThanOrEqualTo(0));
    // A world by its native address (the editor's preview world); 0 is no
    // world.
    final handleSlot = controller.renderHandle(world.native.handleAddress);
    expect(handleSlot, isNot(renderNoWorld));
    // The rendered slot's pixels come back as RGBA8 at the physical size
    // (viewport ABI v4: the editor's Save Frame); a slot the ring lacks is
    // null.
    final frame = controller.readFrame(handleSlot!);
    expect(frame, isNotNull);
    expect(frame!.length, 64 * 48 * 4);
    expect(frame.every((b) => b == 0), isFalse, reason: 'the clear colour is not black');
    expect(controller.readFrame(9), isNull);
    expect(controller.renderHandle(0), renderNoWorld);

    // Quality profiles come from the project the world's cache was cooked
    // from; this world has none, so any name is accepted and the engine
    // defaults apply. Switching is a plain property write, applied by the
    // next frame.
    expect(controller.quality, isNull);
    expect(controller.qualityOk, isTrue);
    controller.quality = 'low';
    expect(controller.quality, 'low');
    expect(controller.qualityOk, isTrue);
    expect(controller.render(world), greaterThanOrEqualTo(0));
    controller.quality = null;
    expect(controller.qualityOk, isTrue);

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
    // The frame counts the renderer reports for the last frame (engine
    // defaults: MSAA 4 at the viewport size before the resize; the demo
    // world has no scene, so no casters and nothing blended).
    expect(stats.msaa, 4);
    expect(stats.renderWidth, 64);
    expect(stats.renderHeight, 48);
    expect(stats.shadowMap, 0);
    expect(stats.transparent, 0);
    expect(stats.lightsDropped, 0);
    // M3-16: the render call's stages and the GPU memory estimate; the
    // pass timestamps are read back a few frames late, so after a couple
    // of renders only the mechanism is checked.
    expect(stats.prepare.count, stats.framesRendered);
    expect(stats.encode.count, stats.framesRendered);
    expect(stats.encode.p50, greaterThan(0));
    expect(stats.gpuBytes, greaterThan(64 * 48 * 4));
    expect(stats.gpuTimestamps, isTrue, reason: 'Metal reports stage timestamps on every supported Mac');
    expect(stats.gpuFrame.count, lessThanOrEqualTo(stats.framesRendered));
    // The demo world spawns nothing: no draws, nothing culled.
    expect(stats.draws, 0);
    expect(stats.culled, 0);
    expect(stats.shadowDraws, 0);
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
