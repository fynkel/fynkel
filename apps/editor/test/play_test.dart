// Play / Stop and the Console's Game tab (blueprint §19.2e item 8): the
// records the channel writes, the process tree Stop signals, the session
// that holds a run, the filters over its rows, and the shell — a project
// with errors that refuses to play, a run whose diagnostics land in the
// Console and take the user to the entity, Stop, and the banner that says
// an edit applies on the next Play.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/gameplay.dart' show EntityRef;
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/console/console_panel.dart';
import 'package:fynkel_editor/src/play/game_console.dart';
import 'package:fynkel_editor/src/play/game_record.dart';
import 'package:fynkel_editor/src/play/play_session.dart';
import 'package:fynkel_editor/src/play/process_tree.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

String record(Map<String, Object?> fields) => jsonEncode(fields);

/// Lets the process's lines through the decoder and the line splitter.
Future<void> settle() async {
  for (var i = 0; i < 5; i++) {
    await Future<void>.delayed(Duration.zero);
  }
}

/// The same, in a widget test: the decoder, the splitter and the frame.
/// (`pumpAndSettle` is no use while Play runs — the toolbar's spinner
/// never stops scheduling frames.)
Future<void> pumpLines(WidgetTester tester) async {
  for (var i = 0; i < 6; i++) {
    await tester.pump();
  }
  await tester.pump(const Duration(milliseconds: 400)); // the Console's tab slides over
}

/// Pumps until [finder] finds something (or gives up). Stop reads the real
/// process table before it signals, and that read is not on the test's
/// clock: each turn gives the machine a moment (`runAsync`) and then pumps,
/// so the frame after it lands is drawn.
Future<void> pumpUntil(WidgetTester tester, Finder finder, {int frames = 60}) async {
  for (var i = 0; i < frames && finder.evaluate().isEmpty; i++) {
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 20)));
    await tester.pump(const Duration(milliseconds: 20));
  }
}

FakeDocument playScene({required String path, String id = 'scene-p'}) => FakeDocument(
  path: path,
  id: id,
  name: 'Playground',
  relativePath: 'scenes/playground.scene.json',
  onDisk: true,
  entities: {
    'cam': FakeDocument.entity('Camera', position: [0, 2, 6]),
    'player': FakeDocument.entity('Player', children: ['gun']),
    'gun': FakeDocument.entity('Gun', parent: 'player'),
  },
);

void main() {
  group('GameRecord', () {
    test('a record is found behind a `flutter run` prefix; anything else is output', () {
      final r = GameRecord.parse(
        'flutter: ${record({'fynkel': 'diagnostic', 'seq': 12, 'code': 'staleEntity', 'severity': 'error', 'message': 'the entity is gone', 'tick': 84, 'source': 'step', 'document': 'assets/scenes/playground.scene.json', 'pointer': '/entities/player', 'id': 'player', 'entity': '000000010000002a0000000000000001', 'suggestion': 'check the behaviour\'s handle'})}',
      );
      expect(r.kind, GameRecordKind.diagnostic);
      expect(r.severity, Severity.error);
      expect(r.seq, 12);
      expect(r.tick, 84);
      expect(r.code, 'staleEntity');
      expect(r.source, 'step');
      expect(r.id, 'player');
      expect(r.entity, const EntityRef(1, 42, 1));
      expect(r.hasTarget, isTrue);
      expect(r.location(relative: (p) => p.split('/').last), 'playground.scene.json  /entities/player');
      expect(r.location(), 'assets/scenes/playground.scene.json  /entities/player');
      expect(
        r.text,
        'tick 84 error staleEntity: the entity is gone (assets/scenes/playground.scene.json  /entities/player)',
      );

      final plain = GameRecord.parse('Launching lib/main.dart on macOS…');
      expect(plain.kind, GameRecordKind.output);
      expect(plain.severity, Severity.info);
      expect(plain.source, 'stdout');
      expect(plain.hasTarget, isFalse);

      final err = GameRecord.parse('Error: no device', stderr: true);
      expect(err.kind, GameRecordKind.output);
      expect(err.severity, Severity.warning);
      expect(err.source, 'stderr');

      // A line that merely mentions a record is not one.
      expect(GameRecord.parse('saw {"fynkel": not json').kind, GameRecordKind.output);
      // A `fynkel` field of an unknown kind is not a record either.
      expect(GameRecord.parse(record({'fynkel': 'gossip'})).kind, GameRecordKind.output);
    });

    test('log, stats and state records read as the Console shows them', () {
      final log = GameRecord.parse(record({'fynkel': 'log', 'message': 'picked up a coin', 'tick': 7}));
      expect(log.kind, GameRecordKind.log);
      expect(log.source, 'game');
      expect(log.message, 'picked up a coin');
      expect(log.severity, Severity.info);

      final stats = GameRecord.parse(record({'fynkel': 'stats', 'fps': 60, 'ticks': 60, 'summary': '60 fps'}));
      expect(stats.kind, GameRecordKind.stats);
      expect(stats.message, '60 fps');
      expect(stats.stats!['fps'], 60);

      final started = GameRecord.parse(record({'fynkel': 'state', 'state': 'started', 'pid': 99}));
      expect(started.kind, GameRecordKind.state);
      expect(started.message, 'the game process started (pid 99)');

      final loaded = GameRecord.parse(
        record({'fynkel': 'state', 'state': 'loaded', 'scene': 'Playground', 'entities': 12, 'behaviours': 3}),
      );
      expect(loaded.message, 'loaded Playground — 12 entities, 3 behaviours, 0 assets');

      final failed = GameRecord.parse(record({'fynkel': 'state', 'state': 'loadFailed', 'scene': 'Playground'}));
      expect(failed.message, 'the load of Playground failed');

      // Related diagnostics come along.
      final related = GameRecord.parse(
        record({
          'fynkel': 'diagnostic',
          'code': 'FYN-R0006',
          'message': 'the asset has no hierarchy',
          'related': [
            {'code': 'FYN-D0401', 'severity': 'error', 'message': 'named here', 'document': 'a.scene.json'},
          ],
        }),
      );
      expect(related.related.single.code, 'FYN-D0401');
      expect(related.severity, Severity.error); // a diagnostic without a severity is an error
    });

    test('identical records collapse on everything but time, sequence and tick', () {
      String key(int seq, int tick) =>
          GameRecord.keyOf(GameRecord.parse(record({'fynkel': 'log', 'message': 'tick', 'seq': seq, 'tick': tick})));
      expect(key(1, 1), key(2, 900));
    });
  });

  group('ProcessTree', () {
    const table = '''
  100     1
  200   100
  300   200
  301   200
  400     1
''';

    test('the table becomes a tree, deepest first', () {
      final children = ProcessTree.parse(table);
      expect(children[100], [200]);
      expect(children[200], [300, 301]);
      expect(ProcessTree.flatten(children, 100), [300, 301, 200, 100]);
      expect(ProcessTree.flatten(children, 400), [400]);
      expect(ProcessTree.pids(table), {100, 200, 300, 301, 400});
      // A header line, a blank line and a self-parent are ignored.
      expect(ProcessTree.parse('  PID  PPID\n\n  7   7\n  8   1\n')[1], [8]);
    });

    test('Stop signals the tree it saw first, and kills only what survived', () async {
      final sent = <(int, ProcessSignal)>[];
      var live = table;
      final tree = ProcessTree(
        readTable: () async => live,
        send: (pid, signal) {
          sent.add((pid, signal));
          return true;
        },
      );
      // Polite: everything is gone by the time the grace is over.
      final killed = await tree.terminate(
        100,
        exited: Future<void>.microtask(() => live = '  400     1\n'),
        grace: const Duration(seconds: 5),
      );
      expect(killed, isEmpty);
      expect(sent, [
        (300, ProcessSignal.sigterm),
        (301, ProcessSignal.sigterm),
        (200, ProcessSignal.sigterm),
        (100, ProcessSignal.sigterm),
      ]);

      // Stubborn: the same list takes SIGKILL, plus the game's own pid
      // (100 is in the walk already — nobody takes a signal twice).
      sent.clear();
      live = table;
      final second = await tree.terminate(
        100,
        exited: Completer<void>().future,
        grace: Duration.zero,
        also: [400, 100],
      );
      expect(second, [300, 301, 200, 100, 400]);
      expect(sent.where((s) => s.$2 == ProcessSignal.sigterm).map((s) => s.$1), [300, 301, 200, 100, 400]);
      expect(sent.where((s) => s.$2 == ProcessSignal.sigkill).map((s) => s.$1), [300, 301, 200, 100, 400]);
    });
  });

  group('PlaySession', () {
    late FakePlayLauncher launcher;
    late PlaySession session;
    late List<(int, ProcessSignal)> signals;
    var alive = '  7000     1\n  7100  7000\n';

    setUp(() {
      launcher = FakePlayLauncher();
      signals = [];
      alive = '  7000     1\n  7100  7000\n';
      session = PlaySession(
        projectDir: '/g',
        cli: () => '/bin/fynkel',
        start: launcher.call,
        environment: const {'PATH': '/usr/bin'},
        stopGrace: Duration.zero,
        tree: ProcessTree(
          readTable: () async => alive,
          send: (pid, signal) {
            signals.add((pid, signal));
            return true;
          },
        ),
      );
    });

    tearDown(() => session.dispose());

    test('Play runs the CLI with the channel in its environment and reads the run', () async {
      await session.play();
      expect(session.state, PlayState.starting);
      expect(session.statusText, 'Starting…');
      final (executable, arguments, workingDirectory, environment) = launcher.calls.single;
      expect(executable, '/bin/fynkel');
      // The desktop is named: `flutter run` must not pick a phone, a
      // simulator or Chrome, and must not refuse to choose at all.
      expect(arguments, ['run', '--project', '/g', '-d', defaultPlayDevice()]);
      expect(defaultPlayDevice(), Platform.isMacOS ? 'macos' : isNotEmpty);
      expect(workingDirectory, '/g');
      expect(environment, {'PATH': '/usr/bin', 'FYNKEL_EDITOR_CHANNEL': 'stdio'});
      expect(session.pid, launcher.last.pid);

      launcher.last.emit('Launching lib/main.dart on macOS…');
      launcher.last.emit(record({'fynkel': 'state', 'seq': 1, 'state': 'started', 'pid': 7100, 'fixedDt': 1 / 60}));
      await settle();
      expect(session.state, PlayState.running);
      expect(session.gamePid, 7100);
      expect(session.statusText, 'Playing');

      launcher.last.emit(
        record({'fynkel': 'state', 'seq': 2, 'state': 'loaded', 'scene': 'Playground', 'entities': 12}),
      );
      launcher.last.emit(record({'fynkel': 'stats', 'seq': 3, 'fps': 59, 'ticks': 60, 'entities': 12}));
      launcher.last.emit(record({'fynkel': 'log', 'seq': 4, 'message': 'hello', 'tick': 3}));
      await settle();
      expect(session.scene, 'Playground');
      expect(session.statusText, 'Playing · Playground');
      expect(session.lastStats!['fps'], 59);
      expect(session.records.map((r) => r.kind), [
        GameRecordKind.state, // the editor's own launch row
        GameRecordKind.output,
        GameRecordKind.state,
        GameRecordKind.state,
        GameRecordKind.stats,
        GameRecordKind.log,
      ]);

      // stderr is its own stream; its lines are rows too.
      launcher.last.emitError('a warning from the build');
      await settle();
      expect(session.records.last.source, 'stderr');

      launcher.last.exitWith(0);
      await settle();
      expect(session.state, PlayState.exited);
      expect(session.exitCode, 0);
      expect(session.crashed, isFalse);
      expect(session.statusText, 'Exited');
      expect(session.isActive, isFalse);
    });

    test('a crash keeps the exit code and says nothing was changed', () async {
      await session.play();
      launcher.last.emit(record({'fynkel': 'state', 'state': 'started', 'pid': 7100}));
      await settle();
      launcher.last.emit(
        record({'fynkel': 'diagnostic', 'code': 'missedDeadline', 'severity': 'info', 'message': 'late'}),
      );
      launcher.last.exitWith(-9); // `kill -9` from outside
      await settle();
      expect(session.state, PlayState.exited);
      expect(session.exitCode, -9);
      expect(session.crashed, isTrue);
      expect(session.statusText, 'Exited with code -9');
      expect(session.records.last.severity, Severity.error);
      expect(session.records.last.message, contains('nothing in the project was changed'));
      // The line written just before the crash is still there.
      expect(session.records.any((r) => r.code == 'missedDeadline'), isTrue);
      expect(session.errorCount, 1);
    });

    test('a run that dies before the game starts says so and keeps the reason', () async {
      await session.play();
      // What `flutter run` prints when it cannot pick a device: the whole
      // run is over before `package:fynkel` ever writes a record.
      launcher.last.emitError("More than one device connected; please specify a device with the '-d <deviceId>' flag");
      launcher.last.exitWith(1);
      await settle();
      expect(session.state, PlayState.exited);
      expect(session.startedGame, isFalse);
      expect(session.failedToStart, isTrue);
      expect(session.statusText, 'Did not start (code 1)');
      expect(session.records.last.severity, Severity.error);
      expect(session.records.last.message, contains('failed before the game started'));
      expect(session.records.last.message, contains('nothing in the project was changed'));
      // The reason itself is one of the rows.
      expect(session.records.any((r) => r.message.contains('More than one device connected')), isTrue);
    });

    test('Stop signals the tree and the run is not a crash', () async {
      await session.play();
      launcher.last.emit(record({'fynkel': 'state', 'state': 'started', 'pid': 7100}));
      await settle();
      final stopping = session.stop();
      await settle();
      expect(session.state, PlayState.stopping);
      expect(session.statusText, 'Stopping…');
      launcher.last.exitWith(143);
      await settle();
      await stopping;
      expect(session.state, PlayState.exited);
      expect(session.crashed, isFalse); // asked for
      expect(session.wasStopped, isTrue);
      // `-15` (or 143) is the signal Stop sent, not a fault to read as one.
      expect(session.statusText, 'Stopped');
      expect(signals.where((s) => s.$2 == ProcessSignal.sigterm).map((s) => s.$1), [7100, 7000]);
      expect(session.records.last.message, 'the game stopped (exit code 143)');
    });

    test('without the CLI Play says so and stays idle', () async {
      final without = PlaySession(projectDir: '/g', cli: () => null, start: launcher.call);
      addTearDown(without.dispose);
      await without.play();
      expect(without.state, PlayState.idle);
      expect(launcher.calls, isEmpty);
      expect(without.failure, contains('CLI was not found'));
      expect(without.records.single.severity, Severity.error);
    });

    test('a process that will not start is reported, not thrown', () async {
      final broken = PlaySession(
        projectDir: '/g',
        cli: () => '/bin/fynkel',
        start: (executable, arguments, {workingDirectory, environment}) async =>
            throw ProcessException(executable, arguments, 'No such file', 2),
      );
      addTearDown(broken.dispose);
      await broken.play();
      expect(broken.state, PlayState.idle);
      expect(broken.failure, contains('could not start'));
    });

    test('the oldest rows are dropped and counted, and Clear empties the tab', () async {
      final small = PlaySession(projectDir: '/g', cli: () => '/bin/fynkel', start: launcher.call, maxRecords: 4);
      addTearDown(small.dispose);
      await small.play();
      for (var i = 0; i < 10; i++) {
        launcher.last.emit(record({'fynkel': 'log', 'seq': i, 'message': 'line $i'}));
      }
      await settle();
      expect(small.records.length, 4);
      expect(small.droppedRecords, 7); // ten lines plus the launch row
      expect(small.records.last.message, 'line 9');
      small.clear();
      expect(small.records, isEmpty);
      expect(small.droppedRecords, 0);
    });

    test('an edit during a run is marked once, and a new run forgets it', () async {
      await session.play();
      launcher.last.emit(record({'fynkel': 'state', 'state': 'started', 'pid': 7100}));
      await settle();
      var notifications = 0;
      session.addListener(() => notifications++);
      session.markEdited();
      session.markEdited();
      expect(session.editedWhilePlaying, isTrue);
      expect(session.runningOlderFiles, isTrue);
      expect(notifications, 1);
      launcher.last.exitWith(0);
      await settle();
      // Nothing is on screen running the older files any more.
      expect(session.editedWhilePlaying, isTrue);
      expect(session.runningOlderFiles, isFalse);
      await session.play();
      expect(session.editedWhilePlaying, isFalse);
      expect(session.records.first.message, contains('/bin/fynkel run --project /g'));
    });
  });

  group('GameConsoleModel', () {
    late FakePlayLauncher launcher;
    late PlaySession session;
    late GameConsoleModel model;
    late List<GameRecord> opened;

    setUp(() async {
      launcher = FakePlayLauncher();
      session = PlaySession(projectDir: '/g', cli: () => '/bin/fynkel', start: launcher.call);
      opened = [];
      model = GameConsoleModel(
        session: session,
        relative: (path) => path.startsWith('/g/') ? path.substring(3) : path,
        openRecord: opened.add,
      );
      await session.play();
      launcher.last.emit(record({'fynkel': 'state', 'seq': 1, 'state': 'started', 'pid': 7100}));
      launcher.last.emit('Building macOS application…');
      launcher.last.emit(record({'fynkel': 'stats', 'seq': 2, 'fps': 60, 'summary': '60 fps'}));
      launcher.last.emit(record({'fynkel': 'log', 'seq': 3, 'message': 'coin', 'tick': 3}));
      for (var i = 0; i < 3; i++) {
        launcher.last.emit(
          record({
            'fynkel': 'diagnostic',
            'seq': 4 + i,
            'tick': 10 + i,
            'code': 'staleEntity',
            'severity': 'error',
            'message': 'the entity is gone',
            'source': 'step',
            'document': '/g/assets/scenes/playground.scene.json',
            'pointer': '/entities/player',
            'id': 'player',
          }),
        );
      }
      launcher.last.emit(
        record({'fynkel': 'diagnostic', 'seq': 7, 'code': 'tooManyAnchors', 'severity': 'warning', 'message': 'ui'}),
      );
      await settle();
    });

    tearDown(() {
      model.dispose();
      session.dispose();
    });

    test('output and stats are hidden, repeats collapse, and the filters narrow', () {
      // The launch row, started, the log, one collapsed diagnostic, the warning.
      expect(model.entries.length, 5);
      final stale = model.entries.firstWhere((e) => e.record.code == 'staleEntity');
      expect(stale.count, 3);
      expect(stale.record.tick, 12); // the newest of the group
      expect(model.location(stale.record), 'assets/scenes/playground.scene.json  /entities/player');
      expect(model.lineOf(stale), endsWith('(×3)'));
      expect(model.countOf(Severity.error), 3);
      expect(model.countOf(Severity.warning), 1);

      model.setShowOutput(true);
      expect(model.entries.any((e) => e.record.kind == GameRecordKind.output), isTrue);
      model.setShowOutput(false);
      model.setShowStats(true);
      expect(model.entries.any((e) => e.record.kind == GameRecordKind.stats), isTrue);
      model.setShowStats(false);

      model.setCollapse(false);
      expect(model.entries.where((e) => e.record.code == 'staleEntity').length, 3);
      model.setCollapse(true);

      model.toggleSeverity(Severity.info);
      expect(model.entries.map((e) => e.record.code), ['staleEntity', 'tooManyAnchors']);
      model.toggleSeverity(Severity.info);

      model.setQuery('player');
      expect(model.entries.single.record.code, 'staleEntity');
      model.setQuery('tick 12');
      expect(model.entries.single.record.code, 'staleEntity');
      model.setQuery('nothing at all');
      expect(model.entries, isEmpty);
      model.setQuery('');
    });

    test('a run that never started shows the output that says why', () async {
      final own = PlaySession(projectDir: '/g', cli: () => '/bin/fynkel', start: launcher.call);
      addTearDown(own.dispose);
      final view = GameConsoleModel(session: own);
      addTearDown(view.dispose);
      expect(view.showOutput, isFalse);

      await own.play();
      own.markEdited(); // a run is up: nothing about Output changes yet
      launcher.last.emitError("More than one device connected; please specify a device with '-d <deviceId>'");
      await settle();
      expect(view.entries.any((e) => e.record.message.contains('More than one device')), isFalse);

      launcher.last.exitWith(1);
      await settle();
      // The Console shows the reason by itself; the switch says so too.
      expect(view.showOutput, isTrue);
      expect(view.entries.any((e) => e.record.message.contains('More than one device')), isTrue);

      // The user's own choice survives, and the next Play starts over.
      view.setShowOutput(false);
      await own.play();
      await settle();
      expect(view.showOutput, isFalse);
      launcher.last.emit(record({'fynkel': 'state', 'seq': 1, 'state': 'started', 'pid': 7100}));
      launcher.last.exitWith(1); // a real crash: the game had started
      await settle();
      expect(own.failedToStart, isFalse);
      expect(view.showOutput, isFalse);
    });

    test('a row with a target opens it; one without does not', () {
      final stale = model.entries.firstWhere((e) => e.record.code == 'staleEntity');
      model.select(model.keyOf(stale));
      expect(model.canOpenSelected, isTrue);
      model.openSelected();
      expect(opened.single.id, 'player');

      final log = model.entries.firstWhere((e) => e.record.kind == GameRecordKind.log);
      model.select(model.keyOf(log));
      expect(model.canOpenSelected, isFalse);
      model.openSelected();
      expect(opened.length, 1);
      model.activate(log.record);
      expect(opened.length, 1);
    });

    test('copy takes the selection, or every visible row', () async {
      final copied = <String>[];
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        (call) async {
          if (call.method == 'Clipboard.setData') copied.add((call.arguments as Map)['text'] as String);
          return null;
        },
      );
      addTearDown(
        () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
          SystemChannels.platform,
          null,
        ),
      );
      expect(await model.copy(), 5);
      expect(copied.single.split('\n').length, 5);
      final stale = model.entries.firstWhere((e) => e.record.code == 'staleEntity');
      model.select(model.keyOf(stale));
      expect(await model.copy(), 1);
      expect(copied.last, contains('staleEntity'));
    });
  });

  group('the shell', () {
    late Preferences prefs;
    late MemoryLayoutStore store;
    late FakePlayLauncher launcher;

    setUp(() {
      prefs = Preferences.inMemory();
      store = MemoryLayoutStore();
      launcher = FakePlayLauncher();
    });

    testWidgets('a project with errors refuses to Play and points at the Console', (tester) async {
      final project = FakeProject(
        dir: '/fake/game',
        diagnostics: [diag('FYN-D0206', message: 'unknown field "speeed"', document: '/fake/game/a.scene.json')],
      );
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          store: store,
          projects: {'/fake/game': project},
          initialProject: '/fake/game',
          startPlayProcess: launcher.call,
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(launcher.calls, isEmpty);
      expect(shell.play!.state, PlayState.idle);
      expect(shell.consoleTab.value, ConsolePanel.projectTab);
      // Said in the status bar, where it stays until dismissed.
      expect(
        find.descendant(of: find.byKey(const Key('notice')), matching: find.textContaining('`fynkel run` refuses')),
        findsOneWidget,
      );
      await tester.tap(find.byKey(const Key('notice.dismiss')));
      await tester.pump();
      expect(find.byKey(const Key('notice')), findsNothing);
    });

    testWidgets('Play runs the project, its records reach the Game tab, and a row opens the entity', (tester) async {
      final dir = Directory.systemTemp.createTempSync('fynkel_editor_play');
      addTearDown(() => dir.deleteSync(recursive: true));
      Directory('${dir.path}/assets/scenes').createSync(recursive: true);
      final scenePath = '${dir.path}/assets/scenes/playground.scene.json';
      final project = FakeProject(dir: dir.path)..addDocument(playScene(path: scenePath));
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          store: store,
          projects: {dir.path: project},
          initialProject: dir.path,
          startPlayProcess: launcher.call,
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      expect(find.byKey(const Key('window.playState')), findsNothing); // nothing has run

      // Show Gizmos in Play (item 9h): the run's environment asks the game
      // view for them; the choice is remembered in editor.json.
      expect(shell.registry.byId('play.gizmos')!.label, 'Show Gizmos in Play');
      shell.registry.execute('play.gizmos');
      expect(shell.registry.byId('play.gizmos')!.label, 'Hide Gizmos in Play');
      expect(File('${dir.path}/.fynkel/editor.json').readAsStringSync(), contains('"playGizmos": true'));

      shell.registry.execute('play.start');
      await pumpLines(tester);
      expect(launcher.calls.single.$2, ['run', '--project', dir.path, '-d', defaultPlayDevice()]);
      expect(launcher.calls.single.$4!['FYNKEL_EDITOR_CHANNEL'], 'stdio');
      expect(launcher.calls.single.$4!['FYNKEL_GIZMOS'], '1');
      expect(shell.consoleTab.value, ConsolePanel.gameTab);
      // The plain editor's Play builds and opens its own window: the pill says so.
      expect(find.text('Building… (opens in its own window)'), findsOneWidget);
      expect(shell.registry.byId('play.start')!.label, 'Stop Playing', reason: '⌘P toggles, as in Unity');
      expect(shell.registry.byId('play.start')!.enabled!(), isTrue);
      expect(shell.registry.byId('play.stop')!.enabled!(), isTrue);

      launcher.last.emit(record({'fynkel': 'state', 'seq': 1, 'state': 'started', 'pid': 7100}));
      launcher.last.emit(
        record({'fynkel': 'state', 'seq': 2, 'state': 'loaded', 'scene': 'Playground', 'entities': 3}),
      );
      launcher.last.emit(
        record({
          'fynkel': 'diagnostic',
          'seq': 3,
          'tick': 84,
          'code': 'staleEntity',
          'severity': 'error',
          'message': 'the entity is gone',
          'source': 'step',
          // As a game names it: from the **assets** root, the way the
          // cooked project is laid out — not the editor's project path.
          'document': 'scenes/playground.scene.json',
          'pointer': '/entities/player',
          'id': 'player',
        }),
      );
      await pumpLines(tester);
      expect(find.text('Playing · Playground · own window'), findsOneWidget);
      expect(find.text('the entity is gone'), findsOneWidget);
      expect(find.textContaining('the game process started (pid 7100)'), findsOneWidget);
      // The status line names the GAME's pid (7100), not the CLI's (7000):
      // it is the number a `kill -9` from a terminal needs.
      expect((tester.widget(find.byKey(const Key('console.game.status'))) as Text).data, contains('game pid 7100'));

      // The search narrows the tab to the diagnostic; double-clicking it
      // opens the scene with the entity selected.
      await tester.enterText(find.byKey(const Key('console.game.search')), 'staleEntity');
      await pumpLines(tester);
      final entry = shell.gameConsole!.entries.single;
      expect(entry.record.code, 'staleEntity');
      final row = find.byKey(Key('console.game.row.${shell.gameConsole!.keyOf(entry)}'));
      expect(row, findsOneWidget);
      await tester.tap(row);
      await tester.pump(const Duration(milliseconds: 40));
      expect(find.byKey(const Key('console.game.detail')), findsOneWidget);
      await tester.tap(row);
      await tester.pump();
      await tester.pump();
      expect(shell.tabs!.open.single.path, scenePath);
      expect(shell.hierarchy!.selection, ['player']);
      shell.gameConsole!.setQuery('');
      await pumpLines(tester);

      // An edit while the game is up: the banner says when it applies.
      expect(find.byKey(const Key('window.playEdited')), findsNothing);
      shell.tabs!.open.single.transaction([Command.renameObject('player', 'Hero')]);
      await tester.pumpAndSettle();
      expect(shell.play!.editedWhilePlaying, isTrue);
      expect(find.byKey(const Key('window.playEdited')), findsOneWidget);
      expect(find.textContaining('Changes apply on the next Play'), findsOneWidget);

      // Stop: the process tree is signalled and the toolbar goes back to ▶.
      shell.registry.execute('play.stop');
      await pumpLines(tester);
      expect(shell.play!.state, PlayState.stopping);
      launcher.last.exitWith(143);
      await tester.pumpAndSettle();
      expect(shell.play!.state, PlayState.exited);
      expect(shell.play!.crashed, isFalse);
      expect(find.text('Exited'), findsNothing);
      expect(find.text('Stopped'), findsWidgets); // the toolbar and the Console's status line
      // The game is gone, so the banner about it goes too.
      expect(shell.play!.editedWhilePlaying, isTrue);
      expect(find.byKey(const Key('window.playEdited')), findsNothing);
      expect(shell.registry.byId('play.start')!.enabled!(), isTrue);
      expect(shell.registry.byId('play.start')!.label, 'Play');

      // Clear empties the Game tab.
      expect(shell.registry.byId('play.clearConsole')!.enabled!(), isTrue);
      shell.registry.execute('play.clearConsole');
      await tester.pumpAndSettle();
      expect(shell.play!.records, isEmpty);
      expect(find.byKey(const Key('console.game.empty')), findsOneWidget);
    });

    testWidgets('Play again in the banner stops the run and starts a fresh one', (tester) async {
      final dir = Directory.systemTemp.createTempSync('fynkel_editor_replay');
      addTearDown(() => dir.deleteSync(recursive: true));
      Directory('${dir.path}/assets/scenes').createSync(recursive: true);
      final scenePath = '${dir.path}/assets/scenes/playground.scene.json';
      final project = FakeProject(dir: dir.path)..addDocument(playScene(path: scenePath));
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          store: store,
          projects: {dir.path: project},
          initialProject: dir.path,
          startPlayProcess: launcher.call,
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      shell.registry.execute('play.start');
      await pumpLines(tester);
      launcher.last.emit(record({'fynkel': 'state', 'seq': 1, 'state': 'started', 'pid': 7100}));
      await pumpLines(tester);
      expect(launcher.calls.length, 1);

      // An edit while the game is up raises the banner.
      final tab = shell.openDocument(scenePath)!;
      await pumpLines(tester);
      expect(find.byKey(const Key('window.playEdited')), findsNothing);
      tab.transaction([Command.renameObject('player', 'Hero')]);
      await pumpLines(tester);
      expect(find.byKey(const Key('window.playEdited')), findsOneWidget);

      // Play again stops the run that is up before it starts anything:
      // `play.start` alone refuses while a game is running, which is why
      // the banner's button has its own path.
      final first = launcher.last;
      await tester.tap(find.byKey(const Key('window.playAgain')));
      await pumpLines(tester);
      expect(shell.play!.state, PlayState.stopping);
      expect(launcher.calls.length, 1);
      first.exitWith(143);
      await pumpUntil(tester, find.byKey(const Key('dialog.playSave')));

      // The document is dirty, so the save prompt comes first — then the
      // new run, which forgets the edit.
      expect(find.byKey(const Key('dialog.playSave')), findsOneWidget);
      await tester.tap(find.byKey(const Key('playSave.save')));
      await pumpLines(tester);
      expect(launcher.calls.length, 2);
      expect(launcher.last, isNot(same(first)));
      expect(shell.play!.state, PlayState.starting);
      expect(shell.play!.editedWhilePlaying, isFalse);
      expect(find.byKey(const Key('window.playEdited')), findsNothing);
      expect(File(scenePath).readAsStringSync(), contains('"Hero"'));

      launcher.last.exitWith(0);
      await pumpLines(tester);
      expect(shell.play!.state, PlayState.exited);
      // Stop's grace (`Future.any`) leaves its timer behind when the tree
      // goes down politely; the test clock has to run past it.
      await tester.pump(const Duration(seconds: 4));
    });

    testWidgets('a dirty document gets the save prompt, and Cancel does not play', (tester) async {
      final dir = Directory.systemTemp.createTempSync('fynkel_editor_play_dirty');
      addTearDown(() => dir.deleteSync(recursive: true));
      Directory('${dir.path}/assets/scenes').createSync(recursive: true);
      final scenePath = '${dir.path}/assets/scenes/playground.scene.json';
      final project = FakeProject(dir: dir.path)..addDocument(playScene(path: scenePath));
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          store: store,
          projects: {dir.path: project},
          initialProject: dir.path,
          startPlayProcess: launcher.call,
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      final tab = shell.openDocument(scenePath)!;
      await tester.pumpAndSettle();
      tab.transaction([Command.renameObject('cam', 'Eye')]);
      await tester.pumpAndSettle();
      expect(tab.dirty, isTrue);

      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('dialog.playSave')), findsOneWidget);
      await tester.tap(find.byKey(const Key('playSave.cancel')));
      await tester.pumpAndSettle();
      expect(launcher.calls, isEmpty);
      expect(shell.play!.state, PlayState.idle);

      // Save and Play writes the file first, then runs.
      shell.registry.execute('play.start');
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('playSave.save')));
      await pumpLines(tester);
      expect(tab.dirty, isFalse);
      expect(File(scenePath).readAsStringSync(), contains('"Eye"'));
      expect(launcher.calls.length, 1);
      expect(shell.play!.editedWhilePlaying, isFalse);

      // End the run inside the test: a session disposed while a process is
      // up signals a real tree, and no test may reach the machine's.
      launcher.last.exitWith(0);
      await pumpLines(tester);
      expect(shell.play!.state, PlayState.exited);
    });
  });
}
