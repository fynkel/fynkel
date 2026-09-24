// ADR-0021 P3: `fynkel editor` hot reloads the project editor on save and
// writes each outcome to <host>/status.json; the editor follows the file
// and says it in the status bar — a compile error included.

import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/play/code_reload.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

void main() {
  late Directory dir;
  late String path;
  var seq = 0;

  void write(Map<String, Object?> body) {
    seq++;
    final file = File(path);
    file.writeAsStringSync(jsonEncode({'version': 1, 'seq': seq, ...body}));
    // A distinct modification time for each write, whatever the clock's grain.
    file.setLastModifiedSync(DateTime(2026, 1, 1).add(Duration(seconds: seq)));
  }

  setUp(() {
    dir = Directory.systemTemp.createTempSync('fynkel_code_reload');
    path = '${dir.path}/status.json';
    seq = 0;
  });
  tearDown(() => dir.deleteSync(recursive: true));

  test('reads each new event once; a missing or half-written file is not one', () {
    final status = CodeReloadStatus(path, start: false);
    addTearDown(status.dispose);
    var events = 0;
    status.addListener(() => events++);
    status.poll();
    expect(status.last, isNull, reason: 'no file yet');

    write({'state': 'reloaded', 'ms': 340, 'files': ['lib/game/player.dart']});
    status.poll();
    status.poll();
    expect(events, 1);
    expect(status.last!.state, 'reloaded');
    expect(status.last!.ms, 340);
    expect(status.last!.files, ['lib/game/player.dart']);

    File(path)
      ..writeAsStringSync('{"version": 1, "se')
      ..setLastModifiedSync(DateTime(2027));
    status.poll();
    expect(events, 1, reason: 'a torn read waits for the next poll');

    write({'state': 'reloadFailed', 'message': "lib/game/player.dart:12:5: Error: Expected ';' after this."});
    status.poll();
    expect(events, 2);
    expect(status.last!.failed, isTrue);
  });

  testWidgets('the shell shows a reload and a failed one in the status bar', (tester) async {
    final project = FakeProject(dir: '/fake/game');
    await pumpLarge(
      tester,
      shellApp(
        prefs: Preferences.inMemory(),
        store: MemoryLayoutStore(),
        projects: {'/fake/game': project},
        initialProject: '/fake/game',
        codeReloadStatus: path,
      ),
    );
    write({'state': 'reloaded', 'ms': 351, 'files': ['lib/game/collectible.dart']});
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.text('Code reloaded in 351 ms (lib/game/collectible.dart)'), findsOneWidget);

    write({
      'state': 'reloadFailed',
      'message': "lib/game/player.dart:12:5: Error: Expected ';' after this.",
      'files': ['lib/game/player.dart'],
    });
    await tester.pump(const Duration(milliseconds: 500));
    expect(
      find.text("Code did not reload (lib/game/player.dart): lib/game/player.dart:12:5: Error: Expected ';' after this."),
      findsOneWidget,
    );
    await tester.pumpWidget(const SizedBox());
  });

  test('CompileError.parse: each located error line, the excerpt skipped; else the first line', () {
    final errors = CompileError.parse(
      "lib/game/player.dart:12:5: Error: Expected ';' after this.\n"
      '    speed = 3\n'
      '    ^^^^^\n'
      "lib/game/enemy.dart:40:11: Error: The getter 'hp' isn't defined.",
    );
    expect([for (final e in errors) '${e.file}:${e.line}:${e.column} ${e.message}'], [
      "lib/game/player.dart:12:5 Expected ';' after this.",
      "lib/game/enemy.dart:40:11 The getter 'hp' isn't defined.",
    ]);
    final plain = CompileError.parse('Could not start the compiler.\nsecond line').single;
    expect((plain.file, plain.message), (null, 'Could not start the compiler.'));
    expect(CompileError.parse(null), isEmpty);
    expect(CompileError.parse('  '), isEmpty);
  });
}
