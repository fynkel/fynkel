// Screen ↔ world helpers on the game view (ADR-0062 step 2): a position in
// the view (a tap's `localPosition`) to a ray and a pick, a world point to
// a position in the view — in the view's own logical pixels, from the
// frame's camera.

import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/material.dart' hide Transform;
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/advanced.dart';
import 'package:fynkel/testing.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('fynkel_flutter/viewport');
  final fixture = Directory('../../crates/fynkel_document/fixtures/project').absolute.path;
  late String cache;
  late NativeLibrary lib;

  setUpAll(() {
    cache = Directory.systemTemp.createTempSync('fynkel-flutter-screen-world-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });
  setUp(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      channel,
      (call) async => call.method == 'registerTexture' ? 42 : null,
    );
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null);
  });

  /// The fixture's camera at (0, 2, 6), fov 1 rad, looking down -Z, in a
  /// `width` × `height` view.
  Offset expected(Vec3 p, double width, double height) {
    final depth = 6 - p.z;
    final halfH = depth * math.tan(0.5);
    final halfW = halfH * width / height;
    return Offset((p.x / halfW * 0.5 + 0.5) * width, (0.5 - (p.y - 2) / halfH * 0.5) * height);
  }

  testWidgets('a tap position picks what it shows; a world point lands in the view', (tester) async {
    final world = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    world.loadProject(fixture, cacheDir: cache);
    world.frame(1 / 60);
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    final view = GlobalKey<FynkelGameViewState>();
    await tester.pumpWidget(
      MaterialApp(
        home: Align(
          alignment: Alignment.topLeft,
          child: SizedBox(
            width: 400,
            height: 300,
            child: FynkelGameView(key: view, world: world, controller: controller),
          ),
        ),
      ),
    );
    await tester.pump();
    final state = view.currentState!;

    // World → view: where the floor point is drawn in this 400 × 300 view.
    const ground = Vec3(1.5, 0, 1);
    final at = state.worldToScreen(ground)!;
    final want = expected(ground, 400, 300);
    expect(at.onScreen, isTrue);
    expect(at.offset.dx, closeTo(want.dx, 0.01));
    expect(at.offset.dy, closeTo(want.dy, 0.01));

    // View → world: the ray at that position runs through the point, and
    // picking there hits the Floor at it.
    final ray = state.screenRay(at.offset)!;
    final along = (ground - ray.origin).dot(ray.direction);
    expect((ray.at(along) - ground).length, lessThan(1e-3));
    final hit = state.pick(at.offset)!;
    expect(hit.entity, world.find('Floor')!.entity);
    expect((hit.point - ground).length, lessThan(1e-2));
    // The sky: nothing.
    expect(state.pick(const Offset(200, 1)), isNull);

    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });
}
