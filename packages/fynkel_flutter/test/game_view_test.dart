// `FynkelGameView` drives `World.frame` from the Ticker and renders into
// the viewport (real Rust viewport, mocked texture channel).

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('fynkel_flutter/viewport');

  setUp(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      switch (call.method) {
        case 'registerTexture':
          return 42;
        case 'stats':
          return {'copyCalls': 1};
        default:
          return null;
      }
    });
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null);
  });

  testWidgets('ticks the world, renders, reports stats, tears down', (tester) async {
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.reset);
    final world = World.create();
    addTearDown(world.dispose);
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    final fixedTicks = <int>[];
    final frames = <FramePlan>[];
    FynkelFrameStats? last;

    await tester.pumpWidget(
      MaterialApp(
        home: Center(
          child: SizedBox(
            width: 100,
            height: 50,
            child: FynkelGameView(
              world: world,
              controller: controller,
              fixedUpdate: fixedTicks.add,
              onFrame: (dt, plan) => frames.add(plan),
              onStats: (s) => last = s,
              overlay: const Text('HUD'),
            ),
          ),
        ),
      ),
    );
    // Layout → requestSize on a microtask → native create → channel → attached.
    await tester.pump();
    await tester.pump();
    expect(controller.state, ViewportState.attached, reason: '${controller.error}');
    expect(controller.physicalWidth, 200);
    expect(controller.physicalHeight, 100);
    expect(find.byType(Texture), findsOneWidget);
    expect(find.text('HUD'), findsOneWidget);

    // 30 frames at 60 Hz → ≥ 29 fixed ticks (the first frame primes the clock).
    for (var i = 0; i < 30; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
    expect(frames.length, greaterThanOrEqualTo(30));
    expect(fixedTicks, isNotEmpty);
    expect(fixedTicks.length, world.tick);

    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    expect(last, isNotNull);
    expect(last!.tickInterval.count, greaterThan(0));
    expect(last!.frames, greaterThanOrEqualTo(last!.tickInterval.count));
    expect(last!.fps, greaterThan(0));
    expect(last!.renderCall.count, greaterThan(0));
    expect(last!.viewport!.framesRendered, greaterThan(0));
    expect(last!.platform!.copyCalls, 1);
    expect(last!.physicalWidth, 200);
    // M3-16: the frame's split, the engine's systems, Flutter's timings,
    // the GPU stages and memory.
    final s = last!;
    expect(s.behaviours.count, s.simulate.count);
    expect(s.nativeStep.count, s.simulate.count);
    expect(s.nativeStep.p50, greaterThan(0));
    expect(s.nativeStep.p95, lessThanOrEqualTo(s.simulate.p95));
    expect(s.systems, isNotEmpty);
    expect(s.systems.first.system, SystemKind.step);
    expect(s.totalWork.p95, greaterThanOrEqualTo(s.simulate.p95));
    expect(s.viewport!.extract.count, greaterThan(0));
    expect(s.viewport!.prepare.count, greaterThan(0));
    expect(s.viewport!.gpuBytes, greaterThan(0));
    expect(s.world.nativeBytes, greaterThan(0));
    expect(s.world.residentBytes, greaterThan(0));
    // The widget tester renders no real Flutter frames, so its timings are
    // empty; the GPU stamps trail the draw and may not have landed yet.
    expect(s.flutterBuild.count, greaterThanOrEqualTo(0));
    expect(s.viewport!.gpuFrame.count, lessThanOrEqualTo(s.viewport!.framesRendered));
    // The capture line: every key once, the timers as p50/p95/p99, the
    // viewport's counts, no Dart heap (the test runner has no probe).
    final line = s.summary();
    final keys = line.split(' ').map((kv) => kv.split('=').first).toList();
    expect(keys.toSet().length, keys.length);
    expect(line, contains('frames=${s.frames} '));
    expect(line, contains('entities=${world.stats().liveEntities} '));
    expect(line, contains('work=${s.totalWork.p50.toStringAsFixed(3)}/'));
    expect(line, contains('instances=${s.viewport!.instances} '));
    expect(line, contains('mem_gpu=${s.viewport!.gpuBytes}'));
    expect(line, isNot(contains('heap=')));

    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(controller.state, ViewportState.detached);
  });

  testWidgets('audio: true opens the device (or reports none), suspends with the lifecycle and closes', (tester) async {
    final world = World.create();
    addTearDown(world.dispose);
    world.throwOnDiagnostics = false;
    final seen = <FynkelDiagnostic>[];
    world.onDiagnostics = seen.addAll;
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    expect(world.audio.isOpen, isFalse);

    await tester.pumpWidget(
      MaterialApp(home: FynkelGameView(world: world, controller: controller, audio: true)),
    );
    await tester.pump();
    // A machine with an output device opens it; one without reports
    // `audio_device` and stays on the null backend. Either way the world
    // keeps ticking.
    final opened = world.audio.isOpen;
    expect(world.audio.stats.device, opened);
    if (!opened) expect(seen.map((d) => d.code), contains(DiagnosticCode.audioDevice));
    await tester.pump(const Duration(microseconds: 16667));
    await tester.pump(const Duration(microseconds: 16667));
    expect(world.tick, greaterThan(0));

    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await tester.pump();
    expect(world.audio.stats.suspended, opened);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(world.audio.stats.suspended, isFalse);

    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(world.audio.isOpen, isFalse, reason: 'the view closed what it opened');
    expect(world.audio.stats.device, isFalse);
  });

  testWidgets('a debounced viewport resizes once when a drag settles, a game on every frame', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    Future<int> resizesWhileDragging({required Duration debounce}) async {
      final controller = FynkelViewportController(placeholders: false);
      addTearDown(controller.dispose);
      var side = 100.0;
      late StateSetter setSide;
      await tester.pumpWidget(
        MaterialApp(
          home: Center(
            child: StatefulBuilder(
              builder: (context, setState) {
                setSide = setState;
                return SizedBox(
                  width: side,
                  height: side,
                  child: FynkelViewport(controller: controller, resizeDebounce: debounce),
                );
              },
            ),
          ),
        ),
      );
      await tester.pump();
      await tester.pump();
      expect(controller.isAttached, isTrue, reason: 'the first size is never held back');
      expect(controller.physicalWidth, 100);

      // The drag: a new size on every frame, none of them final.
      for (var i = 0; i < 8; i++) {
        setSide(() => side += 10);
        await tester.pump(const Duration(milliseconds: 16));
      }
      await tester.pump(const Duration(milliseconds: 200)); // the drag settles
      await tester.pumpAndSettle();
      expect(controller.physicalWidth, 180, reason: 'it ends at the size the drag left');
      final n = controller.resizes;
      await tester.pumpWidget(const SizedBox());
      return n;
    }

    // A game: its viewport is the window and it draws every frame anyway.
    expect(await resizesWhileDragging(debounce: Duration.zero), greaterThan(1));
    // The editor: one resize, and the last frame is held (scaled) until then.
    expect(await resizesWhileDragging(debounce: const Duration(milliseconds: 120)), 1);
  });

  testWidgets('shows the placeholder until attached and the error on failure', (tester) async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      throw PlatformException(code: 'texture', message: 'nope');
    });
    final controller = FynkelViewportController();
    addTearDown(controller.dispose);
    await tester.pumpWidget(MaterialApp(home: FynkelViewport(controller: controller)));
    expect(find.byType(Texture), findsNothing);
    await tester.pump();
    await tester.pump();
    expect(controller.state, ViewportState.failed);
    expect(find.textContaining('Viewport failed'), findsOneWidget);
  });

  testWidgets('forwards a trackpad\'s pan-zoom gesture, not only the wheel', (tester) async {
    final controller = FynkelViewportController();
    addTearDown(controller.dispose);
    final seen = <String>[];
    await tester.pumpWidget(
      MaterialApp(
        home: FynkelViewport(controller: controller, onPointerEvent: (e, _) => seen.add(_panZoomPhase(e))),
      ),
    );
    final pad = TestPointer(1, PointerDeviceKind.trackpad);
    final at = tester.getCenter(find.byType(FynkelViewport));
    await tester.sendEventToBinding(pad.panZoomStart(at));
    await tester.sendEventToBinding(pad.panZoomUpdate(at, pan: const Offset(0, 12), scale: 1.2));
    await tester.sendEventToBinding(pad.panZoomEnd());
    expect(seen, ['start', 'update', 'end']);
  });
}

String _panZoomPhase(PointerEvent e) => switch (e) {
  PointerPanZoomStartEvent() => 'start',
  PointerPanZoomUpdateEvent() => 'update',
  PointerPanZoomEndEvent() => 'end',
  _ => '${e.runtimeType}',
};
