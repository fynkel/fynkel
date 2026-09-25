// The game's editor part (ADR-0028), without an editor: what it registers,
// and what its command says.
import 'package:code_only/editor/editor.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

class _Items implements EditorExtensions {
  final items = <String, void Function(EditorContext)>{};

  @override
  String get package => 'code_only';

  @override
  void menuItem(
    String path, {
    required String id,
    required void Function(EditorContext context) run,
    bool Function(EditorContext context)? enabled,
    SingleActivator? shortcut,
    String? description,
  }) => items['$path|$id'] = run;
}

class _Log implements EditorContext {
  final lines = <String>[];

  @override
  String get package => 'code_only';

  @override
  void log(String message) => lines.add(message);

  @override
  void logWarning(String message) => lines.add('warning: $message');

  @override
  void logError(String message) => lines.add('error: $message');

  // The scene, the selection, edits and assets: not used here.
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test('the editor part lists the coin kinds from the Tools menu', () {
    final editor = _Items();
    registerEditor(editor);
    expect(editor.items.keys, [
      'Tools/Code Only/List Coin Kinds|codeOnly.listCoinKinds',
      'Tools/Code Only/Select Coins|codeOnly.selectCoins',
      'Tools/Code Only/Add Coin|codeOnly.addCoin',
    ]);
    final log = _Log();
    editor.items.values.first(log);
    expect(log.lines, ['A coin is worth 1, the big coin 5 (1.6× the size).']);
  });
}
