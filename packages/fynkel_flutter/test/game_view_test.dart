// `FynkelGameView` drives `World.frame` from the Ticker and renders into
// the viewport (real Rust viewport, mocked texture channel).

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
}
