import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show KeyCode, Quality, Vec3;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/testing.dart';
import 'package:sheepdog/app.dart';
import 'package:sheepdog/game.dart';

void main() {
  Future<void> play(WidgetTester tester, Settings settings) async {
    tester.view.physicalSize = const Size(900, 600);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(SheepdogApp(settings: settings, seed: 1));
    expect(find.text('Sheepdog'), findsOneWidget);
    await tester.tap(find.text('Play'));
    await tester.run(0.5);
    // The sheep and the dog load their models: files read outside the
    // test's clock, and the objects appearing in the world's frames.
    var loaded = false;
    tester.game<SheepdogGame>().loaded.then((_) => loaded = true);
    for (var i = 0; !loaded && i < 100; i++) {
      await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
      await tester.pump(const Duration(microseconds: 16667));
    }
    expect(loaded, isTrue, reason: 'the models loaded');
    await tester.run(0.5);
    expect(find.byType(FynkelView), findsNWidgets(2), reason: 'the game and its minimap');
  }

  testWidgets('title → play; the pause button pauses it under a menu, and Resume goes on', (tester) async {
    await play(tester, Settings());
    final world = tester.game<SheepdogGame>().world;
    expect(find.text('home 0 of 8'), findsOneWidget);
    await tester.tap(find.byTooltip('Pause'));
    await tester.run(0.5);
    expect(find.text('Paused'), findsOneWidget);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Resume'));
    await tester.run(0.5);
    expect(find.text('Paused'), findsNothing);
    expect(world.paused, isFalse);
  });

  testWidgets('Escape opens the menu and closes it; back from it quits to the title, and the world is gone', (
    tester,
  ) async {
    await play(tester, Settings());
    final world = tester.game<SheepdogGame>().world;
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.run(0.5);
    expect(find.text('Paused'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.run(0.5);
    expect(find.text('Paused'), findsNothing);
    expect(world.paused, isFalse);
    await tester.binding.handlePopRoute();
    await tester.run(0.5);
    expect(find.text('Paused'), findsOneWidget);
    await tester.binding.handlePopRoute();
    await tester.run(0.5);
    expect(find.text('Play'), findsOneWidget);
    expect(world.isDisposed, isTrue);
  });

  testWidgets('settings from the pause menu: still paused; night falls in the game; the minimap goes', (tester) async {
    final settings = Settings()..themeMode.value = ThemeMode.light;
    await play(tester, settings);
    final world = tester.game<SheepdogGame>().world;
    final day = world.sky;
    await tester.tap(find.byTooltip('Pause'));
    await tester.run(0.5);
    await tester.tap(find.text('Settings'));
    await tester.run(0.5);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Dark'));
    await tester.run(0.5);
    expect(world.isDark, isTrue, reason: 'the game wears the app’s theme');
    expect(world.sky, isNot(day), reason: 'a night sky');
    await tester.tap(find.text('Minimap'));
    await tester.run(0.5);
    await tester.tap(find.byType(BackButton));
    await tester.run(0.5);
    await tester.tap(find.text('Resume'));
    await tester.run(0.5);
    expect(world.paused, isFalse);
    expect(find.byType(FynkelView), findsOneWidget, reason: 'no minimap');
  });

  testWidgets("the quality setting: the device's pick, then the one chosen, in the game at once", (tester) async {
    final settings = Settings();
    await play(tester, settings);
    final world = tester.game<SheepdogGame>().world;
    expect(world.quality, isNull);
    await tester.tap(find.byTooltip('Pause'));
    await tester.run(0.5);
    await tester.tap(find.text('Settings'));
    await tester.run(0.5);
    await tester.tap(find.text('Low'));
    await tester.run(0.5);
    expect((settings.quality.value, world.quality), (Quality.low, Quality.low));
    await tester.tap(find.text('High'));
    await tester.run(0.5);
    expect(world.quality, Quality.high);
    await tester.tap(find.text('Auto'));
    await tester.run(0.5);
    expect(world.quality, isNull);
  });

  testWidgets('controls: Bark rebound to B in the settings barks on B, not space', (tester) async {
    final settings = Settings();
    await play(tester, settings);
    await tester.tap(find.byTooltip('Pause'));
    await tester.run(0.5);
    await tester.tap(find.text('Settings'));
    await tester.run(0.5);
    await tester.drag(find.byType(ListView), const Offset(0, -300));
    await tester.run(0.5);
    await tester.tap(find.widgetWithText(OutlinedButton, 'Space'));
    await tester.run(0.5);
    expect(find.text('Press a key for Bark'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyB);
    await tester.run(0.5);
    expect(settings.controls['bark'], KeyCode.b);
    await tester.tap(find.byType(BackButton));
    await tester.run(0.5);
    await tester.tap(find.text('Resume'));
    await tester.run(0.5);

    final round = tester.game<SheepdogGame>().round;
    var barks = 0;
    round.dog.barked.listen((_) => barks++);
    await tester.sendKeyEvent(LogicalKeyboardKey.space);
    await tester.run(0.6);
    expect(barks, 0, reason: 'space is not bark any more');
    await tester.sendKeyEvent(LogicalKeyboardKey.keyB);
    await tester.run(0.1);
    expect(barks, 1);
  });

  testWidgets('the Bark button barks', (tester) async {
    await play(tester, Settings());
    var barks = 0;
    tester.game<SheepdogGame>().round.dog.barked.listen((_) => barks++);
    await tester.tap(find.text('Bark'));
    await tester.run(0.1);
    expect(barks, 1);
  });

  testWidgets('the end of a round pops up, paused, with its stars; Play again is the same level, playing', (
    tester,
  ) async {
    await play(tester, Settings());
    final sheepdog = tester.game<SheepdogGame>();
    final world = sheepdog.world;
    final flock = sheepdog.round.flock;
    for (final sheep in flock.take(6)) {
      sheep.position = Vec3(0, 0.4, penFront - 1);
    }
    for (final sheep in flock.skip(6)) {
      sheep.take();
    }
    await tester.run(0.5);
    expect(find.text('6 of 8 home'), findsOneWidget);
    expect(find.bySemanticsLabel('2 of 3 stars'), findsOneWidget);
    expect(find.text('The wolves took 2.'), findsOneWidget);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Play again'));
    await tester.run(0.5);
    expect(find.text('6 of 8 home'), findsNothing);
    expect(world.paused, isFalse);
    expect(find.text('home 0 of 8'), findsOneWidget);
    expect(find.textContaining('level 1'), findsOneWidget);
    expect(sheepdog.round.free, hasLength(8));
  });

  testWidgets('a star or more opens the next level; none, only Play again', (tester) async {
    await play(tester, Settings());
    final sheepdog = tester.game<SheepdogGame>();
    for (final sheep in sheepdog.round.flock.take(4)) {
      sheep.position = Vec3(0, 0.4, penFront - 1);
    }
    for (final sheep in sheepdog.round.flock.skip(4)) {
      sheep.take();
    }
    await tester.run(0.5);
    await tester.tap(find.text('Level 2'));
    await tester.run(0.5);
    expect(sheepdog.level.value, 2);
    expect(find.textContaining('level 2   2:00'), findsOneWidget);
    for (final sheep in sheepdog.round.flock) {
      sheep.take();
    }
    await tester.run(0.5);
    expect(find.text('None home'), findsOneWidget);
    expect(find.bySemanticsLabel('0 of 3 stars'), findsOneWidget);
    expect(find.text('Level 3'), findsNothing);
  });

  testWidgets('a wolf coming shows on the HUD while its mark is out', (tester) async {
    await play(tester, Settings());
    final sheepdog = tester.game<SheepdogGame>();
    await sheepdog.restart(level: 2);
    await tester.run(calm - warning + 0.5);
    expect(find.text('A wolf is coming'), findsOneWidget);
    await tester.run(warning);
    expect(find.text('A wolf is coming'), findsNothing);
  });

  testWidgets('every sheep home is "All home!"; Quit to title leaves the game', (tester) async {
    await play(tester, Settings());
    final sheepdog = tester.game<SheepdogGame>();
    for (final sheep in sheepdog.round.flock) {
      sheep.position = Vec3(0, 0.4, penFront - 1);
    }
    await tester.run(0.5);
    expect(find.text('All home!'), findsOneWidget);
    expect(find.bySemanticsLabel('3 of 3 stars'), findsOneWidget);
    await tester.tap(find.text('Quit to title'));
    await tester.run(0.5);
    expect(find.text('Play'), findsOneWidget);
    expect(sheepdog.world.isDisposed, isTrue);
  });
}
