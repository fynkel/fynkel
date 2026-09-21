// Document tabs over the in-memory fake: open/focus/close, dirty and save,
// undo/redo per tab, revert, the file-watcher paths (silent reload with no
// local edits; a conflict — never an overwrite — with local edits), and the
// shell's dialogs (save prompt on close, Open Document, conflict).
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/console/console_panel.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

FakeDocument smallScene({
  String id = 'scene-a',
  String path = '/fake/game/assets/scenes/a.scene.json',
  bool onDisk = false,
}) => FakeDocument(
  path: path,
  id: id,
  name: 'Scene ${id.substring(id.length - 1).toUpperCase()}',
  relativePath: 'scenes/${id.substring(id.length - 1)}.scene.json',
  onDisk: onDisk,
  entities: {
    'cam': FakeDocument.entity('Camera', position: [0, 2, 6]),
    'robot': FakeDocument.entity('Robot', children: ['arm']),
    'arm': FakeDocument.entity('Arm', parent: 'robot', position: [1, 0, 0]),
    'floor': FakeDocument.entity('Floor'),
  },
);

void main() {
  late FakeProject project;

  setUp(() {
    project = FakeProject(dir: '/fake/game');
  });

  group('DocumentTabs', () {
    test('opening is idempotent per document and focuses the tab', () {
      final a = project.addDocument(smallScene());
      final b = project.addDocument(smallScene(id: 'scene-b', path: '/fake/game/assets/scenes/b.scene.json'));
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final ta = tabs.openDocument('scene-a')!;
      expect(tabs.focused, ta);
      expect(tabs.open, [ta]);
      final tb = tabs.openDocument(b.path)!;
      expect(tabs.open.length, 2);
      expect(tabs.focused, tb);
      expect(tabs.openDocument(a.relativePath!), same(ta)); // by relative path too
      expect(tabs.open.length, 2);
      expect(tabs.focused, ta);
      tabs.focusNext();
      expect(tabs.focused, tb);
      tabs.focusNext();
      expect(tabs.focused, ta); // wraps
      tabs.focusPrevious();
      expect(tabs.focused, tb);
      tabs.closeDiscarding(tb);
      expect(tabs.open, [ta]);
      expect(tabs.focused, ta);
      expect(b.isClosed, isTrue);
    });

    test('a refused open reports the diagnostics and opens nothing', () {
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      expect(tabs.openDocument('nope'), isNull);
      expect(tabs.openError, contains('FYN-D0900'));
      expect(tabs.isEmpty, isTrue);
    });
  });

  group('OpenDocument', () {
    test('a transaction dirties the tab and patches the tree from affectedIds; save cleans it', () {
      final doc = project.addDocument(smallScene());
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      final tree = tab.tree;
      expect(tree.builds, 1);
      expect(tree.roots, ['cam', 'robot', 'floor']);
      final robot = tree['robot']!;
      final a = tab.transaction([Command.renameObject('robot', 'Bot')])!;
      expect(a.ok, isTrue);
      expect(tab.dirty, isTrue);
      expect(tree.builds, 1); // patched, not rebuilt
      expect(tree.patches, greaterThanOrEqualTo(1));
      expect(identical(tree['robot'], robot), isTrue); // same node object
      expect(tree['robot']!.name, 'Bot');
      expect(tree['robot']!.version, 2);
      expect(tree['cam']!.version, 1); // untouched
      expect(tab.document.history, 1);
      expect(tab.save(), isTrue);
      expect(tab.dirty, isFalse);
      expect(doc.calls, ['apply', 'save']);
    });

    test('undo and redo replay per tab and patch the tree', () {
      project.addDocument(smallScene());
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      tab.transaction([Command.renameObject('cam', 'Eye')]);
      tab.transaction([Command.reparentObject('floor', parent: 'robot', index: 0)]);
      expect(tab.tree['robot']!.children, ['floor', 'arm']);
      expect(tab.undo(), isTrue);
      expect(tab.tree['robot']!.children, ['arm']);
      expect(tab.tree.roots, ['cam', 'robot', 'floor']);
      expect(tab.tree['cam']!.name, 'Eye');
      expect(tab.undo(), isTrue);
      expect(tab.tree['cam']!.name, 'Camera');
      expect(tab.undo(), isFalse); // nothing left: no error shown
      expect(tab.lastError, isNull);
      expect(tab.redo(), isTrue);
      expect(tab.tree['cam']!.name, 'Eye');
      expect(tab.tree.builds, 1);
    });

    test('a stale base revision is refused as FYN-E0001 and nothing is touched', () {
      final doc = project.addDocument(smallScene());
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      final stale = doc.revision;
      doc.apply([Command.renameObject('cam', 'Other')]); // someone else, same handle
      final a = doc.apply([Command.renameObject('cam', 'Mine')], baseRevision: stale);
      expect(a.isConflict, isTrue);
      expect(doc.entitiesView['cam']!['name'], 'Other');
      // The tab always names the current revision, so its own transactions land.
      expect(tab.transaction([Command.renameObject('cam', 'Mine')])!.ok, isTrue);
    });

    test('revert drops the history and rebuilds the tree', () {
      project.addDocument(smallScene());
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      tab.transaction([Command.deleteObject('robot')]);
      expect(tab.tree['arm'], isNull);
      expect(tab.reload(), isTrue);
      expect(tab.dirty, isFalse);
      expect(tab.document.canUndo, isFalse);
      expect(tab.tree['arm'], isNotNull);
      expect(tab.tree.builds, 2);
    });

    test('selection: click, ⌘-click, ⇧-range over visible rows, pruned on delete', () {
      project.addDocument(smallScene());
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      tab.tree.expand('robot');
      tab.select('cam');
      tab.select('floor', toggle: true);
      expect(tab.selection.toList(), ['cam', 'floor']);
      tab.select('arm', range: true); // anchor is floor (the last plain/toggle click)
      expect(tab.selection.toList(), ['arm', 'floor']);
      tab.select('cam');
      tab.select('floor', range: true);
      expect(tab.selection.toList(), ['cam', 'robot', 'arm', 'floor']);
      tab.transaction([Command.deleteObject('robot')]);
      expect(tab.selection.toList(), ['cam', 'floor']);
    });

    test('entity error badges come from the document validation and the project diagnostics', () {
      final doc = project.addDocument(smallScene());
      doc.validation = [diag('FYN-D0206', pointer: '/entities/robot/components/x', document: doc.path)];
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      expect(tab.tree.hasError('robot'), isTrue);
      expect(tab.tree.hasError('cam'), isFalse);
      expect(tab.errorCount, 1);
      project.diagnostics = [
        diag('FYN-D0301', document: doc.path),
        Diagnostic(code: 'FYN-D0400', severity: Severity.error, message: 'bad', document: doc.path, id: 'cam'),
      ];
      expect(tab.tree.hasError('cam'), isTrue);
      expect(tab.errorCount, 3);
    });
  });

  group('file watcher', () {
    late Directory dir;

    setUp(() {
      dir = Directory.systemTemp.createTempSync('fynkel_editor_docs');
    });

    tearDown(() => dir.deleteSync(recursive: true));

    test('an external change with no local edits reloads silently', () {
      final path = '${dir.path}/a.scene.json';
      final doc = project.addDocument(smallScene(path: path, onDisk: true));
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      expect(tab.tree['cam']!.name, 'Camera');
      // Someone else (the CLI) writes the file.
      final j = jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>;
      ((j['entities'] as Map)['cam'] as Map)['name'] = 'From CLI';
      File(path).writeAsStringSync(jsonEncode(j));
      expect(tab.checkDisk(), isTrue);
      expect(tab.hasConflict, isFalse);
      expect(tab.tree['cam']!.name, 'From CLI');
      expect(tab.dirty, isFalse);
      expect(doc.calls, ['reload']);
      // The editor's own save is not a change.
      tab.transaction([Command.renameObject('floor', 'Ground')]);
      expect(tab.save(), isTrue);
      expect(tab.checkDisk(), isFalse);
    });

    test('an external change with local edits is a conflict: save refused, the file untouched', () async {
      final path = '${dir.path}/a.scene.json';
      final doc = project.addDocument(smallScene(path: path, onDisk: true));
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      final base = tab.document.revision;
      tab.transaction([Command.renameObject('cam', 'Mine')]);
      final ours = tab.document.revision;
      final j = jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>;
      ((j['entities'] as Map)['cam'] as Map)['name'] = 'Theirs';
      final theirs = jsonEncode(j);
      File(path).writeAsStringSync(theirs);

      var notified = 0;
      tab.addListener(() => notified++);
      project.filesChanged([path, '${dir.path}/other.json']); // through the stream, like the watcher
      await Future<void>.delayed(Duration.zero);
      expect(notified, greaterThan(0));
      expect(tab.hasConflict, isTrue);
      final c = tab.conflict!;
      expect(c.baseRevision, base);
      expect(c.ourRevision, ours);
      expect(c.diskRevision, isNotNull);
      expect(c.history, 1);
      expect(c.diskText, theirs);
      expect(c.baselineText, isNot(theirs));
      expect(tab.dirty, isTrue);
      expect(tab.tree['cam']!.name, 'Mine'); // the tab keeps its edits

      expect(tab.save(), isFalse); // never an overwrite
      expect(tab.lastError, contains('changed on disk'));
      expect(File(path).readAsStringSync(), theirs);
      expect(doc.calls, ['apply']); // no save, no reload happened

      // Save As leaves the other writer's file alone and clears the conflict.
      final copy = '${dir.path}/a copy.scene.json';
      expect(tab.saveAs(copy), isTrue);
      expect(tab.hasConflict, isFalse);
      expect(File(path).readAsStringSync(), theirs);
      expect((jsonDecode(File(copy).readAsStringSync()) as Map)['entities']['cam']['name'], 'Mine');
    });

    test('reload from disk resolves a conflict by dropping the local edits', () {
      final path = '${dir.path}/a.scene.json';
      project.addDocument(smallScene(path: path, onDisk: true));
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      tab.transaction([Command.renameObject('cam', 'Mine')]);
      final j = jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>;
      ((j['entities'] as Map)['cam'] as Map)['name'] = 'Theirs';
      File(path).writeAsStringSync(const JsonEncoder.withIndent('  ').convert(j));
      expect(tab.checkDisk(), isTrue);
      expect(tab.hasConflict, isTrue);
      expect(tab.reload(), isTrue);
      expect(tab.hasConflict, isFalse);
      expect(tab.dirty, isFalse);
      expect(tab.tree['cam']!.name, 'Theirs');
      expect(tab.checkDisk(), isFalse); // in step with the disk again
    });

    test('a file deleted under a dirty tab is a conflict too; save is refused', () {
      final path = '${dir.path}/a.scene.json';
      project.addDocument(smallScene(path: path, onDisk: true));
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final tab = tabs.openDocument('scene-a')!;
      tab.transaction([Command.renameObject('cam', 'Mine')]);
      File(path).deleteSync();
      expect(tab.checkDisk(), isTrue);
      expect(tab.conflict!.fileMissing, isTrue);
      expect(tab.save(), isFalse);
      expect(File(path).existsSync(), isFalse);
    });
  });

  group('shell', () {
    late Preferences prefs;
    late MemoryLayoutStore store;

    setUp(() {
      prefs = Preferences.inMemory();
      store = MemoryLayoutStore();
      project.addDocument(smallScene());
      project.addDocument(smallScene(id: 'scene-b', path: '/fake/game/assets/scenes/b.scene.json'));
    });

    Future<EditorShellState> openShell(WidgetTester tester) async {
      await pumpLarge(
        tester,
        shellApp(prefs: prefs, store: store, projects: {'/fake/game': project}, initialProject: '/fake/game'),
      );
      return tester.state<EditorShellState>(find.byType(EditorShell));
    }

    testWidgets('Open Document lists the scenes; a tab appears with the dirty dot after an edit and ⌘S saves', (
      tester,
    ) async {
      final shell = await openShell(tester);
      expect(find.byKey(const Key('tabs.empty')), findsOneWidget);
      await tester.tap(find.byKey(const Key('tabs.open')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('dialog.openDocument')), findsOneWidget);
      await tester.enterText(find.byKey(const Key('openDocument.search')), 'scene b');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('openDocument.scene-a')), findsNothing);
      await tester.tap(find.byKey(const Key('openDocument.scene-b')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.scene-b')), findsOneWidget);
      expect(shell.tabs!.focused!.document.id, 'scene-b');
      expect(find.byKey(const Key('tab.dirty.scene-b')), findsNothing);
      expect(shell.registry.byId('document.save')!.isEnabled, isFalse);

      shell.tabs!.focused!.transaction([Command.renameObject('cam', 'Eye')]);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.dirty.scene-b')), findsOneWidget);
      expect(shell.registry.byId('document.save')!.isEnabled, isTrue);
      expect(shell.registry.byId('document.undo')!.label, 'Undo (1)');

      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyS);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      await tester.pumpAndSettle();
      expect(shell.registry.executed, contains('document.save'));
      expect(find.byKey(const Key('tab.dirty.scene-b')), findsNothing);
      expect(shell.tabs!.focused!.dirty, isFalse);
    });

    testWidgets('closing a dirty tab asks: Cancel keeps it, Don\'t Save closes it, Save saves first', (tester) async {
      final shell = await openShell(tester);
      final tab = shell.openDocument('scene-a')!;
      await tester.pumpAndSettle();
      tab.transaction([Command.renameObject('cam', 'Eye')]);
      await tester.pumpAndSettle();

      // ⌘W → prompt → Cancel.
      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyW);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('dialog.closePrompt')), findsOneWidget);
      await tester.tap(find.byKey(const Key('closePrompt.cancel')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.scene-a')), findsOneWidget);
      expect(tab.dirty, isTrue);

      // The × → Save.
      await tester.tap(find.byKey(const Key('tab.dirty.scene-a')), warnIfMissed: false);
      // The dirty dot turns into × under the pointer; the close command is the reliable route.
      shell.registry.execute('document.close');
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('closePrompt.save')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.scene-a')), findsNothing);
      expect((project.fakeDocuments.first).calls, contains('save'));
      expect(shell.tabs!.isEmpty, isTrue);

      // Don't Save.
      final again = shell.openDocument('scene-b')!;
      await tester.pumpAndSettle();
      again.transaction([Command.renameObject('cam', 'Eye')]);
      await tester.pumpAndSettle();
      shell.registry.execute('document.close');
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('closePrompt.discard')));
      await tester.pumpAndSettle();
      expect(shell.tabs!.isEmpty, isTrue);
      expect(project.fakeDocuments[1].calls, isNot(contains('save')));
    });

    testWidgets('⌘Z inside a text field is the field\'s undo, not the document\'s', (tester) async {
      final shell = await openShell(tester);
      final tab = shell.openDocument('scene-a')!;
      await tester.pumpAndSettle();
      tab.transaction([Command.renameObject('cam', 'Eye')]);
      await tester.pumpAndSettle();
      expect(tab.document.canUndo, isTrue);
      // Type into the Hierarchy search, then ⌘Z: the text comes back, the rename stays.
      await tester.tap(find.byKey(const Key('hierarchy.search')));
      await tester.enterText(find.byKey(const Key('hierarchy.search')), 'robot');
      await tester.pumpAndSettle();
      await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.keyZ);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
      await tester.pumpAndSettle();
      expect(tab.tree['cam']!.name, 'Eye');
      expect(shell.registry.executed, isNot(contains('document.undo')));
      // Unfocus the field: ⌘Z is the document's undo.
      FocusManager.instance.primaryFocus?.unfocus();
      await tester.pumpAndSettle();
      shell.registry.execute('document.undo');
      await tester.pumpAndSettle();
      expect(tab.tree['cam']!.name, 'Camera');
    });

    testWidgets('Save As offers a project-relative path and creates a new folder under the project', (tester) async {
      // The gate found this: the field said "Path in <project>" but was filled with a path
      // relative to assets/, so the copy went to <project>/scenes/… which does not exist.
      final dir = Directory.systemTemp.createTempSync('fynkel_editor_saveas');
      addTearDown(() => dir.deleteSync(recursive: true));
      Directory('${dir.path}/assets/scenes').createSync(recursive: true);
      final own = FakeProject(dir: dir.path);
      own.addDocument(smallScene(id: 'scene-t', path: '${dir.path}/assets/scenes/t.scene.json'));
      await pumpLarge(
        tester,
        shellApp(prefs: prefs, store: store, projects: {dir.path: own}, initialProject: dir.path),
      );
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      final tab = shell.openDocument('scene-t')!;
      await tester.pumpAndSettle();
      shell.registry.execute('document.saveAs');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('dialog.saveAs')), findsOneWidget);
      final field = tester.widget<TextField>(find.byKey(const Key('saveAs.path')));
      expect(field.controller!.text, 'assets/scenes/t copy.scene.json'); // relative to the project, like the label says
      await tester.enterText(find.byKey(const Key('saveAs.path')), 'assets/scenes/new/mine.scene.json');
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('saveAs.confirm')));
      await tester.pumpAndSettle();
      expect(File('${dir.path}/assets/scenes/new/mine.scene.json').existsSync(), isTrue);
      expect(tab.path, '${dir.path}/assets/scenes/new/mine.scene.json');
      expect(find.byKey(const Key('dialog.saveAs')), findsNothing);
    });

    testWidgets('a conflict shows the badge; the dialog offers reload, save as, keep editing', (tester) async {
      final dir = Directory.systemTemp.createTempSync('fynkel_editor_conflict');
      addTearDown(() => dir.deleteSync(recursive: true));
      final path = '${dir.path}/c.scene.json';
      project.addDocument(smallScene(id: 'scene-c', path: path, onDisk: true));
      final shell = await openShell(tester);
      final tab = shell.openDocument('scene-c')!;
      await tester.pumpAndSettle();
      tab.transaction([Command.renameObject('cam', 'Mine')]);
      final j = jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>;
      ((j['entities'] as Map)['cam'] as Map)['name'] = 'Theirs';
      File(path).writeAsStringSync(const JsonEncoder.withIndent('  ').convert(j));
      project.filesChanged([path]);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.conflict.scene-c')), findsOneWidget);

      // ⌘S on a conflicted tab opens the dialog instead of writing.
      shell.registry.execute('document.save');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('dialog.conflict')), findsOneWidget);
      expect(find.textContaining('+1 −1 line'), findsOneWidget);
      expect(find.textContaining('-      "name": "Camera"'), findsOneWidget);
      expect(find.textContaining('+      "name": "Theirs"'), findsOneWidget);
      await tester.tap(find.byKey(const Key('conflict.keep')));
      await tester.pumpAndSettle();
      expect(tab.hasConflict, isTrue);
      expect(tab.dirty, isTrue);

      await tester.tap(find.byKey(const Key('tab.conflict.scene-c')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('conflict.reload')));
      await tester.pumpAndSettle();
      expect(tab.hasConflict, isFalse);
      expect(tab.dirty, isFalse);
      expect(tab.tree['cam']!.name, 'Theirs');
      expect(find.byKey(const Key('tab.conflict.scene-c')), findsNothing);
    });

    testWidgets('a Console row of a scene opens the tab and selects the entity', (tester) async {
      final shell = await openShell(tester);
      final doc = project.fakeDocuments.first;
      project.diagnostics = [diag('FYN-D0206', document: doc.path, pointer: '/entities/arm/components/t/data/x')];
      await tester.pumpAndSettle();
      shell.console!.select(ConsoleEntry.keyOf(shell.console!.entries.first.diagnostic));
      expect(shell.registry.byId('console.openInEditor')!.isEnabled, isTrue);
      shell.registry.execute('console.openInEditor');
      await tester.pumpAndSettle();
      expect(shell.tabs!.focused!.document.id, 'scene-a');
      expect(shell.tabs!.focused!.selection.toList(), ['arm']);
      expect(shell.tabs!.focused!.tree.expanded, contains('robot')); // revealed
      expect(find.byKey(const Key('hierarchy.name.arm')), findsOneWidget);
      expect(find.byKey(const Key('hierarchy.errorBadge.arm')), findsOneWidget);
      expect(find.byKey(const Key('tab.errors.scene-a')), findsOneWidget);
    });
  });
}
