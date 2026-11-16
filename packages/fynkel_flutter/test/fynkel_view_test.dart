import 'dart:async';
import 'dart:ffi' show AllocatorAlloc, Pointer, StructPointer;
import 'dart:io';
import 'dart:math' as math;

import 'package:ffi/ffi.dart';
import 'package:flutter/services.dart';
import 'package:flutter/material.dart' hide Color, Material, Transform;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/src/fynkel_view.dart' show channel, debugAndroid, debugIos, debugViewsOpen, forgetReset;
import 'package:fynkel_flutter/src/error_panel.dart' show ErrorPanel;
import 'package:fynkel_flutter/src/world_widget.dart' show WorldWidgetLayer, worldWidgetsOf;

/// Whether the engine view at [address] is still alive and answering.
bool alive(int address) {
  final stats = malloc<FyViewStats>();
  try {
    return fy_view_stats(Pointer.fromAddress(address), stats) == FyStatus.FY_STATUS_OK;
  } finally {
    malloc.free(stats);
  }
}

/// The app's bundle, as far as the tests need one: [files] by key, each
/// read from the middle of a bigger buffer, as a real bundle may.
class Bundle extends CachingAssetBundle {
  Bundle(this.files);
  final Map<String, List<int>> files;
  final asked = <String>[];

  @override
  Future<ByteData> load(String key) async {
    asked.add(key);
    // Later, as a real bundle answers.
    await Future<void>.delayed(Duration.zero);
    final bytes = files[key];
    if (bytes == null) throw FlutterError('no asset $key');
    final padded = Uint8List(bytes.length + 8)..setAll(4, bytes);
    return ByteData.view(padded.buffer, 4, bytes.length);
  }
}

/// The smallest game: a camera, a texture from the bundle, and a log of
/// what it was told.
/// Notes the world's theme as it loads, and every change after.
class Themed extends Tiny {
  final heard = <Brightness>[];
  Brightness? atLoad;

  @override
  Future<void> onLoad() {
    atLoad = world.theme.brightness;
    on(world.onThemeChanged, (theme) => heard.add(theme.brightness));
    return super.onLoad();
  }
}

class Tiny extends GameObject {
  final log = <String>[];
  Texture2D? texture;

  @override
  Future<void> onLoad() async {
    log.add('load');
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 4)),
        components: [Camera()],
      ),
    );
    texture = await assets.texture('grid.png');
    await super.onLoad();
  }

  @override
  void onStart() {
    log.add('start');
    super.onStart();
  }

  @override
  void onUpdate() {
    if (log.last != 'update') log.add('update');
    super.onUpdate();
  }
}

/// The slip of the first game written with Fynkel 2: a point where
/// `Quat.lookRotation` takes a direction.
class LooksAtAPoint extends GameObject {
  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 1, 4), rotation: Quat.lookRotation(Vec3.zero)),
        components: [Camera()],
      ),
    );
  }
}

/// Throws from onUpdate every frame, counting.
class ThrowsInUpdate extends GameObject {
  var count = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    throw StateError('throw ${++count}');
  }
}

void main() {
  late World world;
  // The engine view behind each texture.
  final views = <int, int>{};
  late List<String> calls;
  // How many calls this test had made at each `reset`.
  late List<int> resets;
  // Whether the plugin was ever reset in this isolate.
  var everReset = false;
  late int nextTexture;
  // When set, `register` answers only once it completes.
  Completer<void>? hold;
  // Whether the plugin's unregister fails, as it does for a texture it
  // already let go of.
  late bool unregisterFails;

  setUp(() {
    // Every pump a frame of the world's: the frame rate has tests of its own.
    world = World()..targetFrameRate = 0;
    world.add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 4)),
        components: [Camera()],
      ),
    );
    calls = [];
    resets = [];
    nextTexture = 7;
    hold = null;
    unregisterFails = false;
    // The Swift or Kotlin plugin, as far as Dart can see it.
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      final args = call.arguments as Map? ?? const {};
      switch (call.method) {
        case 'register':
          if (debugAndroid ?? false) {
            // Asking to be on screen (FynkelView.onScreen), with where
            // it is; this stand-in answers with a texture, as the plugin
            // does in an activity that isn't a FynkelActivity.
            expect(
              args.keys,
              unorderedEquals(['view', 'width', 'height', 'onScreen', 'left', 'top', 'frameWidth', 'frameHeight']),
            );
            calls.add('register ${args['width']}x${args['height']}');
          } else {
            expect(args['acquire'], isNot(0));
            expect(args['setFrameReady'], isNot(0));
            expect(args['free'], isNot(0));
            calls.add('register');
          }
          expect(everReset, isTrue, reason: 'the plugin is reset before the first register');
          final view = args['view'] as int;
          expect(alive(view), isTrue);
          await hold?.future;
          views[nextTexture] = view;
          return nextTexture++;
        case 'reset':
          resets.add(calls.length);
          everReset = true;
          return null;
        case 'unregister':
          final texture = args['texture'] as int;
          expect(alive(views[texture]!), isTrue, reason: 'the view is freed only after this');
          calls.add('unregister $texture');
          if (unregisterFails) throw PlatformException(code: 'gone', message: 'no such texture');
          return null;
        case 'resize':
          final texture = args['texture'] as int;
          expect(views, contains(texture), reason: 'only a registered texture is resized');
          calls.add('resize $texture ${args['width']}x${args['height']}');
          return null;
      }
      throw MissingPluginException();
    });
  });
  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null);
    world.dispose();
  });

  // Lets [future] finish: its real work (the image decoded on another
  // isolate) in real time, and what follows it in the test's own time.
  Future<void> finish(WidgetTester tester, Future<void> future) async {
    var done = false;
    unawaited(future.whenComplete(() => done = true).catchError((_) {}));
    for (var i = 0; i < 400 && !done; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 5)));
      await tester.pump(const Duration(milliseconds: 1));
    }
    expect(done, isTrue, reason: 'it finished');
  }

  /// A hot reload, as far as the widgets see one: each is reassembled,
  /// then built again.
  Future<void> reload(WidgetTester tester) async {
    tester.binding.buildOwner!.reassemble(tester.binding.rootElement!);
    await tester.pump();
  }

  Widget sized(Widget child, {double width = 100, double height = 50}) => Directionality(
    textDirection: TextDirection.ltr,
    child: Center(
      child: SizedBox(width: width, height: height, child: child),
    ),
  );

  testWidgets('draws at the physical size times the scale, calls onFrame, and shows the texture', (tester) async {
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.resetDevicePixelRatio);
    final dts = <double>[];
    final stats = <FynkelViewStats>[];
    await tester.pumpWidget(sized(FynkelView.world(world, renderScale: 0.5, onFrame: dts.add, onStats: stats.add)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, hasLength(1));
    expect(find.byType(Texture), findsOneWidget);
    expect(tester.widget<Texture>(find.byType(Texture)).textureId, 7);

    for (var i = 0; i < 300; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(dts.first, 0);
    expect(dts.skip(1), everyElement(closeTo(0.008, 1e-9)));
    expect(stats, isNotEmpty);
    expect(stats.last.size, const Size(100, 50), reason: '100 × 50 logical, × 2 pixels, × 0.5');
    expect(stats.last.view.posted, stats.last.ticks, reason: 'one engine frame per tick');
    expect(stats.last.tickFps, closeTo(125, 1));

    await tester.pumpWidget(sized(FynkelView.world(world, onStats: stats.add), width: 60, height: 30));
    for (var i = 0; i < 300; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(stats.last.size, const Size(120, 60), reason: 'resized, at scale 1 now');
    expect(calls, hasLength(1), reason: 'a resize keeps the texture');
  });

  testWidgets("the world's quality's renderScale sets the size drawn at, from the next frames, without a new layout", (
    tester,
  ) async {
    tester.view.devicePixelRatio = 2;
    addTearDown(tester.view.resetDevicePixelRatio);
    final stats = <FynkelViewStats>[];
    await tester.pumpWidget(sized(FynkelView.world(world, renderScale: 0.5, onStats: stats.add)));
    for (var i = 0; i < 300; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(stats.last.size, const Size(100, 50));
    world.quality = Quality.high.copyWith(renderScale: 0.75);
    for (var i = 0; i < 300; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(stats.last.size, const Size(75, 38), reason: "100 × 50 at the view's 0.5, × 0.75");
    expect(calls, hasLength(1), reason: 'the same texture');
    expect(() => world.quality = Quality.low.copyWith(renderScale: 0), throwsArgumentError);
    expect(world.quality?.renderScale, 0.75);
  });

  group('the frame rate', () {
    // A 120 Hz display's frames.
    const display = Duration(microseconds: 8333);

    testWidgets('is 60 unless set: on a 120 Hz display, every other frame runs the world and is drawn', (tester) async {
      final own = World();
      addTearDown(own.dispose);
      expect(own.targetFrameRate, 60);
      final dts = <double>[];
      final stats = <FynkelViewStats>[];
      await tester.pumpWidget(sized(FynkelView.world(own, onFrame: dts.add, onStats: stats.add)));
      for (var i = 0; i < 360; i++) {
        await tester.pump(display);
      }
      expect(dts, hasLength(180), reason: 'every other of 360, from the first');
      expect(dts.skip(1), everyElement(closeTo(1 / 60, 1e-5)));
      expect(own.time.sinceStart, closeTo(358 * 0.008333, 1e-6), reason: "the world's clock is the display's");
      expect(stats.last.tickFps, closeTo(60, 1));
      expect(stats.last.view.posted, stats.last.ticks, reason: 'one engine frame per frame of the world');
    });

    testWidgets('0 runs every display frame, and a game can change it at any time', (tester) async {
      final dts = <double>[];
      world.targetFrameRate = 60;
      await tester.pumpWidget(sized(FynkelView.world(world, onFrame: dts.add)));
      for (var i = 0; i < 120; i++) {
        await tester.pump(display);
      }
      expect(dts, hasLength(60));
      world.targetFrameRate = 0;
      for (var i = 0; i < 120; i++) {
        await tester.pump(display);
      }
      expect(dts, hasLength(180));
      world.targetFrameRate = 30;
      for (var i = 0; i < 120; i++) {
        await tester.pump(display);
      }
      expect(dts, hasLength(210), reason: 'every fourth');
      expect(() => world.targetFrameRate = -1, throwsArgumentError);
      expect(world.targetFrameRate, 30);
    });

    testWidgets("every view of a world draws in the world's frames, and only then", (tester) async {
      world.targetFrameRate = 60;
      final left = <double>[], right = <double>[];
      await tester.pumpWidget(
        sized(
          Row(
            children: [
              Expanded(child: FynkelView.world(world, onFrame: left.add)),
              Expanded(child: FynkelView.world(world, onFrame: right.add, input: false)),
            ],
          ),
        ),
      );
      for (var i = 0; i < 240; i++) {
        await tester.pump(display);
        expect(right, hasLength(left.length), reason: 'display frame $i');
      }
      expect(left, hasLength(120));
      expect(world.time.sinceStart, closeTo(238 * 0.008333, 1e-6), reason: 'updated once a frame, not once a view');
    });
  });

  testWidgets("after each frame it posts, onViewStats has the view's counts so far, so a load can be timed", (
    tester,
  ) async {
    final seen = <ViewStats>[];
    final ticks = <int>[];
    await tester.pumpWidget(
      sized(FynkelView.world(world, onFrame: (_) => ticks.add(seen.length), onViewStats: seen.add)),
    );
    for (var i = 0; i < 20; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(seen, isNotEmpty);
    expect([for (final s in seen) s.posted], [for (var i = 1; i <= seen.length; i++) i], reason: 'one per frame');
    expect(ticks.length, greaterThan(seen.length), reason: 'none before the texture is there');

    // A textured cube: the frame that draws it holds the texture's bytes.
    final before = seen.last.textureBytes;
    final pixels = Texture2D.rgba(4, 4, Uint8List(64));
    world.add(
      GameObject(
        transform: Transform(),
        components: [MeshRenderer(Mesh.cube(), Material(baseColorTexture: pixels))],
      ),
    );
    final from = seen.length;
    for (var i = 0; i < 200 && seen.last.textureBytes == before; i++) {
      await tester.pump(const Duration(milliseconds: 8));
      // The render worker runs on its own thread, in real time.
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 2)));
    }
    // 4 × 4 RGBA and its mipmaps, 2 × 2 and 1 × 1, at 4 bytes a texel.
    expect(seen.last.textureBytes - before, (16 + 4 + 1) * 4);
    expect(seen.length, greaterThan(from));
  });

  testWidgets('the plugin lets go of the view before it is freed', (tester) async {
    await tester.pumpWidget(sized(FynkelView.world(world)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(find.byType(Texture), findsOneWidget);
    await tester.pumpWidget(sized(const SizedBox()));
    await tester.pump();
    expect(calls, ['register', 'unregister 7']);
  });

  testWidgets('a view is freed even when the plugin fails to let go of it', (tester) async {
    unregisterFails = true;
    await tester.pumpWidget(sized(FynkelView.world(world)));
    await tester.pump(const Duration(milliseconds: 8));
    final open = debugViewsOpen;
    await tester.pumpWidget(sized(const SizedBox()));
    await tester.pump();
    expect(calls, ['register', 'unregister 7']);
    expect(debugViewsOpen, open - 1, reason: 'freed all the same');
    expect(tester.takeException().toString(), contains('no such texture'), reason: 'and reported');
  });

  testWidgets('removed while its texture is being registered, the view is let go once that finishes', (tester) async {
    final held = hold = Completer<void>();
    await tester.pumpWidget(sized(FynkelView.world(world)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, ['register']);
    await tester.pumpWidget(sized(const SizedBox()));
    await tester.pump();
    expect(calls, ['register'], reason: 'still waiting for the texture');
    held.complete();
    await tester.pump();
    expect(calls, ['register', 'unregister 7']);
  });

  testWidgets('a new world gets a new view', (tester) async {
    await tester.pumpWidget(sized(FynkelView.world(world)));
    await tester.pump(const Duration(milliseconds: 8));
    final other = World();
    addTearDown(other.dispose);
    await tester.pumpWidget(sized(FynkelView.world(other)));
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, ['register', 'unregister 7', 'register']);
    expect(tester.widget<Texture>(find.byType(Texture)).textureId, 8);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('the app going to the background and back sets the world\'s applicationPaused', (tester) async {
    await tester.pumpWidget(sized(FynkelView.world(world)));
    await tester.pump(const Duration(milliseconds: 8));
    final binding = tester.binding;
    expect(world.applicationPaused, isFalse);
    binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    expect(world.applicationPaused, isFalse, reason: 'inactive is on the way');
    binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    expect(world.applicationPaused, isTrue);
    binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    expect(world.applicationPaused, isTrue, reason: 'coming back');
    binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    expect(world.applicationPaused, isFalse);
    await tester.pumpWidget(const SizedBox());
  });

  // A game whose view is gone for now (a loading page, a shop pushed in its
  // place) still hears the app go: its music stops and onApplicationPause
  // saves (playtest audit).
  testWidgets("a FynkelGame showing no view hears the app go away and come back", (tester) async {
    final binding = tester.binding;
    await tester.pumpWidget(FynkelGame(Tiny.new, builder: (context, game) => const SizedBox()));
    final world = tester.state<FynkelGameState<Tiny>>(find.byType(FynkelGame<Tiny>)).world;
    expect(world.applicationPaused, isFalse);
    binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    expect(world.applicationPaused, isTrue);
    binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    expect(world.applicationPaused, isFalse);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('a memory warning tells the world once, and views let go of their images only while away', (
    tester,
  ) async {
    world.add(
      GameObject(
        transform: Transform(rotation: Quat.lookRotation(Vec3(0.6, -1, -0.5))),
        components: [Light.sun(shadows: true)],
      ),
    );
    world.add(GameObject(components: [MeshRenderer(Mesh.cube(), Material())..castShadows = true]));
    var told = 0;
    world.onLowMemory.listen((_) => told++);
    final seen = <ViewStats>[];
    await tester.pumpWidget(
      sized(
        Row(
          children: [
            Expanded(child: FynkelView.world(world, onViewStats: seen.add)),
            Expanded(child: FynkelView.world(world, input: false)),
          ],
        ),
      ),
    );
    for (var i = 0; i < 20; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    int held() {
      final stats = malloc<FyViewStats>();
      try {
        var sum = 0;
        for (final view in views.values) {
          expect(fy_view_stats(Pointer.fromAddress(view), stats), FyStatus.FY_STATUS_OK);
          sum += stats.ref.shadow_bytes;
        }
        return sum;
      } finally {
        malloc.free(stats);
      }
    }

    await tester.runAsync(() async {
      while (held() == 0) {
        await Future<void>.delayed(const Duration(milliseconds: 1));
      }
    });
    final drawing = held();

    final binding = tester.binding;
    binding.handleMemoryPressure();
    await tester.idle();
    expect(told, 1, reason: 'one warning, whatever the number of views');
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 50)));
    expect(held(), drawing, reason: 'drawing, they would only be made again');

    binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
    binding.handleMemoryPressure();
    await tester.idle();
    expect(told, 2, reason: 'the next warning is a warning of its own');
    await tester.runAsync(() async {
      while (held() != 0) {
        await Future<void>.delayed(const Duration(milliseconds: 1));
      }
    });
    binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpWidget(const SizedBox());
  });

  // A hot restart starts a new isolate and disposes nothing, so the first
  // view of each isolate has the plugin free what an earlier one left.
  testWidgets('the plugin is reset once per isolate, before its first view registers', (tester) async {
    forgetReset();
    await tester.pumpWidget(sized(FynkelView.world(world)));
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pumpWidget(sized(const SizedBox()));
    final other = World();
    addTearDown(other.dispose);
    await tester.pumpWidget(sized(FynkelView.world(other)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, ['register', 'unregister 7', 'register']);
    expect(resets, [0]);
    await tester.pumpWidget(sized(const SizedBox()));
  });

  testWidgets('a game resets the plugin as it starts, before it shows any view', (tester) async {
    forgetReset();
    await tester.pumpWidget(FynkelGame(Tiny.new, builder: (context, game) => const SizedBox()));
    await tester.pump();
    expect(resets, [0], reason: "an earlier isolate's views are freed though this game shows none yet");
    await tester.pumpWidget(sized(FynkelView.world(world)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, ['register']);
    expect(resets, [0], reason: 'once per isolate');
    await tester.pumpWidget(sized(const SizedBox()));
  });

  // On Android a view laid out before its window has a size would get a
  // surface of its own (Flutter makes it 1 × 1), only to swap it at once.
  testWidgets('an empty layout makes no view until there is a size', (tester) async {
    await tester.pumpWidget(sized(FynkelView.world(world), width: 0));
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pumpWidget(sized(FynkelView.world(world), height: 0));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, isEmpty);
    await tester.pumpWidget(sized(FynkelView.world(world)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, ['register']);
  });

  group('the Android way', () {
    setUp(() => debugAndroid = true);
    tearDown(() => debugAndroid = null);

    // There a new size is a new surface, which only the plugin can make.
    testWidgets('registers with a size and sends each new size, once, after the texture is there', (tester) async {
      final held = hold = Completer<void>();
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pumpWidget(sized(FynkelView.world(world), width: 60, height: 40));
      await tester.pump(const Duration(milliseconds: 8));
      expect(calls, ['register 300x150'], reason: 'nothing to resize before the texture (3 pixels per point)');
      held.complete();
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pump(const Duration(milliseconds: 8));
      expect(calls, ['register 300x150', 'resize 7 180x120']);

      for (var i = 0; i < 10; i++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
      await tester.pumpWidget(sized(FynkelView.world(world), width: 50, height: 100));
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pump(const Duration(milliseconds: 8));
      expect(calls, ['register 300x150', 'resize 7 180x120'], reason: 'a new size waits until it holds still');
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
      expect(calls, ['register 300x150', 'resize 7 180x120', 'resize 7 150x300', 'unregister 7']);
    });

    testWidgets('a drag that changes the size every frame sends only the size it ends at', (tester) async {
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pump(const Duration(milliseconds: 8));
      expect(calls, ['register 300x150']);
      for (var width = 101; width <= 120; width++) {
        await tester.pumpWidget(sized(FynkelView.world(world), width: width.toDouble(), height: 50));
        await tester.pump(const Duration(milliseconds: 8));
      }
      expect(calls, ['register 300x150'], reason: 'one new surface per drag, not one a frame');
      for (var i = 0; i < 3; i++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
      expect(calls, ['register 300x150', 'resize 7 360x150']);
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
    });

    testWidgets('a failed resize brings a new view at the new size, and the frames go on', (tester) async {
      var failures = 1;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
        final args = call.arguments as Map? ?? const {};
        switch (call.method) {
          case 'register':
            calls.add('register ${args['width']}x${args['height']}');
            return nextTexture++;
          case 'resize':
            calls.add('resize ${args['texture']}');
            if (failures-- > 0) throw PlatformException(code: 'attach', message: 'no RGBA8 format');
          case 'unregister':
            calls.add('unregister ${args['texture']}');
        }
        return null;
      });
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pumpWidget(sized(FynkelView.world(world), width: 60, height: 40));
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
      expect(tester.takeException().toString(), contains('no RGBA8 format'), reason: 'reported');
      expect(calls, ['register 300x150', 'resize 7', 'unregister 7', 'register 180x120']);
      expect(find.byType(ErrorWidget), findsNothing);
      expect(tester.widget<Texture>(find.byType(Texture)).textureId, 8);
    });

    testWidgets('a resize that keeps failing ends in the error, after three new views', (tester) async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'resize') throw PlatformException(code: 'attach', message: 'no RGBA8 format');
        return call.method == 'register' ? nextTexture++ : null;
      });
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await tester.pump(const Duration(milliseconds: 8));
      for (var i = 0; i < 4; i++) {
        await tester.pumpWidget(sized(FynkelView.world(world), width: 60.0 + i, height: 40));
        for (var j = 0; j < 3; j++) {
          await tester.pump(const Duration(milliseconds: 8));
        }
        tester.takeException();
      }
      expect(find.byType(ErrorWidget), findsOneWidget);
    });

    testWidgets('a failed view tells onError once, shows errorBuilder, and its retry brings a new view', (
      tester,
    ) async {
      var failing = true;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'resize' && failing) throw PlatformException(code: 'attach', message: 'no RGBA8 format');
        return call.method == 'register' ? nextTexture++ : null;
      });
      final errors = <Object>[];
      VoidCallback? retry;
      Widget view(double width) => sized(
        FynkelView.world(
          world,
          onError: (error, stack) => errors.add(error),
          errorBuilder: (context, error, again) {
            retry = again;
            return const Text('the view failed', textDirection: TextDirection.ltr);
          },
        ),
        width: width,
        height: 40,
      );
      await tester.pumpWidget(view(300));
      await tester.pump(const Duration(milliseconds: 8));
      for (var i = 0; i < 4; i++) {
        await tester.pumpWidget(view(60.0 + i));
        for (var j = 0; j < 3; j++) {
          await tester.pump(const Duration(milliseconds: 8));
        }
        tester.takeException();
      }
      expect(find.text('the view failed'), findsOneWidget);
      expect(find.byType(ErrorWidget), findsNothing);
      expect(errors, hasLength(1), reason: 'once, when it fails for good, not for each new view');
      expect(errors.single.toString(), contains('no RGBA8 format'));
      failing = false;
      retry!();
      for (var j = 0; j < 3; j++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
      expect(find.text('the view failed'), findsNothing);
      expect(find.byType(Texture), findsOneWidget, reason: 'a new view, after the retry');
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
    });

    // Flutter's own warning comes for Home too (TRIM_MEMORY_UI_HIDDEN), so
    // there the plugin's, which knows the level, is the one heard.
    testWidgets("memory warnings are the plugin's, not Flutter's, which comes on every Home", (tester) async {
      var told = 0;
      world.onLowMemory.listen((_) => told++);
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await tester.pump(const Duration(milliseconds: 8));
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.hidden);
      tester.binding.handleMemoryPressure();
      await tester.idle();
      expect(told, 0, reason: "Flutter's warning, as Home sends it");
      await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.handlePlatformMessage(
        channel.name,
        channel.codec.encodeMethodCall(const MethodCall('lowMemory', {'level': 40})),
        (_) {},
      );
      await tester.idle();
      expect(told, 1, reason: "the plugin's: Android short of memory");
      tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
    });

    testWidgets("the plugin's failure to give a view its new surface brings a new view", (tester) async {
      await tester.pumpWidget(sized(FynkelView.world(world)));
      for (var i = 0; i < 3; i++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
      expect(calls, ['register 300x150']);
      await TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.handlePlatformMessage(
        channel.name,
        channel.codec.encodeMethodCall(
          const MethodCall('failed', {'texture': 7, 'message': "couldn't draw to the new surface"}),
        ),
        (_) {},
      );
      for (var i = 0; i < 3; i++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
      expect(tester.takeException().toString(), contains("couldn't draw to the new surface"));
      expect(calls, ['register 300x150', 'unregister 7', 'register 300x150']);
      expect(tester.widget<Texture>(find.byType(Texture)).textureId, 8);
    });

    group('on screen', () {
      // The plugin in a FynkelActivity: each view a SurfaceView behind
      // Flutter's, placed where the view is.
      setUp(() {
        TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (
          call,
        ) async {
          final args = call.arguments as Map? ?? const {};
          switch (call.method) {
            case 'register':
              calls.add(
                'register ${args['width']}x${args['height']} at ${args['left']},${args['top']} '
                '${args['frameWidth']}x${args['frameHeight']}${args['onScreen'] == true ? '' : ' as a texture'}',
              );
              return args['onScreen'] == true ? {'id': nextTexture++, 'onScreen': true} : nextTexture++;
            case 'place':
              calls.add(
                'place ${args['texture']} at ${args['left']},${args['top']} ${args['width']}x${args['height']}',
              );
            case 'over':
              calls.add('over ${args['texture']} ${args['over']}');
            case 'resize':
              calls.add('resize ${args['texture']} ${args['width']}x${args['height']}');
            case 'unregister':
              calls.add('unregister ${args['texture']}');
          }
          return null;
        });
      });

      Future<void> frames(WidgetTester tester, [int n = 3]) async {
        for (var i = 0; i < n; i++) {
          await tester.pump(const Duration(milliseconds: 8));
        }
      }

      testWidgets('a view on screen clears its place for the surface behind, and moves it with the widget', (
        tester,
      ) async {
        // 100 × 50 in the middle of 800 × 600, 3 pixels a point.
        await tester.pumpWidget(sized(FynkelView.world(world, renderScale: 0.5)));
        await frames(tester);
        expect(calls, ['register 150x75 at 1050,825 300x150']);
        expect(find.byType(Texture), findsNothing, reason: 'the frames are not a texture');
        final clear = find.descendant(of: find.byType(FynkelView), matching: find.byType(CustomPaint));
        expect(clear, findsWidgets);

        // A page sliding in moves it; the drawn size stays.
        await tester.pumpWidget(
          Directionality(
            textDirection: TextDirection.ltr,
            child: Align(
              alignment: Alignment.topLeft,
              child: Padding(
                padding: const EdgeInsets.only(left: 10, top: 20),
                child: SizedBox(width: 100, height: 50, child: FynkelView.world(world, renderScale: 0.5)),
              ),
            ),
          ),
        );
        await frames(tester);
        expect(calls, ['register 150x75 at 1050,825 300x150', 'unregister 7', 'register 150x75 at 30,60 300x150']);
        calls.clear();

        await tester.pumpWidget(sized(const SizedBox()));
        await tester.pump();
        expect(calls, ['unregister 8']);
      });

      testWidgets('a view that moves is placed again, once a move', (tester) async {
        final offset = ValueNotifier(0.0);
        addTearDown(offset.dispose);
        await tester.pumpWidget(
          Directionality(
            textDirection: TextDirection.ltr,
            child: ValueListenableBuilder<double>(
              valueListenable: offset,
              builder: (_, x, child) => Padding(
                padding: EdgeInsets.only(left: x),
                child: child,
              ),
              child: Align(
                alignment: Alignment.topLeft,
                child: SizedBox(width: 100, height: 50, child: FynkelView.world(world)),
              ),
            ),
          ),
        );
        await frames(tester);
        offset.value = 20;
        await frames(tester);
        expect(calls, ['register 300x150 at 0,0 300x150', 'place 7 at 60,0 300x150']);
        await frames(tester);
        expect(calls, hasLength(2), reason: 'placed once a move, not once a frame');
        await tester.pumpWidget(sized(const SizedBox()));
        await tester.pump();
      });

      testWidgets('a view lying within another goes over it; one beside it does not', (tester) async {
        Widget views({required double minimapLeft}) => Directionality(
          textDirection: TextDirection.ltr,
          child: Stack(
            children: [
              Positioned(left: 0, top: 0, width: 200, height: 200, child: FynkelView.world(world)),
              Positioned(
                left: minimapLeft,
                top: 10,
                width: 50,
                height: 50,
                child: FynkelView.world(world, input: false, worldWidgets: false),
              ),
            ],
          ),
        );
        await tester.pumpWidget(views(minimapLeft: 140));
        await frames(tester);
        expect(calls, containsAll(['over 8 true']));
        expect(calls.where((c) => c.startsWith('over 7')), isEmpty, reason: 'the game stays under');

        calls.clear();
        await tester.pumpWidget(views(minimapLeft: 300));
        await frames(tester);
        expect(calls, contains('over 8 false'));
        await tester.pumpWidget(sized(const SizedBox()));
        await tester.pump();
      });

      testWidgets('directToScreen: false keeps the texture', (tester) async {
        await tester.pumpWidget(sized(FynkelView.world(world, directToScreen: false)));
        await frames(tester);
        expect(calls, ['register 300x150 at null,null nullxnull as a texture']);
        expect(tester.widget<Texture>(find.byType(Texture)).textureId, 7);
        await tester.pumpWidget(sized(const SizedBox()));
        await tester.pump();
      });
    });
  });

  group('the iOS way', () {
    // The plugin on an iPhone: each view a layer of its own under
    // Flutter's, placed where the view is; a texture where it can't be.
    late bool layers;
    setUp(() {
      debugIos = true;
      layers = true;
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
        final args = call.arguments as Map? ?? const {};
        switch (call.method) {
          case 'register':
            if (args['layer'] != true) {
              expect(args['acquire'], isNot(0));
              calls.add('register a texture');
              return nextTexture++;
            }
            expect(
              args.keys,
              unorderedEquals([
                'view',
                'width',
                'height',
                'layer',
                'attach',
                'detach',
                'free',
                'onScreen',
                'left',
                'top',
                'frameWidth',
                'frameHeight',
              ]),
            );
            for (final address in ['view', 'attach', 'detach', 'free']) {
              expect(args[address], isNot(0), reason: address);
            }
            calls.add(
              'register a layer ${args['width']}x${args['height']} at ${args['left']},${args['top']} '
              '${args['frameWidth']}x${args['frameHeight']}',
            );
            if (!layers) throw PlatformException(code: 'nolayer', message: 'no Flutter view on screen');
            return {'id': nextTexture++, 'onScreen': true};
          case 'place':
            calls.add('place ${args['texture']} at ${args['left']},${args['top']} ${args['width']}x${args['height']}');
          case 'resize':
            calls.add('resize ${args['texture']} ${args['width']}x${args['height']}');
          case 'unregister':
            calls.add('unregister ${args['texture']}');
        }
        return null;
      });
    });
    tearDown(() => debugIos = null);

    Future<void> frames(WidgetTester tester, [int n = 3]) async {
      for (var i = 0; i < n; i++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
    }

    testWidgets('a view is a layer under Flutter\'s, sized, placed and resized, and goes with the view', (
      tester,
    ) async {
      // 100 × 50 in the middle of 800 × 600, 3 pixels a point.
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await frames(tester);
      expect(calls, ['register a layer 300x150 at 1050,825 300x150']);
      expect(find.byType(Texture), findsNothing, reason: 'the frames are not a texture');
      expect(find.descendant(of: find.byType(FynkelView), matching: find.byType(CustomPaint)), findsWidgets);

      // A new size: the layer is drawn at it once it holds, and put where
      // the view now is.
      await tester.pumpWidget(sized(FynkelView.world(world), width: 60, height: 40));
      await frames(tester, 5);
      expect(calls, [
        'register a layer 300x150 at 1050,825 300x150',
        'place 7 at 1110,840 180x120',
        'resize 7 180x120',
      ]);

      calls.clear();
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
      expect(calls, ['unregister 7']);
    });

    testWidgets('directToScreen: false keeps the texture', (tester) async {
      await tester.pumpWidget(sized(FynkelView.world(world, directToScreen: false)));
      await frames(tester);
      expect(calls, ['register a texture']);
      expect(tester.widget<Texture>(find.byType(Texture)).textureId, 7);
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
    });

    testWidgets('where the plugin has no layer to give (Flutter in an app\'s own views), a texture, quietly', (
      tester,
    ) async {
      layers = false;
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await frames(tester);
      expect(calls, ['register a layer 300x150 at 1050,825 300x150', 'register a texture']);
      expect(tester.takeException(), isNull, reason: 'not an error');
      expect(tester.widget<Texture>(find.byType(Texture)).textureId, 7);
      expect(debugViewsOpen, 1, reason: 'the layer\'s view was freed');
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
    });

    testWidgets('a failed resize brings a new layer at the new size', (tester) async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
        final args = call.arguments as Map? ?? const {};
        switch (call.method) {
          case 'register':
            calls.add('register ${args['width']}x${args['height']}');
            return {'id': nextTexture++, 'onScreen': true};
          case 'resize':
            calls.add('resize ${args['texture']}');
            throw PlatformException(code: 'layer', message: "couldn't configure the layer at its new size");
          case 'unregister':
            calls.add('unregister ${args['texture']}');
        }
        return null;
      });
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await frames(tester, 1);
      await tester.pumpWidget(sized(FynkelView.world(world), width: 60, height: 40));
      await frames(tester, 5);
      expect(tester.takeException().toString(), contains("couldn't configure the layer"), reason: 'reported');
      expect(calls, ['register 300x150', 'resize 7', 'unregister 7', 'register 180x120']);
      expect(find.byType(ErrorWidget), findsNothing);
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
    });
  });

  group('world widgets', () {
    Future<void> frames(WidgetTester tester, [int n = 1]) async {
      for (var i = 0; i < n; i++) {
        await tester.pump(const Duration(milliseconds: 16));
      }
    }

    /// Where the camera sees [point] in the 200 × 100 view, on the screen.
    Offset seen(WidgetTester tester, Vec3 point) {
      final at = world.camera!.worldToScreenPoint(point, width: 200, height: 100)!;
      return tester.getTopLeft(find.byType(FynkelView)) + Offset(at.x, at.y);
    }

    const tag = ValueKey('tag');

    testWidgets('a WorldWidget sits where the camera sees its object, follows it, and hides behind the camera', (
      tester,
    ) async {
      final thing = world.add(
        GameObject(
          components: [
            WorldWidget(
              (_) => const ColoredBox(key: tag, color: Colors.green, child: SizedBox(width: 20, height: 10)),
              offset: Vec3(0, 0.5, 0),
              alignment: Alignment.bottomCenter,
            ),
          ],
        ),
      );
      await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
      await frames(tester, 2); // the picture is a frame behind
      expect(
        tester.getBottomLeft(find.byKey(tag)) + const Offset(10, 0),
        offsetMoreOrLessEquals(seen(tester, Vec3(0, 0.5, 0))),
      );
      thing.position = Vec3(1, -0.5, 0);
      await frames(tester, 2); // the picture is a frame behind
      expect(
        tester.getBottomLeft(find.byKey(tag)) + const Offset(10, 0),
        offsetMoreOrLessEquals(seen(tester, Vec3(1, 0, 0))),
      );
      thing.position = Vec3(0, 0, 10);
      await frames(tester, 2); // the picture is a frame behind
      expect(find.byKey(tag).hitTestable(), findsNothing, reason: 'behind the camera');
      thing.position = Vec3(30, 0, 0);
      await frames(tester, 2); // the picture is a frame behind
      expect(find.byKey(tag).hitTestable(), findsNothing, reason: 'off the view');
      thing.position = Vec3.zero;
      await frames(tester, 2); // the picture is a frame behind
      expect(find.byKey(tag).hitTestable(), findsOneWidget);
      thing.active = false;
      await frames(tester, 2); // the picture is a frame behind
      expect(find.byKey(tag), findsNothing, reason: 'its object is inactive');
      expect(worldWidgetsOf(world), isEmpty);
      thing.active = true;
      await frames(tester, 2); // the picture is a frame behind
      expect(find.byKey(tag), findsOneWidget);
      thing.destroy();
      await frames(tester, 2); // the picture is a frame behind
      expect(find.byKey(tag), findsNothing);
      expect(worldWidgetsOf(world), isEmpty);
    });

    testWidgets('a WorldWidget moves with the engine\'s picture, which shows an update a frame later on a texture', (
      tester,
    ) async {
      // Measured on a Mac (the lab's sync check, filmed at 120 fps): the
      // engine's picture of an update is on screen a frame after Flutter's
      // of the same update.
      final thing = world.add(
        GameObject(
          components: [
            WorldWidget((_) => const ColoredBox(key: tag, color: Colors.green, child: SizedBox(width: 20, height: 10))),
          ],
        ),
      );
      await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
      await frames(tester, 2);
      expect(tester.getCenter(find.byKey(tag)), offsetMoreOrLessEquals(seen(tester, Vec3.zero)));
      thing.position = Vec3(1, 0, 0);
      await frames(tester);
      expect(
        tester.getCenter(find.byKey(tag)),
        offsetMoreOrLessEquals(seen(tester, Vec3.zero)),
        reason: 'the picture on screen is still the last update\'s',
      );
      await frames(tester);
      expect(tester.getCenter(find.byKey(tag)), offsetMoreOrLessEquals(seen(tester, Vec3(1, 0, 0))));
    });

    testWidgets('a WorldWidget shakes with the camera, though the camera itself never moves', (tester) async {
      world.add(GameObject(components: [WorldWidget((_) => const SizedBox(key: tag, width: 20, height: 10))]));
      await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
      await frames(tester);
      final still = tester.getCenter(find.byKey(tag));
      // Paused, the shake holds where it is, to measure against.
      world.time.scale = 0;
      world.cameras.shake(0.3, seconds: 1);
      await frames(tester, 2);
      final shake = world.cameras.shakeOf(world.camera!);
      expect(shake, isNot(Vec3.zero));
      expect(world.camera!.gameObject.position, Vec3(0, 0, 4));
      expect(tester.getCenter(find.byKey(tag)), isNot(offsetMoreOrLessEquals(still)));
      expect(tester.getCenter(find.byKey(tag)), offsetMoreOrLessEquals(seen(tester, -shake)));
    });

    testWidgets('a tap on a world widget reaches it, not the game; beside it, the game', (tester) async {
      var taps = 0;
      world.add(
        GameObject(
          components: [
            WorldWidget(
              (_) => GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => taps++,
                child: const SizedBox(key: tag, width: 40, height: 20),
              ),
            ),
          ],
        ),
      );
      await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
      await frames(tester);
      final on = await tester.startGesture(tester.getCenter(find.byKey(tag)));
      await frames(tester);
      expect(world.input.fingers, isEmpty, reason: 'the game has no finger down');
      await on.up();
      await frames(tester);
      expect(taps, 1);
      final beside = await tester.startGesture(tester.getCenter(find.byKey(tag)) + const Offset(60, 0));
      await frames(tester);
      expect(world.input.fingers, hasLength(1));
      await beside.up();
      await frames(tester);
    });

    testWidgets('nearer world widgets are over farther ones, and shrink beyond a distance', (tester) async {
      final hits = <String>[];
      Widget box(String name) => GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => hits.add(name),
        child: SizedBox(key: ValueKey(name), width: 40, height: 20),
      );
      // Added near first: the order they came in doesn't decide.
      world.add(GameObject(components: [WorldWidget((_) => box('near'))]));
      final far = world.add(
        GameObject(
          transform: Transform(position: Vec3(0, 0, -4)),
          components: [WorldWidget((_) => box('far'), shrinkBeyond: 4)],
        ),
      );
      await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
      await frames(tester);
      await tester.tapAt(seen(tester, Vec3.zero));
      expect(hits, ['near']);
      expect(tester.getSize(find.byKey(const ValueKey('near'))), const Size(40, 20));
      // 8 from the camera, shrunk beyond 4: half as big.
      final rect = tester.getRect(find.byKey(const ValueKey('far')));
      expect(rect.width, moreOrLessEquals(20));
      expect(rect.height, moreOrLessEquals(10));
      expect(rect.center, offsetMoreOrLessEquals(seen(tester, far.position)));
    });
    testWidgets(
      'a WorldWidget that hides behind things fades out while something is in front of it, not its own object',
      (tester) async {
        // Its point is inside its own cube, behind the cube's front: that
        // doesn't hide it.
        world.add(
          GameObject(
            components: [
              MeshRenderer(Mesh.cube(), Material()),
              WorldWidget(
                (_) => const ColoredBox(key: tag, color: Colors.green, child: SizedBox(width: 20, height: 10)),
                hideBehind: true,
              ),
            ],
          ),
        );
        await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
        await frames(tester, 2);
        expect(find.byKey(tag).hitTestable(), findsOneWidget, reason: 'its own cube');
        final wall = world.add(
          GameObject(
            transform: Transform(position: Vec3(0, 0, 2), scale: Vec3(3, 3, 0.1)),
            components: [MeshRenderer(Mesh.cube(), Material())],
          ),
        );
        await frames(tester);
        expect(find.byKey(tag), findsOneWidget, reason: 'fading, still there');
        await frames(tester, 12);
        expect(find.byKey(tag).hitTestable(), findsNothing, reason: 'behind the wall');
        wall.destroy();
        await frames(tester, 12);
        expect(find.byKey(tag).hitTestable(), findsOneWidget, reason: 'the wall is gone');
        // A wall on a layer it doesn't hide behind.
        const glass = 3;
        world.add(
          GameObject(
            layer: glass,
            transform: Transform(position: Vec3(0, 0, 2), scale: Vec3(3, 3, 0.1)),
            components: [MeshRenderer(Mesh.cube(), Material())],
          ),
        );
        final through = world.add(
          GameObject(
            transform: Transform(position: Vec3(0, 0.8, 0)),
            components: [
              WorldWidget(
                (_) => const ColoredBox(
                  key: ValueKey('through'),
                  color: Colors.green,
                  child: SizedBox(width: 20, height: 10),
                ),
                hideBehind: true,
                hideLayers: LayerMask.except([glass]),
              ),
            ],
          ),
        );
        await frames(tester, 12);
        expect(find.byKey(tag).hitTestable(), findsNothing, reason: 'behind the glass, on every layer');
        expect(find.byKey(const ValueKey('through')).hitTestable(), findsOneWidget, reason: 'not behind glass');
        expect(through.get<WorldWidget>()!.hideLayers, LayerMask.except([glass]));
      },
    );

    testWidgets('a WorldWidget kept on the screen waits at its edge, toward its point, and says so', (tester) async {
      late WorldWidgetPlace place;
      final thing = world.add(
        GameObject(
          components: [
            WorldWidget((context) {
              place = WorldWidget.of(context);
              return ValueListenableBuilder(
                valueListenable: place.offScreen,
                builder: (_, off, _) => ColoredBox(
                  key: ValueKey(off ? 'off' : 'on'),
                  color: Colors.green,
                  child: const SizedBox(width: 20, height: 10),
                ),
              );
            }, keepOnScreen: true),
          ],
        ),
      );
      await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
      await frames(tester);
      final view = tester.getRect(find.byType(FynkelView));
      expect(tester.getCenter(find.byKey(const ValueKey('on'))), offsetMoreOrLessEquals(seen(tester, Vec3.zero)));
      expect(place.offScreen.value, isFalse);

      Future<Rect> kept(Vec3 at) async {
        thing.position = at;
        await frames(tester, 2);
        expect(place.offScreen.value, isTrue, reason: 'at $at');
        return tester.getRect(find.byKey(const ValueKey('off')).hitTestable());
      }

      // Right of the view: inside its right edge, the default margin in,
      // level with the point.
      var rect = await kept(Vec3(30, 0, 0));
      expect(rect.right, moreOrLessEquals(view.right - 8));
      expect(rect.center.dy, moreOrLessEquals(view.center.dy));
      expect(place.angle.value, moreOrLessEquals(0));
      rect = await kept(Vec3(-30, 0, 0));
      expect(rect.left, moreOrLessEquals(view.left + 8));
      expect(place.angle.value.abs(), moreOrLessEquals(math.pi));
      rect = await kept(Vec3(0, 30, 0));
      expect(rect.top, moreOrLessEquals(view.top + 8));
      expect(place.angle.value, moreOrLessEquals(-math.pi / 2));
      // Behind the camera and to its right: the right edge still.
      rect = await kept(Vec3(2, 0, 10));
      expect(rect.right, moreOrLessEquals(view.right - 8));
      thing.position = Vec3.zero;
      await frames(tester);
      expect(place.offScreen.value, isFalse);
      expect(tester.getCenter(find.byKey(const ValueKey('on'))), offsetMoreOrLessEquals(seen(tester, Vec3.zero)));
    });

    testWidgets('a WorldWidget hidden behind a wall shows at the edge once its point leaves the view', (tester) async {
      final thing = world.add(
        GameObject(
          components: [
            WorldWidget(
              (_) => const ColoredBox(key: tag, color: Colors.green, child: SizedBox(width: 20, height: 10)),
              hideBehind: true,
              keepOnScreen: true,
            ),
          ],
        ),
      );
      world.add(
        GameObject(
          transform: Transform(position: Vec3(0, 0, 2), scale: Vec3(3, 3, 0.1)),
          components: [MeshRenderer(Mesh.cube(), Material())],
        ),
      );
      await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
      await frames(tester, 2);
      expect(find.byKey(tag).hitTestable(), findsNothing, reason: 'behind the wall');
      thing.position = Vec3(30, 0, 0);
      await frames(tester, 2);
      expect(find.byKey(tag).hitTestable(), findsOneWidget, reason: 'waiting at the edge, wall or not');
    });

    testWidgets('a WorldWidget kept on the screen with its point near the edge stays wholly inside it', (tester) async {
      late WorldWidgetPlace place;
      final thing = world.add(
        GameObject(
          components: [
            WorldWidget((context) {
              place = WorldWidget.of(context);
              return const ColoredBox(
                key: ValueKey('tag'),
                color: Colors.green,
                child: SizedBox(width: 40, height: 10),
              );
            }, keepOnScreen: true),
          ],
        ),
      );
      await tester.pumpWidget(sized(FynkelView.world(world), width: 200, height: 100));
      await frames(tester);
      final view = tester.getRect(find.byType(FynkelView));
      // Its point just inside the right edge: half the tag would hang over.
      var x = 0.0;
      while (seen(tester, Vec3(x, 0, 0)).dx < view.right - 4) {
        x += 0.01;
      }
      thing.position = Vec3(x, 0, 0);
      await frames(tester, 2);
      final at = seen(tester, thing.position);
      expect(at.dx, lessThan(view.right));
      expect(place.offScreen.value, isFalse);
      final rect = tester.getRect(find.byKey(const ValueKey('tag')));
      expect(rect.right, moreOrLessEquals(view.right - 8));
      expect(rect.center.dy, moreOrLessEquals(at.dy));
    });
  });

  group('from a root', () {
    final png = File('../../tests/assets/grid.png').readAsBytesSync();

    testWidgets('runs a game: makes its world, loads it from the bundle, starts it, and frees it all with the view', (
      tester,
    ) async {
      final game = Tiny();
      final bundle = Bundle({'assets/grid.png': png});
      final key = GlobalKey<FynkelViewState>();
      await tester.pumpWidget(sized(FynkelView(key: key, root: game, source: bundleSource(bundle))));
      final own = key.currentState!.world;
      expect(game.world, same(own));
      expect(game.log, ['load']);

      await tester.pump(const Duration(milliseconds: 8));
      expect(game.log, ['load'], reason: 'not started while it loads');
      await finish(tester, game.loaded);
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pump(const Duration(milliseconds: 8));
      expect(game.log, ['load', 'start', 'update']);
      expect(bundle.asked, ['assets/grid.png']);
      expect(game.texture!.width, greaterThan(0));
      expect(calls, ['register']);

      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
      expect(calls, ['register', 'unregister 7']);
      expect(own.isDisposed, isTrue);
      expect(game.isAlive, isFalse);
    });

    testWidgets('a new root on rebuild is reported and not shown; a world in its place starts over', (tester) async {
      final game = Tiny();
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      final key = GlobalKey<FynkelViewState>();
      await tester.pumpWidget(sized(FynkelView(key: key, root: game, source: source)));
      final own = key.currentState!.world;
      await tester.pumpWidget(sized(FynkelView(key: key, root: Tiny(), source: source)));
      expect('${tester.takeException()}', contains('give it a Key to start another'));
      expect(key.currentState!.world, same(own));
      expect(key.currentState!.root, same(game), reason: 'the root it shows');
      await tester.pumpWidget(sized(FynkelView(key: key, root: Tiny(), source: source)));
      expect(tester.takeException(), isNull, reason: 'said once');

      // The way to change level: the next root in the same world.
      game.destroy();
      final level2 = own.add(GameObject());
      await tester.pumpWidget(sized(FynkelView(key: key, root: level2, source: source)));
      expect(tester.takeException(), isNull);
      expect(key.currentState!.root, same(level2));

      await tester.pumpWidget(sized(FynkelView.world(world, key: key)));
      expect(key.currentState!.world, same(world));
      expect(own.isDisposed, isTrue);
      expect(world.isDisposed, isFalse);

      final next = Tiny();
      await tester.pumpWidget(sized(FynkelView(key: key, root: next, source: source)));
      expect(next.world, isNot(same(own)));
      expect(key.currentState!.world, same(next.world));
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump(const Duration(milliseconds: 1));
      expect(world.isDisposed, isFalse, reason: 'a world given by hand is not the view\'s to free');
    });

    testWidgets(
      "a view Flutter makes again for its root (now in a Stack) keeps the game; the world goes with the last",
      (tester) async {
        final counter = Counter();
        final game = GameObject(components: [counter]);
        Widget page({required bool hud}) {
          final view = FynkelView(root: game);
          return sized(
            hud
                ? Stack(
                    children: [
                      view,
                      const Text('hud', textDirection: TextDirection.ltr),
                    ],
                  )
                : view,
          );
        }

        await tester.pumpWidget(page(hud: false));
        await tester.pump(const Duration(milliseconds: 16));
        final world = game.world;
        await tester.pumpWidget(page(hud: true));
        final before = counter.updates;
        for (var i = 0; i < 3; i++) {
          await tester.pump(const Duration(milliseconds: 16));
        }
        expect(tester.takeException(), isNull);
        expect(world.isDisposed, isFalse, reason: 'the new view shows it');
        expect(game.isAlive, isTrue);
        expect(counter.updates, greaterThan(before), reason: 'the game goes on');
        await tester.pumpWidget(sized(const SizedBox()));
        expect(world.isDisposed, isTrue, reason: 'no view shows it now');
      },
    );

    testWidgets('a source given for a root already in a world is reported: that world loads its assets', (
      tester,
    ) async {
      final root = world.add(GameObject());
      await tester.pumpWidget(sized(FynkelView(root: root, source: bundleSource(Bundle({})))));
      expect('${tester.takeException()}', contains('give source: to the World it was added to'));
    });

    testWidgets('a hot reload reads the shaders of a world the view made again', (tester) async {
      final asked = <String>[];
      final game = _LoadsAShader();
      await tester.pumpWidget(
        sized(
          FynkelView(
            root: game,
            source: (name) async {
              asked.add(name);
              return Uint8List.fromList(
                'fn unlit(in: SurfaceInput) -> vec4<f32> {\n    return vec4<f32>(1.0, 0.0, 0.0, 1.0);\n}\n'.codeUnits,
              );
            },
          ),
        ),
      );
      await finish(tester, game.loaded);
      expect(asked, ['flat.wgsl']);
      await reload(tester);
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      await tester.pump();
      expect(asked, ['flat.wgsl', 'flat.wgsl'], reason: 'read again, once');
      await tester.pumpWidget(sized(const SizedBox()));
    });

    testWidgets("a failed load reaches the world's errors", (tester) async {
      final game = Tiny();
      final key = GlobalKey<FynkelViewState>();
      await tester.pumpWidget(
        sized(FynkelView(key: key, root: game, source: bundleSource(Bundle({})), showErrors: false)),
      );
      final errors = <ObjectError>[];
      key.currentState!.world.errors.listen(errors.add);
      await finish(tester, game.loaded);
      await tester.pump();
      expect(errors.single.error.toString(), contains('no asset assets/grid.png'));
      expect(game.isAlive, isFalse);
    });

    testWidgets("in a debug build, what the game throws shows over it with the game's line, and goes to Flutter", (
      tester,
    ) async {
      await tester.pumpWidget(sized(FynkelView(root: LooksAtAPoint()), width: 800, height: 600));
      expect('${tester.takeException()}', contains('give it target - eye'), reason: 'reported to Flutter');
      await tester.pump();
      expect(find.textContaining('LooksAtAPoint#'), findsOneWidget, reason: 'not a black screen');
      expect(find.textContaining('give it target - eye'), findsOneWidget);
      expect(find.textContaining('fynkel_view_test.dart'), findsOneWidget, reason: "the game's own line");
      await tester.tap(find.textContaining('Tap to hide'));
      await tester.pump();
      expect(find.textContaining('give it target - eye'), findsNothing);
    });

    testWidgets('errors while it runs show too, the latest first, with how many came before', (tester) async {
      final game = ThrowsInUpdate();
      await tester.pumpWidget(sized(FynkelView.world(world), width: 800, height: 600));
      world.add(game);
      await tester.pump(const Duration(milliseconds: 16));
      expect(tester.takeException(), isStateError);
      await tester.pump(const Duration(milliseconds: 16));
      expect(tester.takeException(), isStateError);
      game.destroy();
      await tester.pump();
      expect(find.textContaining('ThrowsInUpdate#'), findsOneWidget);
      expect(find.textContaining('throw 2'), findsOneWidget);
      expect(find.textContaining('1 more before it'), findsOneWidget);
    });

    testWidgets('with showErrors off, errors are the game\'s alone: nothing over it, nothing reported', (tester) async {
      final key = GlobalKey<FynkelViewState>();
      final errors = <ObjectError>[];
      await tester.pumpWidget(sized(FynkelView.world(world, key: key, showErrors: false)));
      world.errors.listen(errors.add);
      final game = world.add(ThrowsInUpdate());
      await tester.pump(const Duration(milliseconds: 16));
      await tester.pump(const Duration(milliseconds: 16));
      await tester.pump();
      expect(errors, isNotEmpty);
      expect(tester.takeException(), isNull);
      expect(find.textContaining('ThrowsInUpdate#'), findsNothing);
      game.destroy();
    });

    testWidgets('FynkelApp makes the game, shows it in a view of its own, and puts the overlay over it', (
      tester,
    ) async {
      final made = <Tiny>[];
      await tester.pumpWidget(
        FynkelApp(
          () => (made..add(Tiny())).last,
          overlay: (context, game) => Text('${made.indexOf(game)}', textDirection: TextDirection.ltr),
          source: bundleSource(Bundle({'assets/grid.png': png})),
        ),
      );
      final game = made.single;
      final view = tester.widget<FynkelView>(find.byType(FynkelView));
      expect(view.root, same(game));
      expect(find.text('0'), findsOneWidget);
      expect(game.world, same(tester.state<FynkelViewState>(find.byType(FynkelView)).world));
      await finish(tester, game.loaded);
      expect(game.texture, isNotNull, reason: 'from the source given');
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
      expect(game.isAlive, isFalse);
    });

    testWidgets(
      "FynkelApp's overlay holds Material widgets, in its theme, and opens dialogs over the game (finding 1)",
      (tester) async {
        final volume = ValueNotifier(0.5);
        await tester.pumpWidget(
          FynkelApp(
            Tiny.new,
            theme: ThemeData(colorScheme: ColorScheme.fromSeed(seedColor: Colors.green)),
            source: bundleSource(Bundle({'assets/grid.png': png})),
            overlay: (context, game) => Column(
              children: [
                ValueListenableBuilder(
                  valueListenable: volume,
                  builder: (context, value, _) => Slider(value: value, onChanged: (v) => volume.value = v),
                ),
                Builder(
                  builder: (context) => TextButton(
                    onPressed: () => showDialog<void>(context: context, builder: (_) => const Text('paused')),
                    child: const Text('pause'),
                  ),
                ),
              ],
            ),
          ),
        );
        expect(tester.takeException(), isNull);
        await tester.drag(find.byType(Slider), const Offset(100, 0));
        expect(volume.value, isNot(0.5));
        await tester.tap(find.text('pause'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 300));
        expect(find.text('paused'), findsOneWidget);
        expect(find.byType(FynkelView), findsOneWidget, reason: 'the game stays under the dialog');
        final game = tester.widget<FynkelView>(find.byType(FynkelView)).root!;
        await tester.pumpWidget(const SizedBox());
        await tester.pump(const Duration(milliseconds: 1));
        expect(game.isDestroyed, isTrue, reason: 'leaving the app destroys the game: its onDestroy ran');
      },
    );

    testWidgets("taps around the overlay's text reach the game: a HUD covers only itself", (tester) async {
      await tester.pumpWidget(
        FynkelApp(
          Clickable.new,
          overlay: (context, game) => SafeArea(
            child: ValueListenableBuilder(
              valueListenable: game.clicks,
              builder: (context, clicks, _) => Text('$clicks'),
            ),
          ),
        ),
      );
      for (var i = 0; i < 3; i++) {
        await tester.pump(const Duration(milliseconds: 16));
      }
      final game = FynkelGame.of<Clickable>(tester.element(find.byType(FynkelView))).game;
      final world = game.world;
      final middle = world.camera!.worldToScreenPoint(Vec3.zero, width: world.input.width, height: world.input.height)!;
      await tester.tapAt(Offset(middle.x, middle.y));
      await tester.pump(const Duration(milliseconds: 16));
      expect(game.clicks.value, 1);
      await tester.pump();
      expect(find.text('1'), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets("a page pushed from FynkelApp's overlay finds the game with FynkelGame.of", (tester) async {
      late FynkelGameState<Clickable> found;
      await tester.pumpWidget(
        FynkelApp(
          Clickable.new,
          overlay: (context, game) => TextButton(
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute<void>(
                builder: (context) {
                  found = FynkelGame.of<Clickable>(context);
                  return const Text('settings');
                },
              ),
            ),
            child: const Text('open'),
          ),
        ),
      );
      await tester.pump(const Duration(milliseconds: 16));
      final game = FynkelGame.of<Clickable>(tester.element(find.byType(FynkelView))).game;
      await tester.tap(find.text('open'));
      for (var i = 0; i < 20; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }
      expect(find.text('settings'), findsOneWidget);
      expect(found.game, same(game));
      expect(game.world.paused, isTrue, reason: 'the page covers the game');
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets("a page pushed over a FynkelGame of the app's own is told how to reach the game", (tester) async {
      late BuildContext page;
      await tester.pumpWidget(
        MaterialApp(
          home: FynkelGame(
            Clickable.new,
            builder: (context, game) {
              page = context;
              return FynkelView(root: game);
            },
          ),
        ),
      );
      late Object error;
      unawaited(
        Navigator.of(page).push(
          MaterialPageRoute<void>(
            builder: (context) {
              try {
                FynkelGame.of<Clickable>(context);
              } catch (e) {
                error = e;
              }
              return const SizedBox();
            },
          ),
        ),
      );
      await tester.pump();
      expect('$error', contains("page pushed with Navigator"));
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets("FynkelApp's overlay may be Positioned; one that isn't stays loose from the top left", (tester) async {
      await tester.pumpWidget(
        FynkelApp(
          Clickable.new,
          overlay: (context, game) => const Positioned(right: 10, bottom: 20, child: Text('corner')),
        ),
      );
      await tester.pump(const Duration(milliseconds: 16));
      expect(tester.takeException(), isNull);
      final screen = tester.getSize(find.byType(FynkelView));
      final corner = tester.getRect(find.text('corner'));
      expect(corner.right, screen.width - 10);
      expect(corner.bottom, screen.height - 20);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets("FynkelApp gives its view the view's settings", (tester) async {
      void onStats(FynkelViewStats stats) {}
      void onError(Object error, StackTrace stack) {}
      Widget errorBuilder(BuildContext context, Object error, VoidCallback retry) => const SizedBox();
      await tester.pumpWidget(
        FynkelApp(
          Clickable.new,
          onStats: onStats,
          onError: onError,
          errorBuilder: errorBuilder,
          pauseWhenCovered: false,
          showErrors: false,
          directToScreen: false,
          renderScale: 0.5,
        ),
      );
      final view = tester.widget<FynkelView>(find.byType(FynkelView));
      expect(view.onStats, same(onStats));
      expect(view.onError, same(onError));
      expect(view.errorBuilder, same(errorBuilder));
      expect(view.pauseWhenCovered, isFalse);
      expect(view.showErrors, isFalse);
      expect(view.directToScreen, isFalse);
      expect(view.renderScale, 0.5);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets('a FynkelGame given another make is reported: it keeps the game it started with', (tester) async {
      Widget game(GameObject Function() make) =>
          sized(FynkelGame<GameObject>(make, builder: (context, game) => FynkelView(root: game)));
      await tester.pumpWidget(game(Clickable.new));
      final first = tester.state<FynkelGameState<GameObject>>(find.byType(FynkelGame<GameObject>)).game;
      await tester.pumpWidget(game(Clickable.new));
      expect(tester.takeException(), isNull, reason: 'the same make');
      await tester.pumpWidget(game(LooksAtAPoint.new));
      expect('${tester.takeException()}', contains('give it a Key to start another'));
      expect(tester.state<FynkelGameState<GameObject>>(find.byType(FynkelGame<GameObject>)).game, same(first));
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets('F5 starts a FynkelGame over: a new root in a new world, and what was built around it again', (
      tester,
    ) async {
      final made = <Tiny>[];
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      await tester.pumpWidget(
        sized(
          FynkelGame(
            () => (made..add(Tiny())).last,
            source: source,
            builder: (context, game) => Stack(
              children: [
                FynkelView(root: game),
                Text('game ${made.indexOf(game)}'),
              ],
            ),
          ),
        ),
      );
      final first = made.single, world = first.world;
      await finish(tester, first.loaded);
      await tester.sendKeyEvent(LogicalKeyboardKey.f5);
      await tester.pump();
      expect(made, hasLength(2));
      final second = made.last;
      expect(world.isDisposed, isTrue, reason: 'the old view went, and its world with it');
      expect(first.isAlive, isFalse);
      expect(first.isDestroyed, isTrue, reason: 'its onDestroy ran (finding 7)');
      expect(second.world, isNot(same(world)));
      expect(find.text('game 1'), findsOneWidget);
      expect(second.world.input.isPressed(KeyCode.f5), isFalse, reason: 'the restart key is not the game\'s');
      await finish(tester, second.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets('FynkelGame.of gives the game typed, and says why when it cannot', (tester) async {
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      late BuildContext inside, outside;
      await tester.pumpWidget(
        sized(
          Builder(
            builder: (context) {
              outside = context;
              return FynkelGame(
                Tiny.new,
                source: source,
                builder: (context, game) {
                  inside = context;
                  return FynkelView(root: game);
                },
              );
            },
          ),
        ),
      );
      final Tiny game = FynkelGame.of<Tiny>(inside).game;
      expect(FynkelGame.maybeOf(inside)!.game, same(game));
      expect(FynkelGame.maybeOf(outside), isNull);
      expect(
        () => FynkelGame.of(outside),
        throwsA(isA<FlutterError>().having((e) => e.message, 'message', contains('no FynkelGame'))),
      );
      expect(FynkelGame.maybeOf<LooksAtAPoint>(inside), isNull, reason: 'its game is a Tiny');
      expect(
        () => FynkelGame.of<LooksAtAPoint>(inside),
        throwsA(
          isA<FlutterError>().having(
            (e) => e.message,
            'message',
            allOf(contains('made for Tiny, which is not a LooksAtAPoint'), contains('ask for Tiny')),
          ),
        ),
      );
      await finish(tester, game.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets('with restartOnReload a hot reload starts it over; without, it keeps the game', (tester) async {
      final made = <Tiny>[];
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      Widget game({required bool restart}) => sized(
        FynkelGame(
          () => (made..add(Tiny())).last,
          restartOnReload: restart,
          source: source,
          builder: (context, game) => FynkelView(root: game),
        ),
      );
      await tester.pumpWidget(game(restart: false));
      await finish(tester, made.single.loaded);
      await reload(tester);
      expect(made, hasLength(1));
      await tester.pumpWidget(game(restart: true));
      await reload(tester);
      expect(made, hasLength(2));
      expect(made.first.isAlive, isFalse);
      await finish(tester, made.last.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets("game.world is there in a FynkelGame's builder, so a widget beside the view takes its input", (
      tester,
    ) async {
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      Input? taken;
      await tester.pumpWidget(
        sized(
          FynkelGame(
            Tiny.new,
            source: source,
            builder: (context, game) {
              taken = game.world.input;
              return FynkelView(root: game);
            },
          ),
        ),
      );
      final game = FynkelGame.of<Tiny>(tester.element(find.byType(FynkelView))).game;
      final world = game.world;
      expect(taken, same(world.input));
      expect(tester.state<FynkelViewState>(find.byType(FynkelView)).world, same(world), reason: 'the view shows it');
      await finish(tester, game.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
      expect(world.isDisposed, isTrue, reason: 'the game takes its world with it');
    });

    testWidgets("what a FynkelGame's root throws in onLoad shows over its view, or is reported without one", (
      tester,
    ) async {
      await tester.pumpWidget(
        sized(
          FynkelGame(LooksAtAPoint.new, builder: (context, game) => FynkelView(root: game)),
          width: 800,
          height: 600,
        ),
      );
      expect('${tester.takeException()}', contains('give it target - eye'));
      await tester.pump();
      expect(find.textContaining('give it target - eye'), findsOneWidget, reason: 'heard before the view was');
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));

      await tester.pumpWidget(sized(FynkelGame(LooksAtAPoint.new, builder: (context, game) => const SizedBox())));
      expect('${tester.takeException()}', contains('give it target - eye'), reason: 'no view took it');
      await tester.pumpWidget(const SizedBox());
    });

    testWidgets("a source given to a FynkelGame's view, not the game, is a clear error", (tester) async {
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      late Tiny game;
      await tester.pumpWidget(
        sized(
          FynkelGame(
            Tiny.new,
            source: source,
            builder: (context, made) => FynkelView(root: game = made, source: source),
          ),
        ),
      );
      expect('${tester.takeException()}', contains('give source: to the FynkelGame'));
      await finish(tester, game.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets('a pause panel beside the view shows itself from world.pausedListenable', (tester) async {
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      late Tiny game;
      await tester.pumpWidget(
        sized(
          FynkelGame(
            Tiny.new,
            source: source,
            builder: (context, made) => Stack(
              children: [
                FynkelView(root: game = made),
                ValueListenableBuilder(
                  valueListenable: made.world.pausedListenable,
                  builder: (context, paused, _) => Text(paused ? 'paused' : 'playing'),
                ),
              ],
            ),
          ),
        ),
      );
      expect(find.text('playing'), findsOneWidget);
      expect(game.world.pausedListenable, same(game.world.pausedListenable));
      game.world.paused = true;
      await tester.pump();
      expect(find.text('paused'), findsOneWidget);
      game.world.paused = false;
      await tester.pump();
      expect(find.text('playing'), findsOneWidget);
      await finish(tester, game.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets("a FynkelApp's game wears the app's theme from its onLoad, and hears it change", (tester) async {
      final made = <Themed>[];
      Widget app(ThemeMode mode) => FynkelApp(
        () => (made..add(Themed())).last,
        theme: ThemeData(brightness: Brightness.light),
        darkTheme: ThemeData(brightness: Brightness.dark),
        themeMode: mode,
        source: bundleSource(Bundle({'assets/grid.png': png})),
      );
      await tester.pumpWidget(app(ThemeMode.dark));
      final game = made.single;
      expect(game.atLoad, Brightness.dark, reason: 'there before the root was added');
      expect(game.world.isDark, isTrue);
      await tester.pumpWidget(app(ThemeMode.light));
      for (var i = 0; i < 30; i++) {
        await tester.pump(const Duration(milliseconds: 20));
      }
      expect(game.world.isDark, isFalse);
      expect(game.heard.last, Brightness.light);
      expect(game.heard.length, greaterThan(1), reason: 'each step of the fade');
      expect(made, hasLength(1), reason: 'the same game, not a new one');
      await finish(tester, game.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets("a view that makes its own world gives it the theme around it", (tester) async {
      final game = Themed();
      await tester.pumpWidget(
        Theme(
          data: ThemeData(brightness: Brightness.dark),
          child: sized(FynkelView(root: game, source: bundleSource(Bundle({'assets/grid.png': png})))),
        ),
      );
      expect(game.atLoad, Brightness.dark);
      await finish(tester, game.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets('a view with no FynkelGame around it gives F5 to the game', (tester) async {
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await tester.pump();
      await tester.sendKeyDownEvent(LogicalKeyboardKey.f5);
      world.update(1 / 60);
      expect(world.input.isPressed(KeyCode.f5), isTrue);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.f5);
    });

    testWidgets('a key held through a pause is held again when it ends', (tester) async {
      await tester.pumpWidget(sized(FynkelView.world(world)));
      await tester.pump();
      world.input.bindAction('dash', [KeyCode.w]);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.keyW);
      await tester.pump(const Duration(milliseconds: 16));
      expect(world.input.isPressed(KeyCode.w), isTrue);
      world.paused = true;
      await tester.pump(const Duration(milliseconds: 16));
      expect(world.input.isPressed(KeyCode.w), isFalse, reason: 'a pause lets go');
      world.paused = false;
      await tester.pump(const Duration(milliseconds: 16));
      await tester.pump(const Duration(milliseconds: 16));
      expect(world.input.isPressed(KeyCode.w), isTrue, reason: 'still held on the keyboard');
      await tester.sendKeyUpEvent(LogicalKeyboardKey.keyW);
      await tester.pump(const Duration(milliseconds: 16));
      expect(world.input.isPressed(KeyCode.w), isFalse);
    });
  });

  group('several views', () {
    Future<void> frames(WidgetTester tester) async {
      for (var i = 0; i < 3; i++) {
        await tester.pump(const Duration(milliseconds: 16));
      }
    }

    // A main view, and a small one over its top-left corner.
    Widget both({Camera? camera, bool input = false, bool worldWidgets = true}) => Directionality(
      textDirection: TextDirection.ltr,
      child: Center(
        child: SizedBox(
          width: 200,
          height: 100,
          child: Stack(
            children: [
              Positioned.fill(child: FynkelView.world(world)),
              Positioned(
                left: 0,
                top: 0,
                width: 50,
                height: 50,
                child: FynkelView.world(world, camera: camera, input: input, worldWidgets: worldWidgets),
              ),
            ],
          ),
        ),
      ),
    );

    testWidgets('two views of one world update it once a frame', (tester) async {
      final counter = Counter();
      world.add(GameObject(components: [counter]));
      await tester.pumpWidget(both());
      for (var i = 0; i < 10; i++) {
        await tester.pump(const Duration(milliseconds: 20));
      }
      // As for one view: an update each frame, the first of 0 seconds.
      expect(counter.updates, 10, reason: 'once each frame, not twice');
      expect(world.time.sinceStart, closeTo(0.18, 1e-6), reason: 'time moves once, not twice');
    });

    testWidgets("a second view of a root another view made a world for shows that world, and doesn't free it", (
      tester,
    ) async {
      final root = GameObject();
      Widget views({bool minimap = true}) => Directionality(
        textDirection: TextDirection.ltr,
        child: Stack(
          children: [
            Positioned.fill(child: FynkelView(root: root)),
            if (minimap)
              Positioned(left: 0, top: 0, width: 50, height: 50, child: FynkelView(root: root, input: false)),
          ],
        ),
      );
      await tester.pumpWidget(views());
      await frames(tester);
      expect(tester.takeException(), isNull);
      final made = root.world;
      final states = tester.stateList<FynkelViewState>(find.byType(FynkelView)).toList();
      expect(states.map((s) => s.world), everyElement(same(made)));
      await tester.pumpWidget(views(minimap: false));
      await frames(tester);
      expect(made.isDisposed, isFalse, reason: 'the minimap only showed it');
      await tester.pumpWidget(const SizedBox());
      expect(made.isDisposed, isTrue, reason: 'the view that made it frees it');
    });

    testWidgets("a view joining another's world doesn't restart its time", (tester) async {
      await tester.pumpWidget(sized(FynkelView.world(world)));
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 20));
      }
      await tester.pumpWidget(both());
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 20));
      }
      // Ten frames 20 ms apart, the first of 0 seconds; a restart would
      // lose another 20 ms.
      expect(world.time.sinceStart, closeTo(0.18, 1e-6));
    });

    testWidgets('a view without input keeps taps on it from the game', (tester) async {
      await tester.pumpWidget(both());
      await frames(tester);
      final view = tester.getRect(find.byType(FynkelView).last);
      final on = await tester.startGesture(view.center);
      await frames(tester);
      expect(world.input.fingers, isEmpty);
      await on.up();
      final beside = await tester.startGesture(view.center + const Offset(100, 0));
      await frames(tester);
      expect(world.input.fingers, hasLength(1), reason: 'the main view takes them');
      await beside.up();
      await frames(tester);
    });

    testWidgets("a view that takes input aims fingers from its camera", (tester) async {
      final own = Camera();
      world.add(
        GameObject(
          transform: Transform(position: Vec3(0, 5, 0)),
          components: [own],
        ),
      );
      await tester.pumpWidget(sized(FynkelView.world(world, camera: own)));
      await frames(tester);
      expect(world.input.camera, same(own));
    });

    testWidgets("the game's errors show once, in the view that takes input", (tester) async {
      world.add(GameObject(components: [Thrower()]));
      final reported = <FlutterErrorDetails>[];
      final before = FlutterError.onError;
      FlutterError.onError = reported.add;
      addTearDown(() => FlutterError.onError = before);
      await tester.pumpWidget(both());
      await frames(tester);
      FlutterError.onError = before;
      expect(reported.where((e) => e.exception is StateError), hasLength(1));
      expect(find.byType(ErrorPanel), findsOneWidget);
    });

    // Split screen: two halves, each taking input unless told.
    Widget halves({bool secondInput = true, bool firstInput = true, bool firstShowsErrors = true}) => Directionality(
      textDirection: TextDirection.ltr,
      child: Row(
        children: [
          SizedBox(
            width: 300,
            height: 200,
            child: FynkelView.world(world, input: firstInput, showErrors: firstShowsErrors),
          ),
          SizedBox(width: 100, height: 100, child: FynkelView.world(world, input: secondInput)),
        ],
      ),
    );

    testWidgets("two views of one world that take input: the first takes it, it's said once, errors show once", (
      tester,
    ) async {
      world.add(GameObject(components: [Thrower()]));
      final reported = <FlutterErrorDetails>[];
      final before = FlutterError.onError;
      FlutterError.onError = reported.add;
      addTearDown(() => FlutterError.onError = before);
      await tester.pumpWidget(halves());
      await frames(tester);
      await frames(tester);
      FlutterError.onError = before;
      expect(reported.where((e) => e.exception is StateError), hasLength(1), reason: "the game's error, once");
      expect(find.byType(ErrorPanel), findsOneWidget);
      expect(
        reported.where((e) => '${e.exception}'.contains('give input: false to all but one')),
        hasLength(1),
        reason: 'the mistake is said once',
      );
      expect(world.input.width, 300, reason: 'the first view sizes the input');
      final second = tester.getRect(find.byType(FynkelView).last);
      final on = await tester.startGesture(second.center);
      await frames(tester);
      expect(world.input.fingers, isEmpty, reason: 'the second takes no taps');
      await on.up();
      await frames(tester);
    });

    testWidgets('when the view taking input stops taking it, the next one does', (tester) async {
      final reported = <FlutterErrorDetails>[];
      final before = FlutterError.onError;
      FlutterError.onError = reported.add;
      addTearDown(() => FlutterError.onError = before);
      await tester.pumpWidget(halves());
      await frames(tester);
      await tester.pumpWidget(halves(firstInput: false));
      await frames(tester);
      FlutterError.onError = before;
      expect(world.input.width, 100, reason: 'the second sizes it now');
    });

    testWidgets("a view without input still shows the game's errors, when none that takes input does", (tester) async {
      world.add(GameObject(components: [Thrower()]));
      final reported = <FlutterErrorDetails>[];
      final before = FlutterError.onError;
      FlutterError.onError = reported.add;
      addTearDown(() => FlutterError.onError = before);
      await tester.pumpWidget(sized(FynkelView.world(world, input: false), width: 800, height: 600));
      await frames(tester);
      FlutterError.onError = before;
      expect(reported.where((e) => e.exception is StateError), hasLength(1));
      expect(find.byType(ErrorPanel), findsOneWidget, reason: 'a background behind a title menu shows them too');
    });

    testWidgets('world widgets show over the views that want them', (tester) async {
      world.add(GameObject(components: [WorldWidget((_) => const SizedBox(width: 4, height: 4))]));
      await tester.pumpWidget(both(worldWidgets: false));
      await frames(tester);
      expect(find.byType(WorldWidgetLayer), findsOneWidget);
      await tester.pumpWidget(both());
      await frames(tester);
      expect(find.byType(WorldWidgetLayer), findsNWidgets(2));
    });
  });

  group('covered', () {
    late BuildContext page;

    Widget app({bool pauseWhenCovered = true, void Function(double)? onFrame}) => MaterialApp(
      home: Builder(
        builder: (context) {
          page = context;
          return Center(
            child: sized(FynkelView.world(world, pauseWhenCovered: pauseWhenCovered, onFrame: onFrame)),
          );
        },
      ),
    );

    Future<void> settle(WidgetTester tester) async {
      for (var i = 0; i < 20; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }
    }

    testWidgets('a dialog or a page over the view pauses the game; closing it goes on', (tester) async {
      await tester.pumpWidget(app());
      expect(world.paused, isFalse);
      unawaited(showDialog<void>(context: page, builder: (_) => const Text('menu')));
      await settle(tester);
      expect(world.paused, isTrue);
      Navigator.of(page).pop();
      await settle(tester);
      expect(world.paused, isFalse);
      unawaited(Navigator.of(page).push(MaterialPageRoute<void>(builder: (_) => const Text('settings'))));
      await settle(tester);
      expect(world.paused, isTrue);
      Navigator.of(page).pop();
      await settle(tester);
      expect(world.paused, isFalse);
    });

    testWidgets('covered and paused, it draws 10 frames a second; a game paused by itself keeps its rate', (
      tester,
    ) async {
      final dts = <double>[];
      await tester.pumpWidget(app(onFrame: dts.add));
      unawaited(showDialog<void>(context: page, builder: (_) => const Text('menu')));
      await settle(tester);
      expect(world.paused, isTrue);
      dts.clear();
      for (var i = 0; i < 120; i++) {
        await tester.pump(const Duration(microseconds: 8333));
      }
      expect(dts, hasLength(inInclusiveRange(9, 11)), reason: 'about 10 in a second');
      Navigator.of(page).pop();
      await settle(tester);
      dts.clear();
      world.paused = true;
      for (var i = 0; i < 120; i++) {
        await tester.pump(const Duration(microseconds: 8333));
      }
      expect(dts, hasLength(120), reason: "paused by the game, at its rate (every frame, here)");
    });

    testWidgets('a page over a dialog over the view: the game goes on once both are closed', (tester) async {
      await tester.pumpWidget(app());
      unawaited(showDialog<void>(context: page, builder: (_) => const Text('menu')));
      await settle(tester);
      unawaited(Navigator.of(page).push(MaterialPageRoute<void>(builder: (_) => const Text('settings'))));
      await settle(tester);
      Navigator.of(page).pop();
      await settle(tester);
      expect(world.paused, isTrue, reason: 'the dialog is still over it');
      Navigator.of(page).pop();
      await settle(tester);
      expect(world.paused, isFalse);
    });

    testWidgets('a game paused before it was covered stays paused', (tester) async {
      await tester.pumpWidget(app());
      world.paused = true;
      unawaited(showDialog<void>(context: page, builder: (_) => const Text('menu')));
      await settle(tester);
      Navigator.of(page).pop();
      await settle(tester);
      expect(world.paused, isTrue);
    });

    testWidgets('a game that unpauses and pauses itself again while covered stays paused once uncovered', (
      tester,
    ) async {
      await tester.pumpWidget(app());
      unawaited(showDialog<void>(context: page, builder: (_) => const Text('menu')));
      await settle(tester);
      expect(world.paused, isTrue);
      // The game over screen comes up under the settings page.
      world.paused = false;
      world.paused = true;
      Navigator.of(page).pop();
      await settle(tester);
      expect(world.paused, isTrue, reason: 'the game paused itself: it stays paused');
    });

    testWidgets('a game that unpauses itself while covered runs on once uncovered', (tester) async {
      await tester.pumpWidget(app());
      unawaited(showDialog<void>(context: page, builder: (_) => const Text('menu')));
      await settle(tester);
      world.paused = false;
      Navigator.of(page).pop();
      await settle(tester);
      expect(world.paused, isFalse);
    });

    testWidgets('with pauseWhenCovered off, the game runs on under a dialog', (tester) async {
      await tester.pumpWidget(app(pauseWhenCovered: false));
      unawaited(showDialog<void>(context: page, builder: (_) => const Text('menu')));
      await settle(tester);
      expect(world.paused, isFalse);
      Navigator.of(page).pop();
      await settle(tester);
    });

    testWidgets('a view freed while covered lets the game go on', (tester) async {
      await tester.pumpWidget(app());
      unawaited(showDialog<void>(context: page, builder: (_) => const Text('menu')));
      await settle(tester);
      expect(world.paused, isTrue);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
      expect(world.paused, isFalse);
    });
  });

  testWidgets('a camera whose near is not below its far is reported, and the game goes on', (tester) async {
    world.camera!
      ..gameObject.name = 'eye'
      ..near = 2000;
    var frames = 0;
    await tester.pumpWidget(sized(FynkelView.world(world, onFrame: (_) => frames++)));
    for (var i = 0; i < 10; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(tester.takeException(), isA<ArgumentError>().having((e) => e.message, 'message', contains("'eye'")));
    expect(frames, 10, reason: 'every frame came');
    world.camera!.far = 5000;
    for (var i = 0; i < 130; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(tester.takeException(), isNull, reason: 'it draws once far is past near');
  });

  testWidgets('a throw from onFrame is reported, once a second at most, and the frames go on', (tester) async {
    var frames = 0;
    await tester.pumpWidget(
      sized(
        FynkelView.world(
          world,
          onFrame: (_) {
            if (++frames >= 3) throw StateError('broken onFrame');
          },
        ),
      ),
    );
    for (var i = 0; i < 10; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(tester.takeException().toString(), contains('broken onFrame'));
    expect(frames, 10, reason: 'every frame came');
    for (var i = 0; i < 130; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(tester.takeException().toString(), contains('broken onFrame'), reason: 'reported again a second later');
    expect(frames, 140);
  });

  testWidgets('a lost GPU device brings a new view, up to three times a minute, then the error', (tester) async {
    final dts = <double>[];
    await tester.pumpWidget(sized(FynkelView.world(world, onFrame: dts.add)));
    for (var i = 0; i < 5; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(calls, ['register']);

    Future<void> lose() async {
      final address = views[nextTexture - 1]!;
      expect(fy_view_lose_device(Pointer.fromAddress(address)), FyStatus.FY_STATUS_OK);
      // The render thread destroys it in real time.
      for (var i = 0; i < 200 && tester.takeException() == null; i++) {
        await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 5)));
        await tester.pump(const Duration(milliseconds: 8));
      }
      for (var i = 0; i < 5; i++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
    }

    for (var loss = 1; loss <= 3; loss++) {
      await lose();
      expect(calls, [
        'register',
        for (var i = 0; i < loss; i++) ...['unregister ${7 + i}', 'register'],
      ]);
      expect(find.byType(Texture), findsOneWidget);
      expect(tester.widget<Texture>(find.byType(Texture)).textureId, 7 + loss);
    }
    final before = dts.length;
    await lose();
    expect(find.byType(ErrorWidget), findsOneWidget, reason: 'a fourth in a minute is one too many');
    expect(calls.where((c) => c == 'register'), hasLength(4));
    for (var i = 0; i < 10; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(dts.length, greaterThan(before + 10), reason: 'the world runs on');
  });

  testWidgets('a failed render worker brings a new view, as a lost device does', (tester) async {
    await tester.pumpWidget(sized(FynkelView.world(world)));
    for (var i = 0; i < 5; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(calls, ['register']);
    expect(fy_view_fail_worker(Pointer.fromAddress(views[7]!)), FyStatus.FY_STATUS_OK);
    // The render thread panics in real time.
    for (var i = 0; i < 200 && tester.takeException() == null; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 5)));
      await tester.pump(const Duration(milliseconds: 8));
    }
    for (var i = 0; i < 5; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(calls, ['register', 'unregister 7', 'register']);
    expect(tester.widget<Texture>(find.byType(Texture)).textureId, 8);
    expect(alive(views[8]!), isTrue);
  });

  testWidgets('with no plugin, as in a widget test of the game, the game runs on unseen', (tester) async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, null);
    forgetReset();
    final dts = <double>[];
    await tester.pumpWidget(sized(FynkelView.world(world, onFrame: dts.add)));
    await tester.pump(const Duration(milliseconds: 10));
    // The registration fails in real time.
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 50)));
    for (var i = 0; i < 29; i++) {
      await tester.pump(const Duration(milliseconds: 10));
    }
    expect(dts, hasLength(30));
    expect(world.time.sinceStart, closeTo(0.29, 1e-6), reason: '29 frames of 10 ms after the first');
    expect(find.byType(Texture), findsNothing);
    expect(tester.takeException(), isNull);
  });

  test('a renderScale or shrinkBeyond out of range says the value given', () {
    expect(
      () => FynkelView.world(world, renderScale: 0),
      throwsA(isA<AssertionError>().having((e) => '${e.message}', 'message', contains('renderScale is 0'))),
    );
    expect(
      () => WorldWidget((_) => const SizedBox(), shrinkBeyond: -2),
      throwsA(isA<AssertionError>().having((e) => '${e.message}', 'message', contains('shrinkBeyond is -2'))),
    );
  });

  testWidgets('an unbounded size is a clear error', (tester) async {
    await tester.pumpWidget(
      Directionality(
        textDirection: TextDirection.ltr,
        child: ListView(children: [FynkelView.world(world)]),
      ),
    );
    expect(tester.takeException().toString(), contains('FynkelView needs a bounded size'));
  });
}

/// A cube in front of a camera, counting the taps on it.
class Clickable extends GameObject {
  final clicks = ValueNotifier(0);

  @override
  void onLoad() {
    super.onLoad();
    add(
      GameObject(
        transform: Transform(position: Vec3(0, 0, 4)),
        components: [Camera()],
      ),
    );
    add(GameObject(components: [MeshRenderer(Mesh.cube(), Material()), _Counts()]));
  }
}

class _Counts extends Behaviour {
  @override
  bool onClick(Finger finger) {
    (gameObject.parent! as Clickable).clicks.value++;
    return true;
  }
}

/// Counts its updates.
class Counter extends Behaviour {
  int updates = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    updates++;
  }
}

/// Loads a shader from its world's source.
class _LoadsAShader extends GameObject {
  @override
  Future<void> onLoad() async {
    await assets.shader('flat.wgsl');
    await super.onLoad();
  }
}

/// Throws once, in its first update.
class Thrower extends Behaviour {
  bool _threw = false;

  @override
  void onUpdate() {
    super.onUpdate();
    if (_threw) return;
    _threw = true;
    throw StateError('thrown');
  }
}
