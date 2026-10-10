import 'dart:async';
import 'dart:ffi' show AllocatorAlloc, Pointer;
import 'dart:io';

import 'package:ffi/ffi.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart' hide Transform;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel/src/bindings.g.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/src/fynkel_view.dart' show channel, debugAndroid, forgetReset;

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

  setUp(() {
    world = World();
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
    // The Swift or Kotlin plugin, as far as Dart can see it.
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
      final args = call.arguments as Map? ?? const {};
      switch (call.method) {
        case 'register':
          if (debugAndroid ?? false) {
            expect(args.keys, unorderedEquals(['view', 'width', 'height']));
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
    await tester.pumpWidget(
      sized(FynkelView(world: world, resolutionScale: 0.5, onFrame: dts.add, onStats: stats.add)),
    );
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

    await tester.pumpWidget(sized(FynkelView(world: world, onStats: stats.add), width: 60, height: 30));
    for (var i = 0; i < 300; i++) {
      await tester.pump(const Duration(milliseconds: 8));
    }
    expect(stats.last.size, const Size(120, 60), reason: 'resized, at scale 1 now');
    expect(calls, hasLength(1), reason: 'a resize keeps the texture');
  });

  testWidgets("after each frame it posts, onViewStats has the view's counts so far, so a load can be timed", (
    tester,
  ) async {
    final seen = <ViewStats>[];
    final ticks = <int>[];
    await tester.pumpWidget(
      sized(FynkelView(world: world, onFrame: (_) => ticks.add(seen.length), onViewStats: seen.add)),
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
    await tester.pumpWidget(sized(FynkelView(world: world)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(find.byType(Texture), findsOneWidget);
    await tester.pumpWidget(sized(const SizedBox()));
    await tester.pump();
    expect(calls, ['register', 'unregister 7']);
  });

  testWidgets('removed while its texture is being registered, the view is let go once that finishes', (tester) async {
    final held = hold = Completer<void>();
    await tester.pumpWidget(sized(FynkelView(world: world)));
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
    await tester.pumpWidget(sized(FynkelView(world: world)));
    await tester.pump(const Duration(milliseconds: 8));
    final other = World();
    addTearDown(other.dispose);
    await tester.pumpWidget(sized(FynkelView(world: other)));
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, ['register', 'unregister 7', 'register']);
    expect(tester.widget<Texture>(find.byType(Texture)).textureId, 8);
    await tester.pumpWidget(const SizedBox());
  });

  // A hot restart starts a new isolate and disposes nothing, so the first
  // view of each isolate has the plugin free what an earlier one left.
  testWidgets('the plugin is reset once per isolate, before its first view registers', (tester) async {
    forgetReset();
    await tester.pumpWidget(sized(FynkelView(world: world)));
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pumpWidget(sized(const SizedBox()));
    final other = World();
    addTearDown(other.dispose);
    await tester.pumpWidget(sized(FynkelView(world: other)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, ['register', 'unregister 7', 'register']);
    expect(resets, [0]);
    await tester.pumpWidget(sized(const SizedBox()));
  });

  // On Android a view laid out before its window has a size would get a
  // surface of its own (Flutter makes it 1 × 1), only to swap it at once.
  testWidgets('an empty layout makes no view until there is a size', (tester) async {
    await tester.pumpWidget(sized(FynkelView(world: world), width: 0));
    await tester.pump(const Duration(milliseconds: 8));
    await tester.pumpWidget(sized(FynkelView(world: world), height: 0));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, isEmpty);
    await tester.pumpWidget(sized(FynkelView(world: world)));
    await tester.pump(const Duration(milliseconds: 8));
    expect(calls, ['register']);
  });

  group('the Android way', () {
    setUp(() => debugAndroid = true);
    tearDown(() => debugAndroid = null);

    // There a new size is a new surface, which only the plugin can make.
    testWidgets('registers with a size and sends each new size, once, after the texture is there', (tester) async {
      final held = hold = Completer<void>();
      await tester.pumpWidget(sized(FynkelView(world: world)));
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pumpWidget(sized(FynkelView(world: world), width: 60, height: 40));
      await tester.pump(const Duration(milliseconds: 8));
      expect(calls, ['register 300x150'], reason: 'nothing to resize before the texture (3 pixels per point)');
      held.complete();
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pump(const Duration(milliseconds: 8));
      expect(calls, ['register 300x150', 'resize 7 180x120']);

      for (var i = 0; i < 10; i++) {
        await tester.pump(const Duration(milliseconds: 8));
      }
      await tester.pumpWidget(sized(FynkelView(world: world), width: 50, height: 100));
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pumpWidget(sized(const SizedBox()));
      await tester.pump();
      expect(calls, ['register 300x150', 'resize 7 180x120', 'resize 7 150x300', 'unregister 7']);
    });

    testWidgets('a failed resize is reported', (tester) async {
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(channel, (call) async {
        if (call.method == 'resize') throw PlatformException(code: 'attach', message: 'no RGBA8 format');
        return call.method == 'register' ? 7 : null;
      });
      await tester.pumpWidget(sized(FynkelView(world: world)));
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pumpWidget(sized(FynkelView(world: world), width: 60, height: 40));
      await tester.pump(const Duration(milliseconds: 8));
      await tester.pump();
      expect(tester.takeException().toString(), contains('no RGBA8 format'));
      expect(find.byType(ErrorWidget), findsOneWidget);
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

    testWidgets('a new root on rebuild is ignored; a world in its place starts over', (tester) async {
      final game = Tiny();
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      final key = GlobalKey<FynkelViewState>();
      await tester.pumpWidget(sized(FynkelView(key: key, root: game, source: source)));
      final own = key.currentState!.world;
      await tester.pumpWidget(sized(FynkelView(key: key, root: Tiny(), source: source)));
      expect(key.currentState!.world, same(own));

      await tester.pumpWidget(sized(FynkelView(key: key, world: world)));
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

    testWidgets("a failed load reaches the world's errors", (tester) async {
      final game = Tiny();
      final key = GlobalKey<FynkelViewState>();
      await tester.pumpWidget(sized(FynkelView(key: key, root: game, source: bundleSource(Bundle({})))));
      final errors = <ObjectError>[];
      key.currentState!.world.errors.listen(errors.add);
      await finish(tester, game.loaded);
      await tester.pump();
      expect(errors.single.error.toString(), contains('no asset assets/grid.png'));
      expect(game.isAlive, isFalse);
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

    testWidgets('F5 starts a FynkelGame over: a new root in a new world, and what was built around it again', (
      tester,
    ) async {
      final made = <Tiny>[];
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      await tester.pumpWidget(
        sized(
          FynkelGame(
            make: () => (made..add(Tiny())).last,
            builder: (context, game) => Stack(
              children: [
                FynkelView(root: game, source: source),
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
      expect(second.world, isNot(same(world)));
      expect(find.text('game 1'), findsOneWidget);
      expect(second.world.input.isDown(KeyCode.f5), isFalse, reason: 'the restart key is not the game\'s');
      await finish(tester, second.loaded);
      await tester.pumpWidget(const SizedBox());
      await tester.pump(const Duration(milliseconds: 1));
    });

    testWidgets('with restartOnReload a hot reload starts it over; without, it keeps the game', (tester) async {
      final made = <Tiny>[];
      final source = bundleSource(Bundle({'assets/grid.png': png}));
      Widget game({required bool restart}) => sized(
        FynkelGame(
          make: () => (made..add(Tiny())).last,
          restartOnReload: restart,
          builder: (context, game) => FynkelView(root: game, source: source),
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

    testWidgets('a view with no FynkelGame around it gives F5 to the game', (tester) async {
      await tester.pumpWidget(sized(FynkelView(world: world)));
      await tester.pump();
      await tester.sendKeyDownEvent(LogicalKeyboardKey.f5);
      world.update(1 / 60);
      expect(world.input.isDown(KeyCode.f5), isTrue);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.f5);
    });
  });

  testWidgets('an unbounded size is a clear error', (tester) async {
    await tester.pumpWidget(
      Directionality(
        textDirection: TextDirection.ltr,
        child: ListView(children: [FynkelView(world: world)]),
      ),
    );
    expect(tester.takeException().toString(), contains('FynkelView needs a bounded size'));
  });
}
