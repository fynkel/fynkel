// ADR-0021 P3c: opening a game project hands it over to its project
// editor. The editor starts `fynkel editor <dir> --handover`, follows the
// project's status file and quits once the other editor is up; a failed
// build says why and the project can still open here, without the game. A
// project that is not a game, and the project editor's own project, open
// here as before.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/play/editor_game.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_handover.dart';
import 'package:path/path.dart' as p;

import 'support/fakes.dart';

void _status(String dir, int seq, String state, {int pid = 4242, String? message}) {
  final file = File(p.join(dir, '.fynkel', 'editor_host', 'status.json'));
  file.parent.createSync(recursive: true);
  file.writeAsStringSync(
    jsonEncode({'version': 1, 'seq': seq, 'state': state, 'pid': pid, 'message': ?message}),
  );
  // A new modification time for every write, whatever the clock's grain.
  file.setLastModifiedSync(DateTime.now().add(Duration(seconds: seq)));
}

void main() {
  late Directory tmp;
  late String dir;

  setUp(() {
    tmp = Directory.systemTemp.createTempSync('fynkel-handover');
    dir = p.join(tmp.path, 'coins');
    Directory(p.join(dir, 'lib')).createSync(recursive: true);
    File(p.join(dir, 'pubspec.yaml')).writeAsStringSync('name: coins\n');
    File(p.join(dir, 'lib', 'fynkel_game.dart')).writeAsStringSync('');
  });
  tearDown(() => tmp.deleteSync(recursive: true));

  test('a game project is a Dart package with lib/fynkel_game.dart', () {
    expect(isGameProject(dir), isTrue);
    File(p.join(dir, 'lib', 'fynkel_game.dart')).deleteSync();
    expect(isGameProject(dir), isFalse);
  });

  test('the handover follows its own process\'s lines of the status file', () async {
    final started = <List<String>>[];
    final h = EditorHandover(
      dir: dir,
      cli: '/x/fynkel',
      root: '/x/root',
      flutter: '/x/flutter',
      interval: const Duration(milliseconds: 10),
      windowFrame: () async => [120, 80.4, 1400, 900],
      start: (exe, args, {required log}) async {
        started.add([exe, ...args, log]);
        return 4242;
      },
    );
    addTearDown(h.dispose);
    _status(dir, 7, 'exited', pid: 1); // the last session's
    await h.begin();
    expect(started.single, [
      '/x/fynkel',
      'editor',
      dir,
      '--handover',
      '--root',
      '/x/root',
      '--flutter',
      '/x/flutter',
      // The project editor opens where this window is.
      '--window-frame',
      '120,80,1400,900',
      p.join(dir, '.fynkel', 'editor_host', 'editor.log'),
    ]);
    await Future<void>.delayed(const Duration(milliseconds: 40));
    expect(h.state, HandoverState.building, reason: 'another process\'s line is not ours');
    _status(dir, 1, 'building', message: 'Building macOS application...');
    await Future<void>.delayed(const Duration(milliseconds: 40));
    expect(h.message, 'Building macOS application...');
    _status(dir, 2, 'running');
    await Future<void>.delayed(const Duration(milliseconds: 40));
    expect(h.state, HandoverState.running);
  });

  group('the shell', () {
    late Preferences prefs;
    setUp(() => prefs = Preferences.inMemory());

    testWidgets('opening a game project builds its editor, then this one quits', (tester) async {
      final started = <List<String>>[];
      var quits = 0;
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          projects: {dir: FakeProject(dir: dir)},
          quit: () => quits++,
          isGame: isGameProject,
          startEditor: (exe, args, {required log}) async {
            started.add(args);
            return 4242;
          },
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      var done = false;
      unawaited(shell.openProjectDir(dir).then((_) => done = true));
      // The window's frame is asked for first (the runner's channel).
      for (var i = 0; i < 10 && started.isEmpty; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }
      await tester.pump();
      expect(started.single.take(3), ['editor', dir, '--handover']);
      expect(find.text('Opening coins…'), findsOneWidget);

      await tester.runAsync(() async => _status(dir, 1, 'building', message: 'Running pod install...'));
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.text('Running pod install...'), findsOneWidget);

      await tester.runAsync(() async => _status(dir, 2, 'running'));
      await tester.pump(const Duration(milliseconds: 400));
      await tester.pump();
      expect(done, isTrue);
      expect(quits, 1);
      expect(shell.project, isNull, reason: 'nothing opened here');
    });

    testWidgets('a build that fails says why; Open Without the Game opens it here', (tester) async {
      final started = <List<String>>[];
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          projects: {dir: FakeProject(dir: dir)},
          isGame: isGameProject,
          startEditor: (exe, args, {required log}) async {
            started.add(args);
            return 4242;
          },
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      var done = false;
      unawaited(shell.openProjectDir(dir).then((_) => done = true));
      // The window's frame is asked for first (the runner's channel).
      for (var i = 0; i < 10 && started.isEmpty; i++) {
        await tester.pump(const Duration(milliseconds: 100));
      }
      await tester.pump();
      await tester.runAsync(() async => _status(dir, 2, 'failed', message: "lib/main.dart:3: Error: Expected ';'"));
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.text('coins did not open in its project editor'), findsOneWidget);
      expect(find.textContaining("Error: Expected ';'"), findsOneWidget);

      await tester.tap(find.byKey(const Key('handover.openHere')));
      await tester.pumpAndSettle();
      expect(done, isTrue);
      expect(shell.project?.dir, dir);
    });

    testWidgets('a project that is not a game, and the editor\'s own project, open here', (tester) async {
      final started = <String>[];
      final other = p.join(tmp.path, 'plain');
      Directory(other).createSync();
      await pumpLarge(
        tester,
        shellApp(
          prefs: prefs,
          projects: {dir: FakeProject(dir: dir), other: FakeProject(dir: other)},
          isGame: isGameProject,
          game: EditorGame(name: 'coins', projectDir: dir, build: (_) => const SizedBox()),
          startEditor: (exe, args, {required log}) async {
            started.add(args[1]);
            return 1;
          },
        ),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      await shell.openProjectDir(other);
      await tester.pumpAndSettle();
      expect(shell.project?.dir, other);
      await shell.openProjectDir(dir);
      await tester.pumpAndSettle();
      expect(shell.project?.dir, dir);
      expect(started, isEmpty);
      expect(find.byKey(const Key('dialog.handover')), findsNothing);
    });
  });
}
