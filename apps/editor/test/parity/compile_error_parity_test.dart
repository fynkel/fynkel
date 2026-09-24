// Unity parity: a script that does not compile (docs/unity-parity.md
// § Tier 2, a failed hot reload). `fynkel editor` reports the failed
// reload; the compiler's errors become Console rows at their file, line
// and column, ↩ opens that line in the IDE, and Play is refused until a
// reload goes through — the editor would otherwise play older code than
// the files.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

void main() {
  testWidgets('compile errors are Console rows that open their line; Play waits for a reload that goes through', (
    tester,
  ) async {
    final tmp = Directory.systemTemp.createTempSync('fynkel-compile-');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final status = '${tmp.path}/status.json';
    var seq = 0;
    Future<void> report(String state, {String? message}) async {
      File(status).writeAsStringSync(
        jsonEncode({
          'version': 1,
          'seq': ++seq,
          'state': state,
          'message': ?message,
          'files': ['lib/game/player.dart'],
        }),
      );
      File(status).setLastModifiedSync(DateTime(2027, 1, 1, 0, 0, seq));
      await tester.pump(const Duration(milliseconds: 500));
      await tester.pumpAndSettle();
    }

    final e = await Editor.open(tester, codeReloadStatus: status);
    await report(
      'reloadFailed',
      message: [
        "lib/game/player.dart:12:5: Error: Expected ';' after this.",
        '    speed = 3',
        '    ^^^^^',
        "lib/game/enemy.dart:40:11: Error: The getter 'hp' isn't defined for the type 'Enemy'.",
      ].join('\n'),
    );

    // Play is refused, and the Console shows why.
    await e.press(LogicalKeyboardKey.keyP, command: true);
    expect(find.textContaining('All compiler errors have to be fixed before you can enter Play mode'), findsOneWidget);
    expect(e.shell.play!.isActive, isFalse);
    final console = e.shell.console!;
    final rows = [
      for (final r in console.entries)
        '${r.diagnostic.code} ${console.locationOf(r.diagnostic)} ${r.diagnostic.message}',
    ];
    expect(rows.take(2), [
      "compile lib/game/player.dart:12:5 Expected ';' after this.",
      "compile lib/game/enemy.dart:40:11 The getter 'hp' isn't defined for the type 'Enemy'.",
    ]);

    // ↩ on the row opens the file at the line in the IDE.
    final opened = File('${tmp.path}/opened.txt');
    final ide = File('${tmp.path}/ide.sh')..writeAsStringSync('#!/bin/sh\necho "\$@" > "${opened.path}"\n');
    await tester.runAsync(() => Process.run('chmod', ['+x', ide.path]));
    e.shell.widget.prefs.setIdeCommand('${ide.path} {file}:{line}:{column}');
    final row = find.textContaining("Expected ';' after this.").last;
    await e.click(row);
    await e.press(LogicalKeyboardKey.enter);
    await tester.runAsync(() async {
      for (var i = 0; i < 50 && !opened.existsSync(); i++) {
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
    });
    expect(opened.readAsStringSync().trim(), '${e.dir.path}/lib/game/player.dart:12:5');

    // The fix reloads: the rows go, and Play is no longer refused for it.
    await report('reloaded');
    expect(console.entries.where((r) => r.diagnostic.code == 'compile'), isEmpty);
    expect(e.shell.codeReload!.errors, isEmpty);
  });
}
