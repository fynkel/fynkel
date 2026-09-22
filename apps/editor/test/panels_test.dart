// Item 9e, panels and navigation: a tab's chrome (no maximise buttons, a
// close × that fades in under the pointer, the right-click menu, a double
// click to maximise), panel copies in the workspace (ids, titles, the
// lowest free number, save → restore, Show / Hide over every copy), and in
// the shell: two Inspectors (one locked) with two selections, two Project
// browsers in two folders, a Hierarchy locked to a document, "Show in …"
// never breaking a lock, and a restart that brings the copies and their
// locks back.
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart' show AssetEntry;
import 'package:fynkel_editor/src/shell/editor_selection.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';
import 'package:fynkel_editor/src/theme/icons.dart';

import 'documents_test.dart' show smallScene;
import 'support/fakes.dart';

const _copyable = {PanelRegistry.hierarchy, PanelRegistry.scene, PanelRegistry.inspector, PanelRegistry.project};

PanelRegistry registry() => PanelRegistry([
  for (final (id, home) in [
    (PanelRegistry.hierarchy, PanelHome.left),
    (PanelRegistry.scene, PanelHome.center),
    (PanelRegistry.inspector, PanelHome.right),
    (PanelRegistry.project, PanelHome.bottom),
    (PanelRegistry.console, PanelHome.bottom),
  ])
    PanelSpec(
      id: id,
      title: '${id[0].toUpperCase()}${id.substring(1)}',
      icon: FIcons.cropSquare,
      home: home,
      copies: _copyable.contains(id),
      builder: (_, copy) => Text('body $copy'),
    ),
]);

const swatch = 't1-0000-0000-0000-000000000001';
const tone = 'a1-0000-0000-0000-000000000001';

FakeProject assetProject(String dir) {
  final p = FakeProject(dir: dir);
  p.assetEntries = [
    const AssetEntry(id: swatch, path: 'textures/swatch.png', name: 'swatch', kind: 'source', importer: 'fynkel.image'),
    const AssetEntry(id: tone, path: 'sounds/tone.wav', name: 'tone', kind: 'source', importer: 'fynkel.audio'),
  ];
  return p;
}

EditorShellState shellOf(WidgetTester tester) => tester.state<EditorShellState>(find.byType(EditorShell));

double closeOpacity(WidgetTester tester, String id) => tester
    .widget<AnimatedOpacity>(
      find.descendant(of: find.byKey(Key('tab.close.$id')), matching: find.byType(AnimatedOpacity)),
    )
    .opacity;

/// tabbed_view 1.18.1 paints a tab strip's corner once at the width it
/// had before its "hidden tabs" button appeared, when docking rebuilds a
/// tab group narrower than its tabs (a copy opened beside it): a one-frame,
/// debug-only overflow inside the package. Only that report is dropped;
/// every other error still fails the test. Returns the restore.
VoidCallback ignoreTabStripCornerOverflow() {
  final previous = FlutterError.onError;
  FlutterError.onError = (d) {
    // Reported while the package paints its tab strip.
    final inStrip = StackTrace.current.toString().contains('_TabsAreaLayoutRenderBox.paint');
    if (inStrip && d.exceptionAsString().contains('RenderFlex overflowed')) return;
    previous?.call(d);
  };
  return () => FlutterError.onError = previous;
}

Future<void> rightClick(WidgetTester tester, Finder f) async {
  await tester.tap(f, buttons: kSecondaryMouseButton, kind: PointerDeviceKind.mouse);
  await tester.pumpAndSettle();
}

void main() {
  group('PanelIds and copies in the workspace', () {
    test('ids, numbers and titles', () {
      expect(PanelIds.base('inspector#2'), 'inspector');
      expect(PanelIds.number('inspector'), 1);
      expect(PanelIds.number('inspector#3'), 3);
      for (final bad in ['inspector#1', 'inspector#0', 'inspector#x', 'inspector#02']) {
        expect(PanelIds.number(bad), 0, reason: bad);
      }
      expect(PanelIds.of('project', 1), 'project');
      expect(PanelIds.of('project', 4), 'project#4');
      final r = registry();
      expect(r.byId('inspector#2')?.id, 'inspector');
      expect(r.byId('console#2'), isNull, reason: 'the Console stays single');
      expect(r.byId('inspector#1'), isNull);
      expect(r.titleOf('inspector#2'), 'Inspector 2');
      expect(r.titleOf('inspector'), 'Inspector');
    });

    test('a copy opens beside the one used last, takes the lowest free number, and becomes the active one', () {
      final store = MemoryLayoutStore();
      final ws = Workspace(registry: registry(), store: store);
      expect(ws.openCopy(PanelRegistry.console), isNull);
      expect(ws.openCopy(PanelRegistry.inspector), 'inspector#2');
      expect(ws.copiesOf(PanelRegistry.inspector), ['inspector', 'inspector#2']);
      expect(ws.activeCopy(PanelRegistry.inspector), 'inspector#2');
      expect(ws.layout.hierarchy(nameInfo: true), contains('IInspector,IInspector 2'), reason: 'to its right');
      expect(ws.openCopy(PanelRegistry.inspector), 'inspector#3');
      ws.hide('inspector#2');
      expect(ws.isVisible('inspector#2'), isFalse);
      expect(ws.isShown(PanelRegistry.inspector), isTrue);
      expect(ws.openCopy(PanelRegistry.inspector), 'inspector#2', reason: 'the lowest free number');
      ws.focus('inspector');
      expect(ws.copiesByRecency(PanelRegistry.inspector), ['inspector', 'inspector#2', 'inspector#3']);
      expect(ws.visiblePanels, contains(PanelRegistry.inspector));

      // Save → restore keeps the copies.
      final shape = ws.layout.hierarchy(nameInfo: true);
      final again = Workspace(registry: registry(), store: store);
      expect(again.restoreProblem, isNull);
      expect(again.layout.hierarchy(nameInfo: true), shape);
      expect(again.copiesOf(PanelRegistry.inspector), ['inspector', 'inspector#2', 'inspector#3']);

      // Show / Hide in the Window menu is over every copy.
      again.toggle(PanelRegistry.inspector);
      expect(again.isShown(PanelRegistry.inspector), isFalse);
      again.toggle(PanelRegistry.inspector);
      expect(again.copiesOf(PanelRegistry.inspector), ['inspector']);
    });

    test('with no copy shown, New <Panel> shows the first one', () {
      final ws = Workspace(registry: registry(), store: MemoryLayoutStore());
      ws.hide(PanelRegistry.project);
      expect(ws.openCopy(PanelRegistry.project), 'project');
      expect(ws.copiesOf(PanelRegistry.project), ['project']);
    });

    test('maximise and restore', () {
      final ws = Workspace(registry: registry(), store: MemoryLayoutStore());
      ws.toggleMaximized(PanelRegistry.scene);
      expect(ws.isMaximized(PanelRegistry.scene), isTrue);
      ws.toggleMaximized(PanelRegistry.scene);
      expect(ws.isMaximized(PanelRegistry.scene), isFalse);
      // A copy opened while something is maximised restores the layout first.
      ws.toggleMaximized(PanelRegistry.scene);
      ws.openCopy(PanelRegistry.inspector);
      expect(ws.isMaximized(PanelRegistry.scene), isFalse);
    });
  });

  group('the tab', () {
    Future<EditorShellState> pumpShell(WidgetTester tester) async {
      await pumpLarge(
        tester,
        shellApp(
          prefs: Preferences.inMemory(),
          projects: {'/fake/game': FakeProject(dir: '/fake/game')},
          initialProject: '/fake/game',
        ),
      );
      return shellOf(tester);
    }

    testWidgets('has no maximise button; its close × fades in under the pointer and closes the panel', (tester) async {
      final shell = await pumpShell(tester);
      expect(find.byTooltip('Maximize'), findsNothing);
      expect(closeOpacity(tester, PanelRegistry.inspector), 0);

      final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
      addTearDown(mouse.removePointer);
      await mouse.addPointer(location: Offset.zero);
      await mouse.moveTo(tester.getCenter(find.byKey(const Key('tab.inspector'))));
      await tester.pumpAndSettle();
      expect(closeOpacity(tester, PanelRegistry.inspector), 1);
      expect(closeOpacity(tester, PanelRegistry.hierarchy), 0, reason: 'only the tab under the pointer');

      await mouse.moveTo(tester.getCenter(find.byKey(const Key('tab.close.inspector'))));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('tab.close.inspector')), kind: PointerDeviceKind.mouse);
      await tester.pumpAndSettle();
      expect(shell.workspace!.isShown(PanelRegistry.inspector), isFalse);
      expect(find.byKey(const Key('tab.inspector')), findsNothing);
    });

    testWidgets('a right click opens Maximise / New <Panel> / Close; a double click maximises', (tester) async {
      final shell = await pumpShell(tester);
      final ws = shell.workspace!;

      await rightClick(tester, find.byKey(const Key('tab.scene')));
      expect(find.text('Maximise'), findsOneWidget);
      expect(find.text('New Scene View'), findsOneWidget);
      await tester.tap(find.byKey(const Key('tab.menu.maximize')));
      await tester.pumpAndSettle();
      expect(ws.isMaximized(PanelRegistry.scene), isTrue);
      await rightClick(tester, find.byKey(const Key('tab.scene')));
      expect(find.text('Restore'), findsOneWidget);
      await tester.tap(find.byKey(const Key('tab.menu.maximize')));
      await tester.pumpAndSettle();
      expect(ws.isMaximized(PanelRegistry.scene), isFalse);

      // The Console has no copies: no New item.
      await rightClick(tester, find.byKey(const Key('tab.console'), skipOffstage: false).hitTestable());
      expect(find.byKey(const Key('tab.menu.copy')), findsNothing);
      await tester.tapAt(Offset.zero);
      await tester.pumpAndSettle();

      await rightClick(tester, find.byKey(const Key('tab.project')));
      expect(find.text('New Project Browser'), findsOneWidget);
      await tester.tap(find.byKey(const Key('tab.menu.copy')));
      await tester.pumpAndSettle();
      expect(ws.copiesOf(PanelRegistry.project), ['project', 'project#2']);
      expect(
        find.descendant(of: find.byKey(const Key('tab.project#2')), matching: find.text('Project 2')),
        findsOneWidget,
      );

      await rightClick(tester, find.byKey(const Key('tab.project#2')));
      await tester.tap(find.byKey(const Key('tab.menu.close')));
      await tester.pumpAndSettle();
      expect(ws.copiesOf(PanelRegistry.project), ['project']);

      final tab = find.byKey(const Key('tab.hierarchy'));
      await tester.tap(tab, kind: PointerDeviceKind.mouse);
      await tester.pump(const Duration(milliseconds: 50));
      await tester.tap(tab, kind: PointerDeviceKind.mouse);
      await tester.pumpAndSettle();
      expect(ws.isMaximized(PanelRegistry.hierarchy), isTrue);
      await tester.pump(kDoubleTapTimeout * 2);
      await tester.tap(tab, kind: PointerDeviceKind.mouse);
      await tester.pump(const Duration(milliseconds: 50));
      await tester.tap(tab, kind: PointerDeviceKind.mouse);
      await tester.pumpAndSettle();
      expect(ws.isMaximized(PanelRegistry.hierarchy), isFalse);
    });

    testWidgets('the Window menu has a New command per copyable panel', (tester) async {
      final shell = await pumpShell(tester);
      for (final (id, label) in [
        (PanelRegistry.hierarchy, 'New Hierarchy'),
        (PanelRegistry.scene, 'New Scene View'),
        (PanelRegistry.inspector, 'New Inspector'),
        (PanelRegistry.project, 'New Project Browser'),
      ]) {
        final c = shell.registry.byId('window.newCopy.$id')!;
        expect(c.label, label);
        c.run();
      }
      await tester.pumpAndSettle();
      for (final id in ['hierarchy#2', 'scene#2', 'inspector#2', 'project#2']) {
        expect(shell.workspace!.isVisible(id), isTrue, reason: id);
      }
      expect(shell.sceneOf('scene#2'), isNotNull);
      expect(shell.registry.byId('window.panel.inspector')!.label, 'Hide Inspector');
    });
  });

  group('copies in the shell', () {
    late Directory dir;
    setUp(() => dir = Directory.systemTemp.createTempSync('panels_test'));
    tearDown(() => dir.deleteSync(recursive: true));

    Future<EditorShellState> pumpShell(WidgetTester tester, MemoryLayoutStore store, {Key? key}) async {
      await pumpLarge(
        tester,
        shellApp(
          key: key,
          prefs: Preferences.inMemory(),
          store: store,
          projects: {dir.path: assetProject(dir.path)},
          initialProject: dir.path,
        ),
      );
      return shellOf(tester);
    }

    testWidgets('two Inspectors, one locked; "Show in Inspector" never breaks a lock; a restart brings them back', (
      tester,
    ) async {
      final store = MemoryLayoutStore();
      final shell = await pumpShell(tester, store);
      final ws = shell.workspace!;
      shell.newPanelCopy(PanelRegistry.inspector);
      await tester.pumpAndSettle();
      expect(
        find.descendant(of: find.byKey(const Key('tab.inspector#2')), matching: find.text('Inspector 2')),
        findsOneWidget,
      );
      final first = shell.selectionOf('inspector')!;
      final second = shell.selectionOf('inspector#2')!;
      expect(second, isNot(same(first)));

      // Both follow the browser until one is locked.
      final browser = shell.browserOf('project')!;
      browser.select(swatch);
      expect(first.target, const InspectAsset(swatch));
      expect(second.target, const InspectAsset(swatch));
      second.setLocked(true);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.locked.inspector#2')), findsOneWidget);
      expect(find.byKey(const Key('tab.locked.inspector')), findsNothing);
      browser.select(tone);
      expect(first.target, const InspectAsset(tone));
      expect(second.target, const InspectAsset(swatch), reason: 'the locked copy holds on');

      // inspector#2 was used last but is locked: the request goes to the other one.
      expect(ws.activeCopy(PanelRegistry.inspector), 'inspector#2');
      browser.select(swatch);
      first.setLocked(true);
      browser.select(tone);
      shell.showAssetInInspector();
      await tester.pumpAndSettle();
      expect(ws.copiesOf(PanelRegistry.inspector), ['inspector', 'inspector#2', 'inspector#3']);
      expect(
        shell.selectionOf('inspector#3')!.target,
        const InspectAsset(tone),
        reason: 'every copy locked: a new one',
      );
      expect(first.target, const InspectAsset(swatch));
      expect(second.target, const InspectAsset(swatch));
      first.setLocked(false);
      browser.select(swatch);
      browser.select(tone);
      expect(first.target, const InspectAsset(tone), reason: 'unlocked, it follows again');
      first.setLocked(true);
      shell.selectionOf('inspector#3')!.setLocked(false);
      shell.showAssetInInspector();
      expect(ws.copiesOf(PanelRegistry.inspector).length, 3, reason: 'an unlocked copy exists');

      // Closing a copy drops its models and what it remembered.
      ws.hide('inspector#3');
      await tester.pumpAndSettle();
      expect(shell.selectionOf('inspector#3'), isNull);
      expect(shell.editorState!.panel('inspector#3'), isNull);

      // A restart: the copies and their asset locks come back.
      await tester.tap(find.byKey(const Key('window.project')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('window.close')));
      await tester.pumpAndSettle();
      final again = await pumpShell(tester, store, key: UniqueKey());
      expect(again.workspace!.copiesOf(PanelRegistry.inspector), ['inspector', 'inspector#2']);
      expect(again.selectionOf('inspector')!.locked, isTrue);
      expect(again.selectionOf('inspector')!.target, const InspectAsset(tone));
      expect(again.selectionOf('inspector#2')!.locked, isTrue);
      expect(again.selectionOf('inspector#2')!.target, const InspectAsset(swatch));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.locked.inspector#2')), findsOneWidget);
    });

    testWidgets('two Project browsers in two folders; Reveal goes to the unlocked one; every Inspector follows both', (
      tester,
    ) async {
      final store = MemoryLayoutStore();
      final shell = await pumpShell(tester, store);
      final ws = shell.workspace!;
      shell.newPanelCopy(PanelRegistry.project);
      await tester.pumpAndSettle();
      final a = shell.browserOf('project')!;
      final b = shell.browserOf('project#2')!;
      b.setFolder('sounds');
      b.setLocked(true);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.locked.project#2')), findsOneWidget);
      expect(ws.activeCopy(PanelRegistry.project), 'project#2');

      shell.revealInProject(swatch);
      expect(a.folder, 'textures');
      expect(a.lead, swatch);
      expect(b.folder, 'sounds', reason: 'the locked browser keeps its folder');
      expect(shell.selectionOf('inspector')!.target, const InspectAsset(swatch));

      b.select(tone);
      expect(
        shell.selectionOf('inspector')!.target,
        const InspectAsset(tone),
        reason: 'the second browser moves it too',
      );

      // Everything locked: Reveal opens a third browser.
      a.setLocked(true);
      final restore = ignoreTabStripCornerOverflow();
      shell.revealInProject(tone);
      await tester.pumpAndSettle();
      expect(ws.copiesOf(PanelRegistry.project), ['project', 'project#2', 'project#3']);
      expect(shell.browserOf('project#3')!.folder, 'sounds');

      // A restart: the folders and locks come back.
      ws.hide('project#3');
      await tester.pumpAndSettle();
      restore();
      await tester.tap(find.byKey(const Key('window.project')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('window.close')));
      await tester.pumpAndSettle();
      final again = await pumpShell(tester, store, key: UniqueKey());
      expect(again.browserOf('project#2')!.folder, 'sounds');
      expect(again.browserOf('project#2')!.locked, isTrue);
      expect(again.browserOf('project')!.folder, 'textures');
      expect(again.browserOf('project#3'), isNull);
    });

    testWidgets('a Hierarchy locked to a document keeps it; focusing it brings that document forward', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      project.addDocument(smallScene());
      project.addDocument(smallScene(id: 'scene-b', path: '/fake/game/assets/scenes/b.scene.json'));
      await pumpLarge(
        tester,
        shellApp(prefs: Preferences.inMemory(), projects: {'/fake/game': project}, initialProject: '/fake/game'),
      );
      final shell = shellOf(tester);
      final ws = shell.workspace!;
      final ta = shell.openDocument('scene-a')!;
      shell.newPanelCopy(PanelRegistry.hierarchy);
      await tester.pumpAndSettle();
      final locked = shell.hierarchyOf('hierarchy#2')!;
      locked.setLocked(true);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('tab.locked.hierarchy#2')), findsOneWidget);
      expect(find.byKey(const Key('hierarchy.lockedTo')), findsOneWidget);

      final tb = shell.openDocument('scene-b')!;
      expect(shell.hierarchyOf('hierarchy')!.tab, tb);
      expect(locked.tab, ta);

      // A Console row in scene A goes to the Hierarchy showing scene A.
      shell.openDiagnosticInEditor(
        const Diagnostic(code: 'FYN-X', severity: Severity.warning, message: 'm', document: 'scene-a', id: 'arm'),
      );
      await tester.pumpAndSettle();
      expect(shell.tabs!.focused, ta);
      expect(ta.selection, ['arm']);
      expect(ws.copiesOf(PanelRegistry.hierarchy).length, 2);

      // Back to B; focusing the locked copy brings A forward.
      shell.tabs!.focus(tb);
      ws.focusedPanel.value = 'hierarchy#2';
      expect(shell.tabs!.focused, ta);

      // Every copy locked to A: a row in B opens a copy that follows B.
      shell.hierarchyOf('hierarchy')!.setLocked(true);
      shell.openDiagnosticInEditor(
        const Diagnostic(code: 'FYN-X', severity: Severity.warning, message: 'm', document: 'scene-b', id: 'floor'),
      );
      await tester.pumpAndSettle();
      expect(ws.copiesOf(PanelRegistry.hierarchy), ['hierarchy', 'hierarchy#2', 'hierarchy#3']);
      expect(shell.hierarchyOf('hierarchy#3')!.tab, tb);
      expect(tb.selection, ['floor']);

      // Closing the document releases the locks on it.
      shell.tabs!.closeDiscarding(ta);
      await tester.pumpAndSettle();
      expect(locked.locked, isFalse);
    });
  });
}
