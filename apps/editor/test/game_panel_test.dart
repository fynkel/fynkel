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
import 'package:fynkel_authoring/fynkel_authoring.dart' show Command;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/commands/commands.dart';
import 'package:fynkel_editor/src/play/editor_game.dart';
import 'package:fynkel_editor/src/play/game_panel.dart';
import 'package:fynkel_editor/src/play/game_preview.dart';
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

Map<String, Object?> _camera(int priority, {bool enabled = true}) => {
  'type': 'fynkel.Camera',
  'version': 1,
  if (!enabled) 'enabled': false,
  'data': {'projection': 'perspective', 'fov_y': 0.9, 'near': 0.2, 'far': 80, 'priority': priority},
};

/// A scene with three cameras: the highest priority wins, a disabled one
/// never does.
FakeDocument _scene() => FakeDocument(
  path: '/fake/game/assets/scenes/main.scene.json',
  id: 'main',
  name: 'Main',
  relativePath: 'scenes/main.scene.json',
  entities: {
    'low': FakeDocument.entity('Low Camera', position: [0, 1, 5], components: {'c': _camera(0)}),
    'main': FakeDocument.entity('Main Camera', position: [0, 3, 12], components: {'c': _camera(2)}),
    'tie': FakeDocument.entity('Tied Camera', position: [0, 3, 12], components: {'c': _camera(2)}),
    'off': FakeDocument.entity('Off Camera', components: {'c': _camera(9, enabled: false)}),
    'floor': FakeDocument.entity('Floor'),
  },
);

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

  test('Pause before Play starts the game paused and stays on across Stop (Unity)', () {
    final run = GameRun(game: _game());
    addTearDown(run.dispose);
    run.togglePause();
    expect(run.paused, isTrue);
    expect(run.running, isFalse);
    run.start(projectDir: '/p');
    expect(run.control.paused, isTrue);
    expect(run.control.pendingSteps, 1, reason: 'the first frame runs, then it pauses');
    run.step();
    expect(run.control.pendingSteps, 2);
    run.stop();
    expect(run.paused, isTrue, reason: 'the toggle is kept');
    run.start(projectDir: '/p');
    expect(run.control.paused, isTrue);
    run.togglePause();
    expect(run.control.paused, isFalse);
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
      expect(
        tester.widget<IconButton>(find.byKey(const Key('window.pause'))).onPressed,
        isNotNull,
        reason: 'Pause may be on before Play',
      );

      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(runner.calls.last.$2, ['cook', '--project', '/fake/game'], reason: 'no preview open: `fynkel cook`');
      expect(find.text('game in /fake/game at null editor=true'), findsOneWidget);
      expect(shell.play!.state, PlayState.running);
      expect(shell.play!.inEditor, isTrue);
      expect(shell.registry.byId('play.start')!.enabled!(), isTrue, reason: '⌘P toggles, as in Unity');
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
      expect(find.text('Open a scene to see what its camera sees.'), findsOneWidget);
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

    testWidgets('Play runs unsaved edits without a prompt and leaves the document dirty (P5)', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      final doc = project.addDocument(_scene());
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
      shell.openDocument(doc.id);
      await tester.pumpAndSettle();
      doc.apply([Command.renameObject('floor', 'Ground')]);
      await tester.pumpAndSettle();
      expect(shell.tabs!.focused!.dirty, isTrue);

      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(find.text('Save changes before playing?'), findsNothing);
      expect(shell.gameRun!.running, isTrue);
      expect(doc.fakePreview!.lastRecookLive, isTrue, reason: 'cooked as the editor holds it');
      expect(runner.calls.where((c) => c.$2.contains('cook')), isEmpty, reason: 'no cook from the files');
      expect(shell.tabs!.focused!.dirty, isTrue, reason: 'nothing was saved');
      expect(find.textContaining('game in /fake/game at main'), findsOneWidget, reason: 'the open scene plays');
      // ⌘P again stops, as in Unity.
      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(shell.gameRun?.running ?? false, isFalse);
    });

    testWidgets('a slow cook shows Importing Assets and blocks the editor; Cancel stops Play (ADR-0040 §4)', (
      tester,
    ) async {
      final project = FakeProject(dir: '/fake/game');
      final doc = project.addDocument(_scene());
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
      shell.openDocument(doc.id);
      await tester.pumpAndSettle();
      final preview = doc.fakePreview!..cookSteps = 50;

      // ⌃⇧⌘R: a short moment shows nothing, but nothing else runs.
      expect(shell.registry.execute('view.recook'), isTrue);
      await tester.pump(const Duration(milliseconds: 100));
      expect(find.byKey(const Key('cook.window')), findsNothing);
      expect(shell.registry.blocked, isTrue);
      expect(shell.registry.execute('view.reload'), isFalse);
      expect(shell.registry.execute('play.start'), isFalse);
      // Past 0.3 s the window shows the file and the count.
      await tester.pump(const Duration(milliseconds: 300));
      expect(find.byKey(const Key('cook.window')), findsOneWidget);
      expect(find.text('Importing Assets'), findsOneWidget);
      expect(find.textContaining('models/asset'), findsOneWidget);
      expect(find.textContaining('of 50'), findsOneWidget);
      // It runs to the end: the window goes, the commands are back.
      await tester.pump(const Duration(seconds: 1));
      await tester.pump();
      expect(find.byKey(const Key('cook.window')), findsNothing);
      expect(shell.registry.blocked, isFalse);
      expect(preview.calls.last, 'cookFinish');

      // Play waits for its cook; Escape cancels it, and Play does not start.
      expect(shell.registry.execute('play.start'), isTrue);
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.byKey(const Key('cook.window')), findsOneWidget);
      expect(shell.gameRun?.running ?? false, isFalse, reason: 'Play waits for the cook');
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pump(const Duration(milliseconds: 100));
      await tester.pumpAndSettle();
      expect(preview.calls, containsAllInOrder(['cookCancel', 'cookFinish']));
      expect(find.byKey(const Key('cook.window')), findsNothing);
      expect(shell.gameRun?.running ?? false, isFalse, reason: 'a cancelled cook does not play');
      expect(find.textContaining('could not cook'), findsNothing, reason: 'Cancel is not an error');
      expect(shell.registry.blocked, isFalse);

      // Play again, left to finish: the game starts on the new cache.
      preview.cookSteps = 3;
      expect(shell.registry.execute('play.start'), isTrue);
      await tester.pump(const Duration(milliseconds: 100));
      await tester.pumpAndSettle();
      expect(preview.lastRecookLive, isTrue);
      expect(shell.gameRun!.running, isTrue);
      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
    });

    testWidgets('Build and Run runs the saved files as their own app, even in a project editor (P6)', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      final doc = project.addDocument(_scene());
      final launcher = FakePlayLauncher();
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          store: store,
          projects: {'/fake/game': project},
          initialProject: '/fake/game',
          runner: runner,
          game: _game(),
          startPlayProcess: launcher.call,
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      shell.openDocument(doc.id);
      await tester.pumpAndSettle();
      doc.apply([Command.renameObject('floor', 'Ground')]);
      await tester.pumpAndSettle();

      expect(shell.registry.byId('play.buildAndRun')!.label, 'Build and Run');
      shell.registry.execute('play.buildAndRun');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('dialog.playSave')), findsOneWidget, reason: 'its own app reads the files');
      await tester.tap(find.byKey(const Key('playSave.save')));
      for (var i = 0; i < 6; i++) {
        await tester.pump();
      }
      await tester.pump(const Duration(milliseconds: 400));
      expect(shell.tabs!.focused!.dirty, isFalse);
      expect(launcher.calls.single.$2.take(3), ['run', '--project', '/fake/game']);
      expect(shell.gameRun!.running, isFalse, reason: 'not in the Game view');
      expect(shell.play!.inEditor, isFalse);
      expect(shell.registry.byId('play.buildAndRun')!.enabled!(), isFalse, reason: 'one run at a time');

      expect(shell.play!.isActive, isTrue);

      await tester.runAsync(() async => launcher.last.exitWith(0));
      for (var i = 0; i < 6; i++) {
        await tester.pump();
      }
      await tester.pump(const Duration(seconds: 1));
      expect(shell.play!.isActive, isFalse);
      expect(shell.registry.byId('play.buildAndRun')!.enabled!(), isTrue);
    });

    testWidgets('between runs the Game view shows what the active camera sees', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      final doc = project.addDocument(_scene());
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
      shell.openDocument(doc.id);
      await tester.pumpAndSettle();
      shell.workspace!.show(PanelRegistry.game);
      await tester.pumpAndSettle();
      final tab = shell.tabs!.focused!;
      expect(activeCamera(tab)!.name, 'Main Camera', reason: 'highest priority, first on a tie, never a disabled one');
      Text camera() => tester.widget<Text>(find.byKey(const Key('game.camera')));
      expect(camera().data, 'Main Camera');

      // The toolbar: aspect, scale and Stats, kept across Play.
      await tester.tap(find.byKey(const Key('game.aspect')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('game.aspect.wide')));
      await tester.pumpAndSettle();
      expect(find.text('16:9'), findsOneWidget);
      await tester.tap(find.byKey(const Key('game.stats')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('game.statsBox')), findsOneWidget);
      expect(find.text('Resolution'), findsOneWidget);

      // Play replaces the picture with the game; Stop brings it back.
      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(find.text('16:9'), findsOneWidget);
      expect(find.byKey(const Key('game.statsBox')), findsOneWidget);
      shell.registry.execute('play.stop');
      await tester.pumpAndSettle();
      expect(camera().data, 'Main Camera');

      // Without a camera the view says so.
      shell.hierarchy!.select('low');
      for (final id in ['main', 'tie', 'off']) {
        shell.hierarchy!.select(id, toggle: true);
      }
      shell.registry.execute('hierarchy.delete');
      await tester.pumpAndSettle();
      expect(find.textContaining('Main has no camera'), findsOneWidget);
      expect(camera().data, 'No camera');
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
