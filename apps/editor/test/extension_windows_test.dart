// Editor extensions (ADR-0028 step 5): a package's windows are dockable
// panels that follow the editor and keep their place across reloads and
// restarts; its context entries join the Hierarchy's, the Project panel's
// and a component card's menus.
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Severity;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/commands/commands.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import 'support/fakes.dart';

const _goldId = '901d0000-0000-4000-8000-000000000001';
const _mainId = '3a1a0000-0000-4000-8000-000000000002';

// What the fake package registers; a test changes it before a "reload".
var _withWindow = true;
var _windowThrows = false;
final _ran = <String>[];

void _tools(EditorExtensions editor) {
  if (_withWindow) {
    editor.window(
      'game.levelTools',
      title: 'Level Tools',
      menu: 'Tools/Level Tools',
      build: (context, editor) {
        if (_windowThrows) throw StateError('no level');
        final scene = editor.scene;
        return Column(
          children: [
            Text('scene: ${scene?.name ?? 'none'}', key: const Key('levelTools.scene')),
            Text('selected: ${editor.selection.length}', key: const Key('levelTools.selected')),
            TextButton(
              key: const Key('levelTools.tidy'),
              onPressed: () => editor.edit('Tidy', (edit) {
                for (final o in editor.selection) {
                  edit.rename(o, 'Tidy ${o.name}');
                }
              }),
              child: const Text('Tidy'),
            ),
          ],
        );
      },
    );
  }
  editor.hierarchyMenuItem(
    'Make Big Coin',
    id: 'game.makeBigCoin',
    withComponent: 'game.Collectible',
    run: (context, objects) => _ran.add('big ${[for (final o in objects) o.id]}'),
  );
  editor.hierarchyMenuItem(
    'Count',
    id: 'game.count',
    enabled: (context, objects) => objects.length > 1,
    run: (context, objects) => _ran.add('count ${objects.length}'),
  );
  editor.projectMenuItem(
    'Price Coin',
    id: 'game.priceCoin',
    dataType: 'game.CoinKind',
    run: (context, assets) => _ran.add('price ${[for (final a in assets) a.id]}'),
  );
  editor.componentMenuItem(
    'game.Collectible',
    'Double Value',
    id: 'game.doubleValue',
    run: (context, objects) => _ran.add('double ${[for (final o in objects) o.id]}'),
  );
}

FakeDocument _scene() => FakeDocument(
  path: '/fake/game/assets/scenes/main.scene.json',
  relativePath: 'scenes/main.scene.json',
  id: 'main',
  name: 'Main',
  entities: {
    'cam': FakeDocument.entity('Camera'),
    'coin': FakeDocument.entity(
      'Coin',
      components: {
        'c': {
          'type': 'game.Collectible',
          'version': 1,
          'data': {'value': 1},
        },
      },
    ),
  },
);

void main() {
  late FakeProject project;
  late Preferences prefs;
  late MemoryLayoutStore layout;

  setUp(() {
    project = FakeProject(dir: '/fake/game');
    project.addDocument(_scene());
    project.assetEntries = [
      const AssetEntry(id: _mainId, path: 'main.scene.json', name: 'main', kind: 'document', document: 'scene'),
      const AssetEntry(
        id: _goldId,
        path: 'gold.data.json',
        name: 'gold',
        kind: 'document',
        document: 'data',
        dataType: 'game.CoinKind',
      ),
    ];
    prefs = Preferences.inMemory();
    layout = MemoryLayoutStore();
    _withWindow = true;
    _windowThrows = false;
    _ran.clear();
  });

  Future<EditorShellState> openShell(
    WidgetTester tester, {
    List<EditorPart> parts = const [EditorPart('game', _tools)],
  }) async {
    await pumpLarge(
      tester,
      shellApp(
        prefs: prefs,
        store: layout,
        projects: {'/fake/game': project},
        initialProject: '/fake/game',
        extensions: parts,
      ),
    );
    return tester.state<EditorShellState>(find.byType(EditorShell));
  }

  Future<void> reload(WidgetTester tester) async {
    final r = tester.binding.reassembleApplication();
    await tester.pump();
    await r;
    await tester.pumpAndSettle();
  }

  List<String> errors(EditorShellState shell) => [
    for (final d in shell.extensions.log)
      if (d.severity == Severity.error) d.message,
  ];

  testWidgets('a window is a panel its menu item shows; it follows the scene and edits it', (tester) async {
    final shell = await openShell(tester);
    final command = shell.registry.byId('window.panel.ext:game.levelTools')!;
    expect((command.menu, command.label), (EditorMenu.tools, 'Level Tools'));
    expect(shell.workspace!.isVisible('ext:game.levelTools'), isFalse);

    expect(shell.registry.execute(command.id), isTrue);
    await tester.pumpAndSettle();
    expect(shell.workspace!.isVisible('ext:game.levelTools'), isTrue);
    expect(find.text('scene: none'), findsOneWidget);

    // It follows the document in front and its selection.
    final tab = shell.openDocument('main')!;
    await tester.pumpAndSettle();
    expect(find.text('scene: Main'), findsOneWidget);
    tab.setSelection(['cam', 'coin']);
    await tester.pumpAndSettle();
    expect(find.text('selected: 2'), findsOneWidget);

    // Its button edits through the context: one undo step.
    await tester.tap(find.byKey(const Key('levelTools.tidy')));
    await tester.pumpAndSettle();
    expect(tab.tree['coin']!.name, 'Tidy Coin');
    expect(tab.undo(), isTrue);
    expect(tab.tree['coin']!.name, 'Coin');
  });

  testWidgets('a reload that drops the window keeps its place and says so; one that brings it back shows it', (
    tester,
  ) async {
    final shell = await openShell(tester);
    shell.registry.execute('window.panel.ext:game.levelTools');
    await tester.pumpAndSettle();

    _withWindow = false;
    await reload(tester);
    expect(shell.registry.byId('window.panel.ext:game.levelTools'), isNull);
    expect(shell.workspace!.isVisible('ext:game.levelTools'), isTrue);
    expect(find.byKey(const Key('extension.window.missing')), findsOneWidget);

    _withWindow = true;
    await reload(tester);
    expect(find.text('scene: none'), findsOneWidget);
  });

  testWidgets('a throwing window shows the error in its frame and logs it once', (tester) async {
    final shell = await openShell(tester);
    _windowThrows = true;
    shell.registry.execute('window.panel.ext:game.levelTools');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('extension.window.error')), findsOneWidget);
    shell.openDocument('main');
    await tester.pumpAndSettle();
    expect(errors(shell), hasLength(1));
    expect(errors(shell).single, contains('window "Level Tools" failed: Bad state: no level'));
  });

  test('a saved layout with a window no extension registers loads without it', () {
    Widget none(BuildContext context, String id) => const SizedBox();
    PanelSpec spec(String id) => PanelSpec(id: id, title: id, icon: Icons.abc, home: PanelHome.right, builder: none);
    final base = [for (final id in PanelRegistry.defaultOrder) spec(id)];
    final store = MemoryLayoutStore();
    final withWindow = PanelRegistry(base)..setExtensionSpecs([spec('ext:game.levelTools')]);
    Workspace(registry: withWindow, store: store)
      ..show('ext:game.levelTools')
      ..save();
    expect(store.snapshot!.layout, contains('ext:game.levelTools'));

    final again = Workspace(registry: withWindow, store: store);
    expect(again.isVisible('ext:game.levelTools'), isTrue, reason: 'its place is kept');

    final without = Workspace(registry: PanelRegistry(base), store: store);
    expect(without.restoreProblem, isNull);
    expect(without.isVisible('ext:game.levelTools'), isFalse);
    expect(without.isVisible(PanelRegistry.inspector), isTrue, reason: 'the rest of the layout stays');
  });

  testWidgets('Hierarchy entries: offered for the selection, filtered by component, greyed by enabled', (tester) async {
    final shell = await openShell(tester);
    final tab = shell.openDocument('main')!;
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('hierarchy.name.coin')), buttons: kSecondaryButton);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('extension.hierarchy.game.makeBigCoin')), findsOneWidget);
    final count = tester.widget<MenuItemButton>(find.byKey(const Key('extension.hierarchy.game.count')));
    expect(count.onPressed, isNull, reason: 'one object: Count is greyed out');
    await tester.tap(find.byKey(const Key('extension.hierarchy.game.makeBigCoin')));
    await tester.pumpAndSettle();
    expect(_ran, ['big [coin]']);

    // The camera has no Collectible: Make Big Coin is not offered.
    tab.setSelection(['cam']);
    await tester.tap(find.byKey(const Key('hierarchy.name.cam')), buttons: kSecondaryButton);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('extension.hierarchy.game.makeBigCoin')), findsNothing);
    expect(find.byKey(const Key('extension.hierarchy.game.count')), findsOneWidget);
  });

  testWidgets('Project entries: offered for assets of their data type', (tester) async {
    await openShell(tester);
    await tester.tap(find.byKey(const Key('project.asset.$_goldId')), buttons: kSecondaryButton);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('extension.project.game.priceCoin')));
    await tester.pumpAndSettle();
    expect(_ran, ['price [$_goldId]']);

    await tester.tap(find.byKey(const Key('project.asset.$_mainId')), buttons: kSecondaryButton);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('extension.project.game.priceCoin')), findsNothing);
  });

  testWidgets('a component card\'s ⋮ has the entries for its type', (tester) async {
    final shell = await openShell(tester);
    final tab = shell.openDocument('main')!;
    tab.setSelection(['coin']);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('card.menu.game.Collectible#0')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('extension.component.game.doubleValue')));
    await tester.pumpAndSettle();
    expect(_ran, ['double [coin]']);

    await tester.tap(find.byKey(const ValueKey('card.menu.fynkel.Transform#0')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('extension.component.game.doubleValue')), findsNothing);
  });
}
