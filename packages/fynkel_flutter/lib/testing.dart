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
/// frames of a 60th of a second instead. A game that loads files is
/// waited for with [FynkelTester.loaded], and anything else it does that
/// takes files with [FynkelTester.until].
library;

import 'dart:async';

import 'package:flutter/widgets.dart' show StatefulElement;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';

import 'src/fynkel_game.dart';
import 'src/fynkel_view.dart';

/// [frames], [run], [loaded], [until] and [game] on a [WidgetTester].
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

  /// Waits until [object] (the [game] on screen unless given) has loaded
  /// what its `onLoad` loads, and everything added then, and throws its
  /// load's error if it failed: `await tester.loaded()`. The files are
  /// read in real time, which a widget test's frames don't move, so it
  /// waits a little in real time and pumps a frame, over and over: the
  /// game runs meanwhile, as it would. Throws a [TimeoutException] after
  /// [timeout] of real time.
  ///
  /// `await tester.runAsync(() => game.loaded)` would wait for ever: the
  /// game's `onLoad` goes on after each `await` only when a frame is
  /// pumped.
  Future<void> loaded([GameObject? object, Duration timeout = const Duration(seconds: 30)]) async {
    final target = object ?? game<GameObject>();
    (Object, StackTrace)? failed;
    var done = false;
    unawaited(
      target.loaded.then(
        (_) => done = true,
        onError: (Object error, StackTrace stack) {
          failed = (error, stack);
          done = true;
        },
      ),
    );
    await _until(() => done, timeout, () => '$target had not loaded after ${timeout.inSeconds} s');
    if (failed case (final error, final stack)) Error.throwWithStackTrace(error, stack);
  }

  /// Runs the game until [condition] holds, as [loaded] does: a little
  /// real time for files to be read, then a frame, over and over. For
  /// what [loaded] can't be given, as a level a button makes inside the
  /// game: `await tester.until(() => game.level == 2)`. Throws a
  /// [TimeoutException] after [timeout] of real time.
  Future<void> until(bool Function() condition, {Duration timeout = const Duration(seconds: 30)}) =>
      _until(condition, timeout, () => 'what until waited for was not so after ${timeout.inMilliseconds} ms');

  Future<void> _until(bool Function() condition, Duration timeout, String Function() failed) async {
    final waited = Stopwatch()..start();
    while (!condition()) {
      if (waited.elapsed > timeout) throw TimeoutException(failed(), timeout);
      await runAsync(() => Future<void>.delayed(const Duration(milliseconds: 5)));
      await frames();
    }
  }

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
