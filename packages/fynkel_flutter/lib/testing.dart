/// For a game's widget tests: move the game on by frames or seconds, and
/// find it on screen.
///
/// ```dart
/// testWidgets('Play starts a round', (tester) async {
///   await tester.pumpWidget(MyApp());
///   await tester.tap(find.text('Play'));
///   await tester.run(0.5);
///   expect(tester.game<MyGame>().round, 1);
/// });
/// ```
///
/// A game's frames go on for ever, so `pumpAndSettle` never settles while
/// one is on screen: [FynkelTester.frames] and [FynkelTester.run] pump
/// frames of a 60th of a second instead.
library;

import 'package:flutter/widgets.dart' show StatefulElement;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';

import 'src/fynkel_game.dart';
import 'src/fynkel_view.dart';

/// [frames], [run] and [game] on a [WidgetTester].
extension FynkelTester on WidgetTester {
  /// Pumps [count] frames, a 60th of a second each: the world runs as on a
  /// 60 Hz display, so once a frame at its default `targetFrameRate` of
  /// 60, every other frame at 30, and 10 times a second while a page
  /// covers it.
  Future<void> frames([int count = 1]) async {
    for (var i = 0; i < count; i++) {
      await pump(const Duration(microseconds: 16667));
    }
  }

  /// Pumps [seconds] of frames, 60 a second, rounded as `world.run`
  /// rounds them.
  Future<void> run(double seconds) => frames((seconds * 60).round());

  /// The game of the first [FynkelView] on screen, as a [T]: the root it
  /// shows ([FynkelViewState.root]), made by the game's `FynkelApp` or
  /// `FynkelGame` or given to the view, whatever type that was made for;
  /// for a view of a world, its `FynkelGame`'s. Throws a [StateError] when
  /// there's no view, or its game isn't a [T].
  T game<T extends GameObject>() {
    final views = find.byType(FynkelView).evaluate();
    if (views.isEmpty) throw StateError('game found no FynkelView on screen: pump the app first');
    final view = views.first;
    final game = ((view as StatefulElement).state as FynkelViewState).root ?? FynkelGame.maybeOf(view)?.game;
    if (game == null) {
      throw StateError('game found a FynkelView of a world, with no game: reach what it shows through its world');
    }
    if (game is! T) throw StateError('game<$T> found a ${game.runtimeType}, which is not a $T');
    return game;
  }
}
