// The docking workspace: the §22.2 default, toggle, save → restore, reset,
// a corrupt store, and the shell's layout surviving a "restart".
import 'package:docking/docking.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

PanelRegistry registry() => PanelRegistry([
  for (final (id, home) in [
    (PanelRegistry.hierarchy, PanelHome.left),
    (PanelRegistry.scene, PanelHome.center),
    (PanelRegistry.inspector, PanelHome.right),
    (PanelRegistry.project, PanelHome.bottom),
    (PanelRegistry.console, PanelHome.bottom),
    (PanelRegistry.profiler, PanelHome.bottom),
    (PanelRegistry.animation, PanelHome.bottom),
    (PanelRegistry.import, PanelHome.bottom),
  ])
    PanelSpec(id: id, title: id, icon: Icons.square, home: home, builder: (_) => Text('body $id')),
]);

const defaultShape = 'R(Ihierarchy,C(Iscene,T(Iproject,Iconsole,Iprofiler,Ianimation,Iimport)),Iinspector)';

void main() {
  test('the default workspace is the §22.2 shape and is saved on first use', () {
    final store = MemoryLayoutStore();
    final ws = Workspace(registry: registry(), store: store);
    expect(ws.layout.hierarchy(nameInfo: true), defaultShape);
    expect(ws.restoreProblem, isNull);
    expect(ws.visiblePanels.length, 8);
    ws.save();
    expect(store.read()!.dockingFormat, 'V1');
    expect(store.read()!.selected, ['project']);
  });

  test('toggle hides and shows; a shown panel returns to its home tabs; the last panel stays', () {
    final ws = Workspace(registry: registry(), store: MemoryLayoutStore());
    ws.toggle(PanelRegistry.console);
    expect(ws.isVisible(PanelRegistry.console), isFalse);
    expect(
      ws.layout.hierarchy(nameInfo: true),
      'R(Ihierarchy,C(Iscene,T(Iproject,Iprofiler,Ianimation,Iimport)),Iinspector)',
    );
    ws.toggle(PanelRegistry.console);
    expect(
      ws.layout.hierarchy(nameInfo: true),
      'R(Ihierarchy,C(Iscene,T(Iproject,Iprofiler,Ianimation,Iimport,Iconsole)),Iinspector)',
    );
    expect(ws.layout.findDockingTabsWithItem(PanelRegistry.console)!.selectedIndex, 4);

    ws.hide(PanelRegistry.hierarchy);
    ws.show(PanelRegistry.hierarchy); // no left sibling → the left edge of the whole workspace
    expect(
      ws.layout.hierarchy(nameInfo: true),
      'R(Ihierarchy,C(Iscene,T(Iproject,Iprofiler,Ianimation,Iimport,Iconsole)),Iinspector)',
    );
    expect(ws.layout.findDockingTabsWithItem(PanelRegistry.console)!.selectedIndex, 4, reason: 'selection survives');
    ws.hide(PanelRegistry.inspector);
    ws.show(PanelRegistry.inspector);
    expect(ws.layout.hierarchy(nameInfo: true), endsWith(',Iinspector)'));

    for (final id in PanelRegistry.defaultOrder) {
      ws.hide(id);
    }
    expect(ws.visiblePanels.length, 1, reason: 'the last panel cannot be hidden');
    ws.show(PanelRegistry.console);
    expect(ws.visiblePanels.length, 2);
  });

  test('a saved layout restores identically, including the selected tab', () {
    final store = MemoryLayoutStore();
    final ws = Workspace(registry: registry(), store: store);
    ws.hide(PanelRegistry.profiler);
    ws.focus(PanelRegistry.console);
    final tabs = ws.layout.findDockingTabsWithItem(PanelRegistry.console)!;
    expect(tabs.selectedIndex, 1);
    ws.layout.moveItem(
      draggedItem: ws.layout.findDockingItem(PanelRegistry.inspector)!,
      targetArea: ws.layout.findDockingItem(PanelRegistry.hierarchy)!,
      dropPosition: DropPosition.bottom,
    );
    final before = ws.layout.hierarchy(nameInfo: true);
    final saved = store.read()!;
    expect(saved.selected, ['console']);

    final again = Workspace(registry: registry(), store: store);
    expect(again.restoreProblem, isNull);
    expect(again.layout.hierarchy(nameInfo: true), before);
    expect(again.layout.findDockingTabsWithItem(PanelRegistry.console)!.selectedIndex, 1);
    expect(again.snapshot().layout, saved.layout);
    expect(again.isVisible(PanelRegistry.profiler), isFalse);
  });

  test('a corrupt or foreign-format store falls back to the default and says why', () {
    final store = MemoryLayoutStore()..write(const LayoutSnapshot(layout: 'garbage', selected: []));
    final ws = Workspace(registry: registry(), store: store);
    expect(ws.layout.hierarchy(nameInfo: true), defaultShape);
    expect(ws.restoreProblem, contains('could not be loaded'));

    store.write(const LayoutSnapshot(layout: 'V2:whatever', selected: [], dockingFormat: 'V2'));
    final ws2 = Workspace(registry: registry(), store: store);
    expect(ws2.restoreProblem, contains('format "V2"'));

    expect(LayoutSnapshot.fromJson({'layout': 1}), isNull);
    expect(LayoutSnapshot.fromJson('nope'), isNull);
  });

  test('reset restores the default and saves it', () {
    final store = MemoryLayoutStore();
    final ws = Workspace(registry: registry(), store: store);
    ws.hide(PanelRegistry.scene);
    ws.hide(PanelRegistry.console);
    expect(ws.layout.hierarchy(nameInfo: true), isNot(defaultShape));
    ws.reset();
    expect(ws.layout.hierarchy(nameInfo: true), defaultShape);
    final again = Workspace(registry: registry(), store: store);
    expect(again.layout.hierarchy(nameInfo: true), defaultShape);
  });

  testWidgets('the shell shows all eight panels, keeps them alive, and the layout survives a restart', (tester) async {
    final store = MemoryLayoutStore();
    final prefs = Preferences.inMemory();
    var project = FakeProject(dir: '/fake/game');
    await pumpLarge(
      tester,
      shellApp(prefs: prefs, store: store, projects: {'/fake/game': project}, initialProject: '/fake/game'),
    );
    for (final title in ['Hierarchy', 'Scene', 'Inspector', 'Project', 'Console', 'Profiler', 'Animation', 'Import']) {
      expect(find.text(title, skipOffstage: false), findsWidgets, reason: '$title tab');
    }
    expect(
      find.textContaining('Arrives with editor item 3.', skipOffstage: false),
      findsOneWidget,
    ); // Hierarchy placeholder
    expect(find.byKey(const Key('console.search'), skipOffstage: false), findsOneWidget); // Console alive while hidden

    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    shell.togglePanel(PanelRegistry.profiler);
    shell.workspace!.focus(PanelRegistry.console);
    await tester.pumpAndSettle();
    final saved = store.read()!;
    final shape = shell.workspace!.layout.hierarchy(nameInfo: true);
    expect(saved.selected, ['console']);
    expect(saved.layout, isNot(contains('profiler')));

    // A tab close through the docking UI hides the panel and saves.
    await tester.tap(find.byKey(const Key('window.close')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('launcher.open')), findsOneWidget);

    // "Restart": a new app (a new key, so a fresh shell) over the same store.
    project = FakeProject(dir: '/fake/game');
    await pumpLarge(
      tester,
      shellApp(
        key: UniqueKey(),
        prefs: prefs,
        store: store,
        projects: {'/fake/game': project},
        initialProject: '/fake/game',
      ),
    );
    final shell2 = tester.state<EditorShellState>(find.byType(EditorShell));
    expect(shell2.workspace!.isVisible(PanelRegistry.profiler), isFalse);
    // Weights are re-measured after the first frame, so compare the shape, not the string.
    expect(shell2.workspace!.layout.hierarchy(nameInfo: true), shape);
    expect(shell2.workspace!.layout.findDockingTabsWithItem(PanelRegistry.console)!.selectedIndex, 1);
    expect(find.byKey(const Key('window.layoutNotice')), findsNothing);

    shell2.resetLayout();
    await tester.pumpAndSettle();
    expect(shell2.workspace!.isVisible(PanelRegistry.profiler), isTrue);
  });

  testWidgets('a corrupt saved layout shows a notice in the window', (tester) async {
    final store = MemoryLayoutStore()..write(const LayoutSnapshot(layout: 'broken', selected: []));
    final project = FakeProject(dir: '/fake/game');
    await pumpLarge(
      tester,
      shellApp(
        prefs: Preferences.inMemory(),
        store: store,
        projects: {'/fake/game': project},
        initialProject: '/fake/game',
      ),
    );
    expect(find.byKey(const Key('window.layoutNotice')), findsOneWidget);
    await tester.tap(find.text('OK'));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('window.layoutNotice')), findsNothing);
  });
}
