// Debug.Log (docs/unity-parity.md § Tier 2): `Debug.log` / `logWarning` /
// `logError` / `logException` reach the Console at their level with the
// caller's stack (the row's line, a double-click to the IDE), and a
// `print` in game code becomes a Console row too — Play in the editor
// runs the game in the editor's process, whose stdout no Console reads.

import 'dart:convert';

import 'package:fynkel/fynkel.dart';
import 'package:test/test.dart';

class Talker extends Behaviour {
  @override
  void update(UpdateContext context) {
    Debug.log('hello');
    Debug.logWarning('careful');
    Debug.logError('broken');
    print('printed');
    try {
      throw StateError('caught');
    } catch (e, st) {
      Debug.logException(e, st);
    }
    destroy();
  }
}

void main() {
  late NativeLibrary lib;
  setUpAll(() => lib = NativeLibrary.open());
  tearDownAll(() => lib.dispose());

  test('under an editor: each level, the calling line, and print', () {
    final lines = <String>[];
    EditorChannel.debugUse(enabled: true, sink: lines.add);
    addTearDown(EditorChannel.debugUse);
    final w = World.create(library: lib)..throwOnDiagnostics = true;
    addTearDown(w.dispose);
    w.spawn(name: 'Talker').addBehaviour(Talker());
    w.frame(1 / 60);
    w.frame(1 / 60);

    final logs = [
      for (final l in lines) jsonDecode(l) as Map<String, Object?>,
    ].where((r) => r['fynkel'] == 'log').toList();
    String? severityOf(String message) => logs.firstWhere((r) => r['message'] == message)['severity'] as String?;
    expect(severityOf('hello'), 'info');
    expect(severityOf('careful'), 'warning');
    expect(severityOf('broken'), 'error');
    final printed = logs.firstWhere((r) => r['message'] == 'printed');
    expect(printed['source'], 'print');
    final caught = logs.firstWhere((r) => r['message'] == 'Bad state: caught');
    expect((caught['source'], caught['severity']), ('exception', 'error'));
    for (final r in [...logs.where((r) => r['source'] == 'game'), printed, caught]) {
      expect(r['stack'], contains('debug_log_test.dart'), reason: '${r['message']}: the caller is in the stack');
      expect(r['tick'], isNotNull);
    }
    // The warning's first frame of this file is Talker.update's line.
    final stack = logs.firstWhere((r) => r['message'] == 'careful')['stack'] as String;
    final own = stack.split('\n').firstWhere((l) => l.contains('debug_log_test.dart'));
    expect(own, contains('Talker.update'));
  });

  test('without an editor: the context\'s diagnostics, prefixed by level; print is left alone', () {
    final context = TestGameContext();
    context.run(() {
      Debug.log('plain');
      Debug.logWarning('careful');
      Debug.logError('broken');
    });
    expect(context.diagnostics.lines, [
      '[game] plain',
      '[game] warning: careful',
      '[game] error: broken',
    ]);
    // Outside any context it still works (Unity's Debug.Log works anywhere).
    Debug.log('outside');
  });
}
