// Unity parity: Debug.Log in the Console (docs/unity-parity.md § Tier 2).
// The SDK's own `Debug.logWarning` / `Debug.logError` and a `print` in game
// code, sent over the editor channel as Play in the editor sends them,
// arrive in the Game tab at their level, located at the calling line,
// and ↩ opens that line in the IDE.
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Severity;
import 'package:fynkel_editor/src/play/game_console.dart';
import 'package:fynkel_editor/src/play/play_session.dart';

/// Game code, as far as the Console is concerned: this file.
void talk() {
  Debug.logWarning('careful'); // the warning's line
  Debug.logError('broken');
  TestGameContext().run(() => print('printed')); // ignore: avoid_print
}

void main() {
  test('Debug.logWarning / logError and print: their level, the calling line, ↩ opens it', () async {
    final dir = Directory.current.path; // apps/editor: this file is the game's own code
    final session = PlaySession(projectDir: dir, cli: () => null);
    final opened = <String>[];
    final model = GameConsoleModel(
      session: session,
      relative: (path) => path.startsWith('$dir/') ? path.substring(dir.length + 1) : path,
      projectDir: dir,
      openInIde: (file, {line, column}) async {
        opened.add('$file:$line');
        return null;
      },
    );
    addTearDown(() {
      model.dispose();
      session.dispose();
    });
    session.beginInEditor();
    EditorChannel.debugUse(enabled: true, sink: session.receive);
    addTearDown(EditorChannel.debugUse);

    talk();

    final source = File('$dir/test/parity/debug_log_parity_test.dart').readAsLinesSync();
    final warningLine = source.indexWhere((l) => l.contains("the warning's line")) + 1;
    final byMessage = {for (final e in model.entries) e.record.message: e.record};
    final warning = byMessage['careful']!, error = byMessage['broken']!, printed = byMessage['printed']!;
    expect((warning.severity, error.severity, printed.severity), (Severity.warning, Severity.error, Severity.info));
    expect(printed.source, 'print', reason: 'print is a Console row, not lost in the editor\'s stdout');
    expect(model.location(warning), 'test/parity/debug_log_parity_test.dart:$warningLine');
    expect(model.location(printed), startsWith('test/parity/debug_log_parity_test.dart:'));
    expect(model.canActivate(warning), isTrue);
    model.activate(warning);
    await pumpEventQueue();
    expect(opened, ['$dir/test/parity/debug_log_parity_test.dart:$warningLine']);
  });
}
