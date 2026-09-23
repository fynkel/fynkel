// ADR-0021 P3b: a code change a hot reload cannot apply ("Reload
// rejected") restarts the editor — at once when nothing would be lost,
// after a Save and Restart when documents are unsaved or the game is
// running. The editor asks `fynkel editor` through `restart.request`
// next to the status file. The open tabs, the one in front and each one's
// selection live in `.fynkel/editor.json` and come back when the project
// opens again.

import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Command;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/scene_view/editor_state.dart';
import 'package:path/path.dart' as p;

import 'support/fakes.dart';

FakeDocument _scene(String dir, String id, String name) => FakeDocument(
  path: p.join(dir, 'assets', 'scenes', '$id.scene.json'),
  id: id,
  name: name,
  relativePath: 'scenes/$id.scene.json',
  entities: {'floor': FakeDocument.entity('Floor'), 'lamp': FakeDocument.entity('Lamp')},
);

void main() {
  late Directory tmp;
  late String dir;
  late String status;
  late Preferences prefs;

  setUp(() {
    tmp = Directory.systemTemp.createTempSync('fynkel-restart');
    dir = tmp.path;
    status = p.join(dir, '.fynkel', 'editor_host', 'status.json');
    Directory(p.dirname(status)).createSync(recursive: true);
    prefs = Preferences.inMemory();
  });
  tearDown(() => tmp.deleteSync(recursive: true));

  File request() => File(p.join(p.dirname(status), 'restart.request'));

  void say(int seq, String state, {String? message}) {
    File(status).writeAsStringSync(jsonEncode({'version': 1, 'seq': seq, 'state': state, 'message': ?message}));
    File(status).setLastModifiedSync(DateTime.now().add(Duration(seconds: seq)));
  }

  FakeProject project() => FakeProject(dir: dir)
    ..addDocument(_scene(dir, 'main', 'Main'))
    ..addDocument(_scene(dir, 'level', 'Level'));

  Future<EditorShellState> open(WidgetTester tester, FakeProject pr) async {
    await pumpLarge(
      tester,
      shellApp(prefs: prefs, projects: {dir: pr}, initialProject: dir, codeReloadStatus: status),
    );
    await tester.pumpAndSettle();
    return tester.state<EditorShellState>(find.byType(EditorShell));
  }

  testWidgets('the open tabs, the one in front and the selection come back', (tester) async {
    var shell = await open(tester, project());
    final main = shell.openDocument('main')!;
    main.setSelection(['floor']);
    shell.openDocument('level')!.setSelection(['lamp']);
    shell.openDocument('main');
    await tester.pumpAndSettle();
    expect(shell.tabs!.focused, same(main));
    final es = EditorState.load(dir);
    expect(es.openTabs, ['main', 'level']);
    expect(es.focusedTab, 'main');
    expect(es.selections, {
      'main': ['floor'],
      'level': ['lamp'],
    });

    // A new editor on the project: the same tabs, the same one in front.
    await tester.pumpWidget(const SizedBox());
    shell = await open(tester, project());
    expect(shell.tabs!.open.map((t) => t.document.id), ['main', 'level']);
    expect(shell.tabs!.focused!.document.id, 'main');
    expect(shell.tabs!.focused!.selection, {'floor'});
    expect(shell.tabs!.open.last.selection, {'lamp'});
  });

  testWidgets('a rejected reload with nothing unsaved restarts at once', (tester) async {
    final shell = await open(tester, project());
    shell.openDocument('main');
    await tester.pumpAndSettle();
    await tester.runAsync(() async => say(1, 'reloadRejected', message: 'Reload rejected: const class changed'));
    await tester.pump(const Duration(milliseconds: 500));
    await tester.pump();
    expect(request().existsSync(), isTrue, reason: 'fynkel editor is asked for a hot restart');
    expect(find.textContaining('Restarting to apply'), findsOneWidget);
    expect(EditorState.load(dir).openTabs, ['main'], reason: 'the tabs were remembered first');
  });

  testWidgets('with unsaved documents it asks: Save and Restart saves, then restarts', (tester) async {
    final shell = await open(tester, project());
    shell.openDocument('main')!.transaction([Command.renameObject('floor', 'Ground')]);
    await tester.pumpAndSettle();
    expect(shell.tabs!.focused!.dirty, isTrue);

    await tester.runAsync(() async => say(1, 'reloadRejected', message: 'Reload rejected: const class changed'));
    await tester.pump(const Duration(milliseconds: 500));
    await tester.pump();
    expect(request().existsSync(), isFalse, reason: 'nothing restarts while work is unsaved');
    expect(find.text('Save and Restart'), findsOneWidget);

    await tester.tap(find.byKey(const Key('notice.action')));
    await tester.pumpAndSettle();
    expect(shell.tabs!.focused!.dirty, isFalse, reason: 'saved first');
    expect(request().existsSync(), isTrue);
  });

  testWidgets('Restart to Apply Code Changes is a command, off without fynkel editor', (tester) async {
    final shell = await open(tester, project());
    expect(shell.registry.byId('play.restart')!.isEnabled, isTrue);
    shell.registry.execute('play.restart');
    await tester.pumpAndSettle();
    expect(request().existsSync(), isTrue);

    await tester.pumpWidget(const SizedBox());
    await pumpLarge(tester, shellApp(prefs: prefs, projects: {dir: project()}, initialProject: dir));
    await tester.pumpAndSettle();
    final plain = tester.state<EditorShellState>(find.byType(EditorShell));
    expect(plain.registry.byId('play.restart')!.isEnabled, isFalse);
  });
}
