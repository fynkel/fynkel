// ADR-0021 P2: what a game asks of its host. The editor's Pause stops a
// `FynkelGameView`'s loop and its audio, Step runs one fixed step, both
// through a game's own `MaterialApp` (the editor mounts the whole app);
// `FynkelLaunch.fromEnvironment` finds the project; `FynkelApplication.quit`
// is whatever the host installed.

import 'dart:io';

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

  testWidgets('the editor\'s Pause stops the loop and the audio; Step runs one fixed step; Resume goes on', (
    tester,
  ) async {
    final world = World.create();
    addTearDown(world.dispose);
    world.throwOnDiagnostics = false;
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    final play = FynkelPlayControl();
    addTearDown(play.dispose);
    final view = GlobalKey<FynkelGameViewState>();

    Future<void> frames(int n) async {
      for (var i = 0; i < n; i++) {
        await tester.pump(const Duration(microseconds: 16667));
      }
    }

    // The editor's tree: the scope, then the game's own app below it.
    await tester.pumpWidget(
      FynkelPlayScope(
        control: play,
        child: MaterialApp(
          home: FynkelGameView(key: view, world: world, controller: controller, audio: true),
        ),
      ),
    );
    await frames(6);
    final opened = world.audio.isOpen;
    expect(world.tick, greaterThan(0));
    expect(view.currentState!.hostPaused, isFalse);

    play.paused = true;
    await tester.pump();
    final pausedAt = world.tick;
    final gameplayAtPause = world.context.time.elapsed;
    final realAtPause = world.context.time.realElapsed;
    final simulationAtPause = world.context.time.simulationElapsed;
    expect(view.currentState!.hostPaused, isTrue);
    expect(view.currentState!.paused, isTrue);
    expect(world.audio.stats.suspended, opened, reason: 'Pause silences the game');
    await frames(10);
    expect(world.tick, pausedAt, reason: 'no frames while paused');

    play.step();
    await frames(2);
    expect(world.tick, pausedAt + 1, reason: 'one Step is one fixed step');
    expect(world.context.time.elapsed, gameplayAtPause);
    expect(world.context.time.realElapsed, realAtPause);
    expect(world.context.time.simulationElapsed, greaterThan(simulationAtPause));
    play
      ..step()
      ..step();
    await frames(4);
    expect(world.tick, pausedAt + 3);
    expect(play.pendingSteps, 0);
    expect(world.audio.stats.suspended, opened, reason: 'stepping stays silent');

    // A live edit while paused: drawn again, not stepped.
    play.redraw();
    await frames(2);
    expect(view.currentState!.redrawn, 1);
    expect(world.tick, pausedAt + 3, reason: 'a redraw is not a step');
    await frames(3);
    expect(view.currentState!.redrawn, 1, reason: 'once per redraw');

    play.paused = false;
    await frames(6);
    expect(world.tick, greaterThan(pausedAt + 3));
    expect(world.audio.stats.suspended, isFalse);
    // Host hitches reach the canonical clock unchanged. Scaling/clamping
    // are applied once in the world; real elapsed keeps the whole hitch.
    final realBeforeHitch = world.context.time.realElapsed;
    final gameBeforeHitch = world.context.time.elapsed;
    world.timeScale = 2;
    await tester.pump(const Duration(seconds: 2));
    expect(world.context.time.realElapsed - realBeforeHitch, closeTo(2, 1e-6));
    expect(world.context.time.elapsed - gameBeforeHitch, closeTo(0.25, 1e-6));
    world.timeScale = 1;
    // Step means nothing while running.
    play.step();
    expect(play.pendingSteps, 0);
    play.redraw();
    await frames(2);
    expect(view.currentState!.redrawn, 1, reason: 'a running view draws every frame anyway');

    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });

  testWidgets('a Game view registers the world it shows with the play scope (ADR-0021 P7)', (tester) async {
    final a = World.create();
    addTearDown(a.dispose);
    final b = World.create();
    addTearDown(b.dispose);
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    final play = FynkelPlayControl();
    addTearDown(play.dispose);
    var changes = 0;
    play.worldsChanged.addListener(() => changes++);

    Widget tree(World w) => FynkelPlayScope(
      control: play,
      child: MaterialApp(home: FynkelGameView(world: w, controller: controller)),
    );
    await tester.pumpWidget(tree(a));
    expect(play.world, same(a));
    await tester.pump();
    expect(changes, 1, reason: 'told after the build, not during it');

    await tester.pumpWidget(tree(b));
    expect(play.worlds, [b], reason: 'the view shows another world');
    await tester.pumpWidget(const SizedBox());
    expect(play.worlds, isEmpty);
    expect(play.world, isNull);
    await tester.pump();
    expect(changes, greaterThanOrEqualTo(2));
  });

  testWidgets('the game\'s own pause (setPaused) and the editor\'s are independent', (tester) async {
    final world = World.create();
    addTearDown(world.dispose);
    final controller = FynkelViewportController(placeholders: false);
    addTearDown(controller.dispose);
    final play = FynkelPlayControl();
    addTearDown(play.dispose);
    final view = GlobalKey<FynkelGameViewState>();
    await tester.pumpWidget(
      FynkelPlayScope(
        control: play,
        child: MaterialApp(home: FynkelGameView(key: view, world: world, controller: controller)),
      ),
    );
    await tester.pump(const Duration(microseconds: 16667));
    view.currentState!.setPaused(true);
    play.paused = true;
    await tester.pump();
    play.paused = false;
    await tester.pump();
    expect(view.currentState!.paused, isTrue, reason: 'the game\'s pause menu is still up');
    view.currentState!.setPaused(false);
    expect(view.currentState!.paused, isFalse);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
  });

  test('FynkelLaunch.fromEnvironment: FYNKEL_PROJECT, else a working directory holding the project', () {
    final dir = Directory.systemTemp.createTempSync('fynkel_launch');
    addTearDown(() => dir.deleteSync(recursive: true));
    expect(
      FynkelLaunch.fromEnvironment(environment: {'FYNKEL_PROJECT': '/p', 'FYNKEL_SCENE': 'abc'}).projectDir,
      '/p',
    );
    expect(FynkelLaunch.fromEnvironment(environment: {'FYNKEL_SCENE': 'abc'}).startScene, 'abc');
    expect(FynkelLaunch.fromEnvironment(environment: {}, workingDirectory: dir.path).projectDir, isNull);
    File('${dir.path}/fynkel.project.json').writeAsStringSync('{}');
    final l = FynkelLaunch.fromEnvironment(environment: {}, workingDirectory: dir.path);
    expect(l.projectDir, dir.path);
    expect(l.startScene, isNull);
    expect(l.inEditor, isFalse);
  });

  test('FynkelApplication.quit runs what the host installed (the editor: end Play)', () {
    final before = FynkelApplication.onQuit;
    addTearDown(() => FynkelApplication.onQuit = before);
    var quits = 0;
    FynkelApplication.onQuit = () => quits++;
    FynkelApplication.quit();
    expect(quits, 1);
  });
}
