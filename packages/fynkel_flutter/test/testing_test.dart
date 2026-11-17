// Stage 14's GT10: every example's widget tests wrote their own `wait` (15
// copies) and dug the game out of the tree by hand.

import 'dart:io';
import 'dart:typed_data';

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

/// Loads a model in its onLoad.
class _Loads extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    add(await assets.instantiate('Box.glb')).name = 'box';
  }
}

/// Its load fails.
class _Fails extends GameObject {
  @override
  Future<void> onLoad() async {
    await super.onLoad();
    await assets.instantiate('missing.glb');
  }
}

Future<Uint8List> _testAsset(String name) => File('../../tests/assets/gltf/$name').readAsBytes();

/// Makes a new root each time it builds: a slip that's easy to write.
class _Rebuilt extends StatefulWidget {
  const _Rebuilt({super.key});

  @override
  State<_Rebuilt> createState() => _RebuiltState();
}

class _RebuiltState extends State<_Rebuilt> {
  void again() => setState(() {});

  @override
  Widget build(BuildContext context) => MaterialApp(home: FynkelView(root: _Root()));
}

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
      MaterialApp(home: FynkelGame<_Root>(_Level.new, builder: (context, game) => FynkelView.world(game.world))),
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

  testWidgets('game is the root the view shows, though it was built again with a new one', (tester) async {
    final key = GlobalKey<_RebuiltState>();
    await tester.pumpWidget(_Rebuilt(key: key));
    await tester.frames(2);
    final shown = tester.game<_Root>();
    key.currentState!.again();
    await tester.pump();
    expect('${tester.takeException()}', contains('give it a Key to start another'));
    await tester.frames(2);
    expect(tester.game<_Root>(), same(shown));
    expect(tester.game<_Root>().frames, greaterThan(2));
  });

  testWidgets('with no game on screen, game says so', (tester) async {
    await tester.pumpWidget(const SizedBox());
    expect(
      () => tester.game<_Root>(),
      throwsA(isA<StateError>().having((e) => e.message, 'message', contains('no FynkelView'))),
    );
  });

  // Both playtest games: a widget test whose game loads a model couldn't
  // wait for it. `tester.runAsync(() => game.loaded)` hung, as what comes
  // after each await in onLoad waits in the test's fake time for a pump;
  // each game copied a loop from the examples' tests.
  testWidgets('loaded waits for the game to load what it loads', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: FynkelGame(_Loads.new, source: _testAsset, builder: _view),
      ),
    );
    await tester.loaded();
    expect(tester.game<_Loads>().find('box'), isNotNull);
  });

  testWidgets("loaded throws the load's error", (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: FynkelGame(_Fails.new, source: _testAsset, builder: _view),
      ),
    );
    await expectLater(tester.loaded(), throwsA(isA<PathNotFoundException>()));
    // The view says so too, as it does every error of the game's.
    expect(tester.takeException(), isA<PathNotFoundException>());
  });
}

Widget _view(BuildContext context, GameObject game) => FynkelView.world(game.world);
