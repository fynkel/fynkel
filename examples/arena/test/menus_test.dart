import 'package:arena/arena.dart';
import 'package:arena/menus.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show KeyCode;
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/testing.dart';

void main() {
  Future<void> play(WidgetTester tester, Settings settings) async {
    tester.view.physicalSize = const Size(800, 600);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(ArenaApp(settings: settings, seed: 1));
    expect(find.text('Arena'), findsOneWidget);
    await tester.tap(find.text('Play'));
    await tester.run(0.5);
    expect(find.byType(FynkelView), findsOneWidget);
  }

  testWidgets('title → play; the pause button pauses it under a menu, and Resume goes on', (tester) async {
    await play(tester, Settings());
    final world = tester.game<ArenaGame>().world;
    await tester.run(2);
    expect(tester.game<ArenaGame>().wave.value, 1);
    expect(world.paused, isFalse);
    await tester.tap(find.text('II'));
    await tester.run(0.5);
    expect(find.text('Paused'), findsOneWidget);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Resume'));
    await tester.run(0.5);
    expect(find.text('Paused'), findsNothing);
    expect(world.paused, isFalse);
  });

  testWidgets("back opens the pause menu, and back from it leaves the game: the world's gone", (tester) async {
    await play(tester, Settings());
    final world = tester.game<ArenaGame>().world;
    await tester.binding.handlePopRoute();
    await tester.run(0.5);
    expect(find.text('Paused'), findsOneWidget);
    expect(find.byType(FynkelView), findsOneWidget, reason: 'alive behind the menu');
    expect(world.isDisposed, isFalse);
    await tester.binding.handlePopRoute();
    await tester.run(0.5);
    expect(find.text('Play'), findsOneWidget);
    expect(find.byType(FynkelView), findsNothing);
    expect(world.isDisposed, isTrue);
  });

  testWidgets('Escape and P open the menu, and close it', (tester) async {
    await play(tester, Settings());
    final world = tester.game<ArenaGame>().world;
    for (final key in [LogicalKeyboardKey.escape, LogicalKeyboardKey.keyP]) {
      await tester.sendKeyEvent(key);
      await tester.run(0.5);
      expect(find.text('Paused'), findsOneWidget);
      expect(world.paused, isTrue);
      await tester.sendKeyEvent(key);
      await tester.run(0.5);
      expect(find.text('Paused'), findsNothing);
      expect(world.paused, isFalse);
    }
  });

  testWidgets('settings from the pause menu: still paused there, the look changes, back to the menu', (tester) async {
    final settings = Settings();
    await play(tester, settings);
    final world = tester.game<ArenaGame>().world;
    await tester.tap(find.text('II'));
    await tester.run(0.5);
    await tester.tap(find.text('Settings'));
    await tester.run(0.5);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Light'));
    await tester.run(0.5);
    expect(settings.themeMode.value, ThemeMode.light);
    expect(world.isDark, isFalse, reason: 'the game wears the app’s theme');
    await tester.tap(find.byType(BackButton));
    await tester.run(0.5);
    expect(find.text('Paused'), findsOneWidget);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Resume'));
    await tester.run(0.5);
    expect(world.paused, isFalse);
  });

  testWidgets('controls: Dash rebound to F in the settings dashes on F, not space; Escape keeps a key', (tester) async {
    final settings = Settings();
    await play(tester, settings);
    await tester.tap(find.text('II'));
    await tester.run(0.5);
    await tester.tap(find.text('Settings'));
    await tester.run(0.5);
    await tester.tap(find.widgetWithText(OutlinedButton, 'Space'));
    await tester.run(0.5);
    expect(find.text('Press a key for Dash'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyF);
    await tester.run(0.5);
    expect(find.widgetWithText(OutlinedButton, 'F'), findsOneWidget);
    expect(settings.controls['dash'], KeyCode.f);
    await tester.tap(find.widgetWithText(OutlinedButton, 'W'));
    await tester.run(0.5);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.run(0.5);
    expect(settings.controls['up'], KeyCode.w, reason: 'Escape keeps it');
    await tester.tap(find.byType(BackButton));
    await tester.run(0.5);
    await tester.tap(find.text('Resume'));
    await tester.run(0.5);
    final player = tester.game<ArenaGame>().round.player;
    await tester.sendKeyDownEvent(LogicalKeyboardKey.space);
    await tester.run(0.05);
    expect(player.dashing, isFalse, reason: 'space is not dash any more');
    await tester.sendKeyUpEvent(LogicalKeyboardKey.space);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyF);
    await tester.run(0.05);
    expect(player.dashing, isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyF);
    await tester.run(0.5);
  });

  test('controls: a key another one has swaps with it; reset goes back', () {
    final controls = arenaControls()..set('up', KeyCode.d);
    expect((controls['up'], controls['right']), (KeyCode.d, KeyCode.w));
    controls.reset();
    expect((controls['up'], controls['right']), (KeyCode.w, KeyCode.d));
  });

  testWidgets('Start over is a new round from wave 1, playing', (tester) async {
    await play(tester, Settings());
    final first = tester.game<ArenaGame>().round;
    await tester.tap(find.text('II'));
    await tester.run(0.5);
    await tester.tap(find.text('Start over'));
    await tester.run(0.5);
    expect(first.isDestroyed, isTrue);
    expect(tester.game<ArenaGame>().world.paused, isFalse);
    expect(tester.game<ArenaGame>().score.value, 0);
  });
}
