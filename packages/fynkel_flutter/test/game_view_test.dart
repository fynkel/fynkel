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

    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(controller.state, ViewportState.detached);
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
