// Editor extensions (ADR-0028 step 6): during Play an extension's context
// has the running game — its world, and the toolbar's Pause, Step and
// Stop; stopped, it has none. A window follows Play starting and ending.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/gameplay.dart' as fynkel;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor/src/play/editor_game.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'support/fakes.dart';

/// The game: one world with two coins, shown to the editor's Play.
class _Game extends StatefulWidget {
  const _Game(this.launch);

  final FynkelLaunch launch;

  @override
  State<_Game> createState() => _GameState();
}

class _GameState extends State<_Game> {
  final world = fynkel.World.create();
  FynkelPlayControl? _play;

  @override
  void initState() {
    super.initState();
    world.build(fynkel.ObjectSpec('Coin'));
    world.build(fynkel.ObjectSpec('Coin'));
    world.step();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _play = FynkelPlayScope.maybeOf(context)?..attach(world);
  }

  @override
  void dispose() {
    _play?.detach(world);
    world.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => const SizedBox.expand();
}

final _seen = <String>[];

List<fynkel.GameObject> _coins(fynkel.World world) => world.findAll('Coin');

void _tools(EditorExtensions editor) {
  editor.menuItem(
    'Tools/Collect Coins',
    id: 'game.collectCoins',
    enabled: (context) => context.play != null,
    run: (context) {
      final world = context.play!.world;
      final coins = _coins(world);
      for (final c in coins) {
        c.destroy();
      }
      _seen.add('collected ${coins.length}');
    },
  );
  editor.window(
    'game.playTools',
    title: 'Play Tools',
    menu: 'Tools/Play Tools',
    build: (context, editor) {
      final play = editor.play;
      return Column(
        children: [
          Text(play == null ? 'stopped' : 'coins ${_coins(play.world).length}', key: const Key('play.state')),
          if (play != null) ...[
            TextButton(key: const Key('play.pause'), onPressed: play.pause, child: const Text('Pause')),
            TextButton(key: const Key('play.step'), onPressed: play.step, child: const Text('Step')),
            TextButton(key: const Key('play.stop'), onPressed: play.stop, child: const Text('Stop')),
          ],
        ],
      );
    },
  );
}

void main() {
  setUp(_seen.clear);

  testWidgets('during Play the context has the running world, Pause, Step and Stop; stopped, none', (tester) async {
    final project = FakeProject(dir: '/fake/game');
    await pumpLarge(
      tester,
      shellApp(
        prefs: Preferences.inMemory(),
        projects: {'/fake/game': project},
        initialProject: '/fake/game',
        runner: RecordingRunner(),
        game: EditorGame(name: 'game', projectDir: '/fake/game', build: _Game.new),
        extensions: const [EditorPart('game', _tools)],
      ),
    );
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    shell.registry.execute('window.panel.ext:game.playTools');
    await tester.pumpAndSettle();
    expect(find.text('stopped'), findsOneWidget);
    expect(shell.registry.byId('game.collectCoins')!.enabled!(), isFalse, reason: 'no game runs');

    var refreshed = 0;
    void count() => refreshed++;
    shell.registry.addListener(count);
    addTearDown(() => shell.registry.removeListener(count));
    shell.registry.execute('play.start');
    await tester.pumpAndSettle();
    expect(refreshed, greaterThan(0), reason: 'the menus look again once the world shows');
    expect(find.text('coins 2'), findsOneWidget, reason: 'the window follows Play starting');
    expect(shell.registry.byId('game.collectCoins')!.enabled!(), isTrue);

    // Pause and Step, as the toolbar's.
    await tester.tap(find.byKey(const Key('play.pause')));
    await tester.pump();
    expect(shell.gameRun!.paused, isTrue);
    final redraws = shell.gameRun!.control.redraws;

    // A change to the world while paused is drawn at once.
    shell.registry.execute('game.collectCoins');
    await tester.pumpAndSettle();
    expect(_seen, ['collected 2']);
    expect(_coins(shell.gameRun!.control.world!..step()), isEmpty, reason: 'gone from the next tick');
    expect(shell.gameRun!.control.redraws, greaterThan(redraws));

    await tester.tap(find.byKey(const Key('play.step')));
    await tester.pump();
    expect(shell.gameRun!.control.pendingSteps, 1);

    await tester.tap(find.byKey(const Key('play.stop')));
    await tester.pumpAndSettle();
    expect(shell.gameRun!.running, isFalse);
    expect(find.text('stopped'), findsOneWidget, reason: 'and Play ending');
  });
}
