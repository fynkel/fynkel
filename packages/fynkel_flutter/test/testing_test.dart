// Stage 14's GT10: every example's widget tests wrote their own `wait` (15
// copies) and dug the game out of the tree by hand.

import 'package:flutter/material.dart' hide Transform;
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';
import 'package:fynkel_flutter/testing.dart';

class _Root extends GameObject {
  var frames = 0;

  @override
  void onUpdate() {
    super.onUpdate();
    frames++;
  }
}

class _Level extends _Root {}

class _Other extends GameObject {}

void main() {
  testWidgets('frames and run move the game on that many frames, and game finds it', (tester) async {
    await tester.pumpWidget(const FynkelApp(_Root.new));
    await tester.frames();
    final game = tester.game<_Root>();
    final start = game.frames;
    await tester.frames(3);
    expect(game.frames, start + 3);
    await tester.run(0.5);
    expect(game.frames, start + 33);
    await tester.run(0.02); // 1.2 frames: 1, rounded as world.run rounds
    expect(game.frames, start + 34);
  });

  testWidgets('game is the type asked for, whatever the FynkelGame was made for; asked wrongly, it says what it is', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: FynkelGame<_Root>(make: _Level.new, builder: (context, game) => FynkelView.world(game.world)),
      ),
    );
    await tester.frames();
    expect(tester.game<_Level>(), isA<_Level>());
    expect(tester.game<GameObject>(), isA<_Level>());
    expect(
      () => tester.game<_Other>(),
      throwsA(isA<StateError>().having((e) => e.message, 'message', contains('_Level'))),
    );
  });

  testWidgets("a view given its root has no FynkelGame: game is that root", (tester) async {
    await tester.pumpWidget(MaterialApp(home: FynkelView(root: _Level())));
    await tester.frames();
    expect(tester.game<_Level>().frames, greaterThan(0));
  });

  testWidgets('with no game on screen, game says so', (tester) async {
    await tester.pumpWidget(const SizedBox());
    expect(
      () => tester.game<_Root>(),
      throwsA(isA<StateError>().having((e) => e.message, 'message', contains('no FynkelView'))),
    );
  });
}
