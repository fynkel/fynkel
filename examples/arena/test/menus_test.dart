import 'package:arena/arena.dart';
import 'package:arena/menus.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show KeyCode;
import 'package:fynkel_flutter/fynkel_flutter.dart';

void main() {
  // The game's frames go on for ever, so no pumpAndSettle: a while of them.
  Future<void> wait(WidgetTester tester, [double seconds = 0.5]) async {
    for (var i = 0; i < seconds * 60; i++) {
      await tester.pump(const Duration(microseconds: 16667));
    }
  }

  ArenaGame game(WidgetTester tester) => FynkelGame.of<ArenaGame>(tester.element(find.byType(FynkelView))).game;

  Future<void> play(WidgetTester tester, Settings settings) async {
    tester.view.physicalSize = const Size(800, 600);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(ArenaApp(settings: settings, seed: 1));
    expect(find.text('Arena'), findsOneWidget);
    await tester.tap(find.text('Play'));
    await wait(tester);
    expect(find.byType(FynkelView), findsOneWidget);
  }

  testWidgets('title → play; the pause button pauses it under a menu, and Resume goes on', (tester) async {
    await play(tester, Settings());
    final world = game(tester).world;
    await wait(tester, 2);
    expect(game(tester).wave.value, 1);
    expect(world.paused, isFalse);
    await tester.tap(find.text('II'));
    await wait(tester);
    expect(find.text('Paused'), findsOneWidget);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Resume'));
    await wait(tester);
    expect(find.text('Paused'), findsNothing);
    expect(world.paused, isFalse);
  });

  testWidgets("back opens the pause menu, and back from it leaves the game: the world's gone", (tester) async {
    await play(tester, Settings());
    final world = game(tester).world;
    await tester.binding.handlePopRoute();
    await wait(tester);
    expect(find.text('Paused'), findsOneWidget);
    expect(find.byType(FynkelView), findsOneWidget, reason: 'alive behind the menu');
    expect(world.isDisposed, isFalse);
    await tester.binding.handlePopRoute();
    await wait(tester);
    expect(find.text('Play'), findsOneWidget);
    expect(find.byType(FynkelView), findsNothing);
    expect(world.isDisposed, isTrue);
  });

  testWidgets('Escape and P open the menu, and close it', (tester) async {
    await play(tester, Settings());
    final world = game(tester).world;
    for (final key in [LogicalKeyboardKey.escape, LogicalKeyboardKey.keyP]) {
      await tester.sendKeyEvent(key);
      await wait(tester);
      expect(find.text('Paused'), findsOneWidget);
      expect(world.paused, isTrue);
      await tester.sendKeyEvent(key);
      await wait(tester);
      expect(find.text('Paused'), findsNothing);
      expect(world.paused, isFalse);
    }
  });

  testWidgets('settings from the pause menu: still paused there, the look changes, back to the menu', (tester) async {
    final settings = Settings();
    await play(tester, settings);
    final world = game(tester).world;
    await tester.tap(find.text('II'));
    await wait(tester);
    await tester.tap(find.text('Settings'));
    await wait(tester);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Light'));
    await wait(tester);
    expect(settings.themeMode.value, ThemeMode.light);
    expect(world.isDark, isFalse, reason: 'the game wears the app’s theme');
    await tester.tap(find.byType(BackButton));
    await wait(tester);
    expect(find.text('Paused'), findsOneWidget);
    expect(world.paused, isTrue);
    await tester.tap(find.text('Resume'));
    await wait(tester);
    expect(world.paused, isFalse);
  });

  testWidgets('controls: Dash rebound to F in the settings dashes on F, not space; Escape keeps a key', (tester) async {
    final settings = Settings();
    await play(tester, settings);
    await tester.tap(find.text('II'));
    await wait(tester);
    await tester.tap(find.text('Settings'));
    await wait(tester);
    await tester.tap(find.widgetWithText(OutlinedButton, 'Space'));
    await wait(tester);
    expect(find.text('Press a key for Dash'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.keyF);
    await wait(tester);
    expect(find.widgetWithText(OutlinedButton, 'F'), findsOneWidget);
    expect(settings.controls['dash'], KeyCode.f);
    await tester.tap(find.widgetWithText(OutlinedButton, 'W'));
    await wait(tester);
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await wait(tester);
    expect(settings.controls['up'], KeyCode.w, reason: 'Escape keeps it');
    await tester.tap(find.byType(BackButton));
    await wait(tester);
    await tester.tap(find.text('Resume'));
    await wait(tester);
    final player = game(tester).round.player;
    await tester.sendKeyDownEvent(LogicalKeyboardKey.space);
    await wait(tester, 0.05);
    expect(player.dashing, isFalse, reason: 'space is not dash any more');
    await tester.sendKeyUpEvent(LogicalKeyboardKey.space);
    await tester.sendKeyDownEvent(LogicalKeyboardKey.keyF);
    await wait(tester, 0.05);
    expect(player.dashing, isTrue);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.keyF);
    await wait(tester);
  });

  test('controls: a key another one has swaps with it; reset goes back', () {
    final controls = arenaControls()..set('up', KeyCode.d);
    expect((controls['up'], controls['right']), (KeyCode.d, KeyCode.w));
    controls.reset();
    expect((controls['up'], controls['right']), (KeyCode.w, KeyCode.d));
  });

  testWidgets('Start over is a new round from wave 1, playing', (tester) async {
    await play(tester, Settings());
    final first = game(tester).round;
    await tester.tap(find.text('II'));
    await wait(tester);
    await tester.tap(find.text('Start over'));
    await wait(tester);
    expect(first.isDestroyed, isTrue);
    expect(game(tester).world.paused, isFalse);
    expect(game(tester).score.value, 0);
  });
}
