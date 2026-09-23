// ADR-0021 P4: Play in the Game view. In a project editor (the game
// compiled in) Play cooks, mounts `buildGame` in the Game panel playing the
// open scene and gives it the keyboard; the title bar's Pause and Step
// reach the game through `FynkelPlayScope`; the game's Quit ends Play, not
// the editor; the game's editor-channel records reach the Console's Game
// tab in-process; Stop unmounts it. While the game has the keyboard the
// editor's bare keys stay out of it.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show EditorChannel;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/commands/commands.dart';
import 'package:fynkel_editor/src/play/editor_game.dart';
import 'package:fynkel_editor/src/play/game_panel.dart';
import 'package:fynkel_editor/src/play/play_session.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import 'support/fakes.dart';

/// A stand-in for a game: says what it was launched with and whether the
/// editor's Pause reached it, and has a Quit button.
class _Game extends StatelessWidget {
  const _Game(this.launch);

  final FynkelLaunch launch;

  @override
  Widget build(BuildContext context) {
    final play = FynkelPlayScope.maybeOf(context);
    return Directionality(
      textDirection: TextDirection.ltr,
      child: Column(
        children: [
          Text('game in ${launch.projectDir} at ${launch.startScene} editor=${launch.inEditor}'),
          Text('paused=${play?.paused}'),
          TextButton(onPressed: FynkelApplication.quit, child: const Text('Quit')),
        ],
      ),
    );
  }
}

EditorGame _game() => EditorGame(name: 'fake_game', projectDir: '/fake/game', build: _Game.new);

void main() {
  test('a run takes over Quit and the channel, and gives both back on Stop', () {
    final lines = <String>[];
    var quits = 0;
    final run = GameRun(game: _game(), onRecord: lines.add, onQuit: () => quits++);
    addTearDown(run.dispose);
    final quitBefore = FynkelApplication.onQuit;

    run.start(projectDir: '/p', startScene: 'scene-1');
    expect(run.running, isTrue);
    expect(run.launch!.inEditor, isTrue);
    expect(run.launch!.startScene, 'scene-1');
    expect(FynkelApplication.inEditor, isTrue);
    FynkelApplication.quit();
    expect(quits, 1, reason: 'the game\'s Quit ends Play');
    EditorChannel.log('hello');
    expect(lines.single, contains('"message":"hello"'));

    run.step();
    expect(run.paused, isTrue, reason: 'Step pauses first');
    expect(run.control.pendingSteps, 1);
    run.togglePause();
    expect(run.paused, isFalse);

    run.stop();
    expect(run.running, isFalse);
    expect(FynkelApplication.inEditor, isFalse);
    expect(FynkelApplication.onQuit, same(quitBefore));
    EditorChannel.log('after');
    expect(lines, hasLength(1), reason: 'the channel is off again');
  });

  group('the shell', () {
    late Preferences prefs;
    late MemoryLayoutStore store;
    late RecordingRunner runner;

    setUp(() {
      prefs = Preferences.inMemory();
      store = MemoryLayoutStore();
      runner = RecordingRunner();
    });

    testWidgets('Play mounts the game in the Game view; Pause, Step, the game\'s Quit and Stop', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          store: store,
          projects: {'/fake/game': project},
          initialProject: '/fake/game',
          runner: runner,
          game: _game(),
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      expect(shell.workspace!.isShown(PanelRegistry.game), isTrue, reason: 'Game sits beside Scene by default');
      expect(find.byKey(const Key('window.pause')), findsOneWidget);
      expect(tester.widget<IconButton>(find.byKey(const Key('window.pause'))).onPressed, isNull);

      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(runner.calls.last.$2, ['cook', '--project', '/fake/game'], reason: 'no preview open: `fynkel cook`');
      expect(find.text('game in /fake/game at null editor=true'), findsOneWidget);
      expect(shell.play!.state, PlayState.running);
      expect(shell.play!.inEditor, isTrue);
      expect(shell.registry.byId('play.start')!.enabled!(), isFalse);
      expect(find.text('Playing'), findsOneWidget);

      // The game has the keyboard: the editor's bare keys stay out.
      expect(CommandRegistry.focusOwnsBareKeys, isTrue);

      // What the game says reaches the Game tab, in process.
      EditorChannel.log('coin collected');
      await tester.pump();
      expect(shell.play!.records.map((r) => r.message), contains('coin collected'));

      await tester.tap(find.byKey(const Key('window.pause')));
      await tester.pump();
      expect(find.text('paused=true'), findsOneWidget);
      expect(find.text('Paused'), findsOneWidget);
      expect(shell.registry.byId('play.pause')!.label, 'Resume');
      await tester.tap(find.byKey(const Key('window.step')));
      await tester.pump();
      expect(shell.gameRun!.control.pendingSteps, 1);
      shell.registry.execute('play.pause');
      await tester.pump();
      expect(find.text('paused=false'), findsOneWidget);

      // The game's Quit ends Play; the editor stays.
      await tester.tap(find.text('Quit'));
      await tester.pumpAndSettle();
      expect(shell.gameRun!.running, isFalse);
      expect(shell.play!.state, PlayState.exited);
      expect(find.textContaining('game in'), findsNothing);
      expect(find.text('Press Play (⌘P)'), findsOneWidget);
      expect(CommandRegistry.focusOwnsBareKeys, isFalse);

      // Play again is a fresh run; ⌘. stops it.
      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(find.textContaining('game in'), findsOneWidget);
      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.period);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      await tester.pumpAndSettle();
      expect(shell.gameRun!.running, isFalse);
    });

    testWidgets('a cook that fails does not play', (tester) async {
      runner.exitCode = 1;
      runner.stderr = 'FYN-I0001: bad texture';
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          store: store,
          projects: {'/fake/game': FakeProject(dir: '/fake/game')},
          initialProject: '/fake/game',
          runner: runner,
          game: _game(),
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(shell.gameRun!.running, isFalse);
      expect(shell.play!.state, PlayState.idle);
      expect(find.textContaining('FYN-I0001: bad texture'), findsOneWidget);
    });

    testWidgets('the plain editor has a Game view that says where the game runs', (tester) async {
      await pumpLarge(
        tester,
        shellApp(prefs: prefs, store: store, projects: {'/fake/game': FakeProject(dir: '/fake/game')}, initialProject: '/fake/game'),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      shell.workspace!.show(PanelRegistry.game);
      await tester.pumpAndSettle();
      expect(find.text('The game runs in its own window'), findsOneWidget);
      expect(find.byKey(const Key('window.pause')), findsNothing);
      expect(shell.registry.byId('play.pause')!.enabled!(), isFalse);
    });
  });
}
