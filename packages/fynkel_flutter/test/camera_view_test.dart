// 3D in the UI (ADR-0062 amendment 1, step 3): a `CameraView` shows what
// one of the world's own cameras sees, inside any widget — a minimap, a
// model preview in an inventory, a rear-view mirror. The camera is an
// ordinary scene camera with a lower priority than the main one; the
// panel draws its frame pose, so it moves as smoothly as the main picture.

import 'dart:io';

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
    cache = Directory.systemTemp.createTempSync('fynkel-flutter-camera-view-').path;
    cookProject(fixture, cacheDir: cache);
    lib = NativeLibrary.open();
  });
  tearDownAll(() {
    Directory(cache).deleteSync(recursive: true);
    lib.dispose();
  });
  setUp(() {
    var next = 40;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      channel,
      (call) async => call.method == 'registerTexture' ? next++ : null,
    );
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null);
  });

  /// Mean absolute difference per channel, 0–255.
  double diff(Uint8List a, Uint8List b) {
    var sum = 0;
    for (var i = 0; i < a.length; i++) {
      sum += (a[i] - b[i]).abs();
    }
    return sum / a.length;
  }

  testWidgets('a panel shows a second camera of the world, follows it, and leaves the game its own', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetDevicePixelRatio);
    final world = World.create(library: lib, fixedDt: 1 / 60, maxStepsPerFrame: 8);
    addTearDown(world.dispose);
    world.onLoadDiagnostics = (_) {};
    world.loadProject(fixture, cacheDir: cache);
    world.frame(1 / 60);
    // A twin of the main camera, at its pose and with its settings, one
    // priority below: the game still draws through the main one.
    // Manual exposure, so two viewports that drew a different number of
    // frames still agree.
    final main = world.find('Camera')!;
    // (EV 6 lights the fixture's floor well enough to tell it from the sky.)
    world.edit(() => main.set(main.get<Camera>()!.copyWith(exposureMode: ExposureMode.manual, exposure: 6)));
    world.frame(1 / 60);
    final settings = main.get<Camera>()!;
    final twin = world.edit(() {
      final o = world.spawn(
        name: 'Panel camera',
        transform: Pose(position: main.transform.position, rotation: main.transform.rotation),
      );
      o.set(settings.copyWith(priority: settings.priority - 1));
      return o;
    });
    world.frame(1 / 60);
    world.frame(1 / 60);

    // The game's own picture at the panel's size, for comparison.
    final game = FynkelViewportController(placeholders: false);
    addTearDown(game.dispose);
    game.requestSize(96, 64);
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 50)));
    final panel = GlobalKey<CameraViewState>();
    await tester.pumpWidget(
      MaterialApp(
        home: Align(
          alignment: Alignment.topLeft,
          child: SizedBox(
            width: 96,
            height: 64,
            child: CameraView(key: panel, world: world, camera: twin),
          ),
        ),
      ),
    );
    for (var i = 0; i < 4 && panel.currentState!.frames == 0; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 30)));
      await tester.pump(const Duration(milliseconds: 16));
    }
    final view = panel.currentState!;
    expect(view.frames, greaterThan(0), reason: 'the panel draws every frame');
    expect(view.controller.viewCamera, twin.entity);
    // Both renderers wait once for the scene's meshes and textures (a
    // frame normally draws without them until they are resident).
    final address = world.native.handleAddress;
    game.renderHandle(address, waitForAssets: true);
    view.controller.renderHandle(address, waitForAssets: true);
    await tester.pump(const Duration(milliseconds: 16));
    expect(view.controller.physicalWidth, 96);
    final mainFrame = game.readFrame(game.render(world)!)!;
    final same = view.controller.readFrame(view.lastSlot!)!;
    expect(diff(same, mainFrame), lessThan(1), reason: 'the twin sees what the main camera sees');

    // Turned to the sky: the panel follows its camera; the game does not.
    world.edit(() => twin.transform.rotation = Quat.axisAngle(Vec3.right, 1.5708) * main.transform.rotation);
    world.frame(1 / 60);
    world.frame(1 / 60);
    await tester.pump(const Duration(milliseconds: 16));
    final turned = view.controller.readFrame(view.lastSlot!)!;
    expect(diff(turned, mainFrame), greaterThan(5), reason: 'the panel looks up');
    final gameAgain = game.readFrame(game.render(world)!)!;
    expect(diff(gameAgain, mainFrame), lessThan(1), reason: 'the game still draws through the main camera');

    // The camera goes: the panel keeps its last picture, no error.
    world.edit(() => twin.destroy());
    world.frame(1 / 60);
    final before = view.frames;
    await tester.pump(const Duration(milliseconds: 16));
    expect(view.lastResult, renderNoCamera);
    expect(view.frames, before);

    // The panel takes no focus from the menu around it.
    expect(tester.widget<FynkelViewport>(find.byType(FynkelViewport)).focusNode!.canRequestFocus, isFalse);

    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });
}
