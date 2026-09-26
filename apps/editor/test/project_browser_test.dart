// The Project browser (item 5) over the fake project: the model (folders,
// list, search, kind filters, sort, selection, previews, drag payloads,
// every operation and its refusal), the Project panel, the Inspector's
// asset, part and folder pages (item 9c), a prefab dropped onto the
// Hierarchy, and the shell's asset commands.
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/theme/icons.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_model.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_panel.dart';
import 'package:fynkel_editor/src/inspector/asset_pages.dart';
import 'package:fynkel_editor/src/inspector/field_widgets.dart';
import 'package:fynkel_editor/src/inspector/inspector_chrome.dart';
import 'package:fynkel_editor/src/inspector/inspector_model.dart';
import 'package:fynkel_editor/src/inspector/inspector_panel.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/project_browser/project_browser_model.dart';
import 'package:fynkel_editor/src/project_browser/project_panel.dart';
import 'package:fynkel_editor/src/shell/editor_selection.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'support/fakes.dart';

const mainScene = 's1-0000-0000-0000-000000000001';
const coinPrefab = 'p1-0000-0000-0000-000000000001';
const rusted = 'm1-0000-0000-0000-000000000001';
const swatch = 't1-0000-0000-0000-000000000001';
const robot = 'g1-0000-0000-0000-000000000001';
const tone = 'a1-0000-0000-0000-000000000001';
const loose = 'l1-0000-0000-0000-000000000001';

FakeProject fakeProject({String dir = '/fake/game', String? readOnly}) {
  final p = FakeProject(dir: dir, readOnly: readOnly);
  p.assetEntries = [
    const AssetEntry(id: mainScene, path: 'scenes/main.scene.json', name: 'Main', kind: 'document', document: 'scene'),
    const AssetEntry(
      id: coinPrefab,
      path: 'prefabs/coin.prefab.json',
      name: 'Coin',
      kind: 'document',
      document: 'prefab',
    ),
    const AssetEntry(
      id: rusted,
      path: 'materials/rusted.material.json',
      name: 'rusted',
      kind: 'document',
      document: 'material',
    ),
    const AssetEntry(id: swatch, path: 'textures/swatch.png', name: 'swatch', kind: 'source', importer: 'fynkel.image'),
    const AssetEntry(
      id: robot,
      path: 'models/robot.glb',
      name: 'robot',
      kind: 'source',
      importer: 'fynkel.gltf',
      subAssets: {'mesh/0': 'u-mesh', 'prefab': 'u-prefab', 'anim/0': 'u-anim'},
    ),
    const AssetEntry(id: tone, path: 'sounds/tone.wav', name: 'tone', kind: 'source', importer: 'fynkel.audio'),
    const AssetEntry(id: loose, path: 'loose.scene.json', name: 'Loose', kind: 'document', document: 'scene'),
  ];
  p.fakeDependents = {
    swatch: [
      {'document': rusted, 'path': 'materials/rusted.material.json', 'pointer': '/base_color_texture', 'asset': swatch},
    ],
    rusted: [
      {
        'document': mainScene,
        'path': 'scenes/main.scene.json',
        'pointer': '/entities/e1/components/m/data/material',
        'asset': rusted,
      },
      {
        'document': mainScene,
        'path': 'scenes/main.scene.json',
        'pointer': '/entities/e2/components/m/data/material',
        'asset': rusted,
      },
    ],
  };
  p.materialFields = {
    rusted: {
      'base_color': [0.5, 0.25, 0.125, 1.0],
      'metallic': 0.2,
    },
  };
  p.assetCount = p.assetEntries.length;
  return p;
}

void main() {
  group('ProjectBrowserModel', () {
    test('item 9e: the padlock and what a restart brings back (folder, view, lock)', () {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      var notified = 0;
      model.addListener(() => notified++);
      model.setFolder('sounds');
      model.setView(BrowserView.list);
      expect(model.remembered, {'folder': 'sounds', 'view': 'list'});
      model.setLocked(true);
      expect(notified, greaterThanOrEqualTo(3));
      expect(model.remembered, {'folder': 'sounds', 'view': 'list', 'locked': true});
      // A locked browser still follows the user's own clicks.
      model.setFolder('textures');
      expect(model.folder, 'textures');

      final again = ProjectBrowserModel(project: project);
      addTearDown(again.dispose);
      again.recall({'folder': 'sounds', 'view': 'list', 'locked': true});
      expect((again.folder, again.view, again.locked), ('sounds', BrowserView.list, true));
      final gone = ProjectBrowserModel(project: project);
      addTearDown(gone.dispose);
      gone.recall({'folder': 'deleted', 'view': 'nonsense'});
      expect((gone.folder, gone.view, gone.locked), ('', BrowserView.folders, false));
    });

    test('folders view shows a folder\'s own assets and sub-folders; the list view shows everything sorted', () {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      expect(model.view, BrowserView.folders);
      expect(model.visibleFolders, ['materials', 'models', 'prefabs', 'scenes', 'sounds', 'textures']);
      expect(model.visibleAssets.map((e) => e.id), [loose]);
      model.setFolder('scenes');
      expect(model.visibleFolders, isEmpty);
      expect(model.visibleAssets.map((e) => e.id), [mainScene]);
      model.goUp();
      expect(model.folder, '');
      model.setView(BrowserView.list);
      expect(model.visibleFolders, isEmpty);
      expect(model.visibleAssets.map((e) => e.path), [
        'prefabs/coin.prefab.json',
        'loose.scene.json',
        'scenes/main.scene.json',
        'models/robot.glb',
        'materials/rusted.material.json',
        'textures/swatch.png',
        'sounds/tone.wav',
      ]);
      model.setSort(AssetSort.kind);
      expect(model.visibleAssets.map(browserKindOf).toList(), [
        'material',
        'model',
        'prefab',
        'scene',
        'scene',
        'sound',
        'texture',
      ]);
      model.setSort(AssetSort.kind); // again: flips
      expect(model.descending, isTrue);
      expect(browserKindOf(model.visibleAssets.first), 'texture');
      model.setSort(AssetSort.path);
      expect(model.descending, isFalse);
      expect(model.visibleAssets.first.path, 'loose.scene.json');
      expect(model.countsByKind, {'scene': 2, 'prefab': 1, 'material': 1, 'texture': 1, 'model': 1, 'sound': 1});
    });

    test(
      'search matches path, name, id and sub-asset ids; kind filters narrow; a search under a folder is recursive',
      () {
        final project = fakeProject();
        final model = ProjectBrowserModel(project: project);
        addTearDown(model.dispose);
        model.setView(BrowserView.list);
        model.setQuery('rob');
        expect(model.visibleAssets.map((e) => e.id), [robot]);
        model.setQuery('mesh/0');
        expect(model.visibleAssets.map((e) => e.id), [robot]);
        model.setQuery('Coin');
        expect(model.visibleAssets.map((e) => e.id), [coinPrefab]);
        model.setQuery('t1-0000');
        expect(model.visibleAssets.map((e) => e.id), [swatch]);
        model.setQuery('');
        model.toggleKind('texture');
        model.toggleKind('sound');
        expect(model.visibleAssets.map((e) => e.id), unorderedEquals([swatch, tone]));
        model.toggleKind('sound');
        expect(model.visibleAssets.map((e) => e.id), [swatch]);
        model.clearFilters();
        expect(model.filtering, isFalse);
        // Folders view: a search from the root looks everywhere, from a folder under it only.
        model.setView(BrowserView.folders);
        model.setQuery('scene');
        expect(model.visibleFolders, isEmpty, reason: 'a search shows assets only');
        expect(model.visibleAssets.map((e) => e.id), unorderedEquals([loose, mainScene]));
        model.setFolder('scenes');
        expect(model.visibleAssets.map((e) => e.id), [mainScene]);
      },
    );

    test('selection: click, ⌘-toggle, ⇧-range, arrows, select all, reveal', () {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      model.setView(BrowserView.list);
      final rows = model.visibleAssets.map((e) => e.id).toList();
      model.select(rows[1]);
      expect(model.selection, {rows[1]});
      expect(model.lead, rows[1]);
      model.select(rows[3], toggle: true);
      expect(model.selection, {rows[1], rows[3]});
      model.select(rows[3], toggle: true);
      expect(model.selection, {rows[1]});
      model.select(rows[4], range: true);
      expect(model.selection, {rows[1], rows[2], rows[3], rows[4]});
      model.select(rows[0]);
      model.step(1);
      expect(model.lead, rows[1]);
      model.step(1, extend: true);
      expect(model.selection, {rows[1], rows[2]});
      model.step(-10);
      expect(model.lead, rows[0]);
      model.selectAll();
      expect(model.selection.length, 7);
      model.clearSelection();
      expect(model.selection, isEmpty);
      expect(model.lead, isNull);
      model.setView(BrowserView.folders);
      model.reveal(robot);
      expect(model.folder, 'models');
      expect(model.lead, robot);
      expect(model.selectedEntries.single.id, robot);
      model.select('nobody');
      expect(model.lead, robot, reason: 'an unknown id is ignored');
    });

    test('previews and drag payloads per kind', () {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      final e = {for (final x in model.entries) x.id: x};
      expect(
        model.preview(e[swatch]!),
        isA<TexturePreview>().having((t) => t.file, 'file', '/fake/game/assets/textures/swatch.png'),
      );
      final sw = model.preview(e[rusted]!) as SwatchPreview;
      expect(sw.color.r, closeTo(0.5, 1e-6));
      expect(sw.color.g, closeTo(0.25, 1e-6));
      expect(sw.color.a, closeTo(1, 1e-6));
      expect(model.preview(e[robot]!), isA<IconPreview>().having((i) => i.kind, 'kind', 'mesh'));
      expect(model.preview(e[tone]!), isA<IconPreview>().having((i) => i.kind, 'kind', 'sound'));
      expect(model.preview(e[mainScene]!), isA<IconPreview>().having((i) => i.kind, 'kind', 'scene'));

      final scene = model.dragPayload(e[mainScene]!)!;
      expect((scene.id, scene.sub, scene.kind), (mainScene, null, 'scene'));
      final prefab = model.dragPayload(e[coinPrefab]!)!;
      expect(prefab.kind, 'prefab');
      expect(prefab.toRef(), {'asset': coinPrefab});
      final imported = model.dragPayload(e[robot]!)!;
      expect((imported.sub, imported.kind, imported.name), ('prefab', 'prefab', 'robot.glb#prefab'));
      expect(imported.toRef(), {'asset': robot, 'sub': 'prefab'});
      final mesh = model.dragPayload(e[robot]!, sub: 'mesh/0')!;
      expect(mesh.kind, 'mesh');
      expect(model.dragPayload(e[robot]!, sub: 'nope'), isNull);
      final sound = model.dragPayload(e[tone]!)!;
      expect((sound.sub, sound.kind), (null, 'sound'));
      final image = model.dragPayload(e[swatch]!)!;
      expect((image.sub, image.kind), (null, 'texture'), reason: 'an image is referenced whole');
      final skin = model.dragPayload(e[robot]!, sub: 'anim/0')!;
      expect(skin.kind, 'clip');
    });

    test(
      'new documents get fresh names in the shown folder, are selected and ready to rename; rename fixes the suffix',
      () {
        final project = fakeProject();
        final model = ProjectBrowserModel(project: project);
        addTearDown(model.dispose);
        model.setFolder('scenes');
        final r = model.newDocument('scene')!;
        final rid = r.id!;
        expect(model.assetPath(r.path!), 'scenes/New Scene.scene.json');
        expect(model.renamingAsset, rid);
        expect(model.selection, {rid});
        expect(model.visibleAssets.map((e) => e.path), contains('scenes/New Scene.scene.json'));
        final r2 = model.newDocument('scene')!;
        expect(model.assetPath(r2.path!), 'scenes/New Scene 2.scene.json');
        final m = model.newDocument('material', inFolder: 'materials')!;
        expect(model.assetPath(m.path!), 'materials/New Material.material.json');
        expect(model.folder, 'materials', reason: 'the browser follows the new file');
        expect(project.operations.last.$1, 'newDocument');
        expect(model.history.canUndo, isTrue);
        expect(model.history.undoLabel, 'New material New Material.material.json');

        // Rename: the typed stem gets the document suffix back; sources keep their extension.
        expect(ProjectBrowserModel.editableName(model.entry(rid)!), 'New Scene');
        expect(model.rename(rid, 'level1'), isTrue);
        expect(model.entry(rid)!.path, 'scenes/level1.scene.json');
        expect(model.renamingAsset, isNull);
        expect(model.rename(rid, 'level1'), isFalse, reason: 'unchanged is no operation');
        expect(model.rename(rid, '   '), isFalse);
        expect(ProjectBrowserModel.editableName(model.entry(robot)!), 'robot.glb');
        expect(ProjectBrowserModel.renamedPath(model.entry(robot)!, 'bot'), 'models/bot.glb');
        expect(ProjectBrowserModel.renamedPath(model.entry(robot)!, 'bot.GLB'), 'models/bot.GLB');
        expect(model.rename(robot, 'bot'), isTrue);
        expect(model.entry(robot)!.path, 'models/bot.glb');
        // Undo and redo run through the project's history.
        model.undo();
        expect(model.entry(robot)!.path, 'models/robot.glb');
        model.redo();
        expect(model.entry(robot)!.path, 'models/bot.glb');
        // A typed extension never changes the real one.
        expect(model.rename(robot, 'bot.png'), isTrue);
        expect(model.entry(robot)!.path, 'models/bot.png.glb');
        // A refused rename is the error, nothing moved.
        expect(model.move([loose], 'scenes'), 1);
        expect(model.rename(loose, 'level1'), isFalse);
        expect(model.lastError, contains('exists'));
        expect(model.entry(loose)!.path, 'scenes/loose.scene.json');
        model.clearError();
        expect(model.lastError, isNull);
      },
    );

    test('move drops into folders, one operation each; canMove refuses the same folder', () {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      expect(model.canMove([swatch, tone], 'textures'), isTrue, reason: 'tone would move');
      expect(model.canMove([swatch], 'textures'), isFalse);
      expect(model.move([swatch, tone], 'textures'), 1);
      expect(model.entry(tone)!.path, 'textures/tone.wav');
      expect(model.move([tone], ''), 1);
      expect(model.entry(tone)!.path, 'tone.wav');
      expect(project.operations.where((o) => o.$1 == 'renameAsset').length, 2);
    });

    test('folders: new, rename (every asset under it moves through the session), delete only when empty', () {
      final tmp = Directory.systemTemp.createTempSync('fynkel_editor_browser');
      addTearDown(() => tmp.deleteSync(recursive: true));
      final project = fakeProject(dir: tmp.path);
      Directory('${tmp.path}/assets/empty').createSync(recursive: true);
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      expect(model.newFolder(), isTrue);
      expect(model.folders, contains('New Folder'));
      expect(model.renamingFolder, 'New Folder');
      expect(model.newFolder(inFolder: 'scenes'), isTrue);
      expect(model.folders, contains('scenes/New Folder'));
      model.cancelRename();
      expect(model.renamingFolder, isNull);

      // Renaming a folder with assets: one renameAsset per asset, the id kept.
      expect(model.renameFolder('models', 'characters'), isTrue);
      expect(model.entry(robot)!.path, 'characters/robot.glb');
      expect(model.folders, contains('characters'));
      expect(project.operations.where((o) => o.$1 == 'renameAsset').single.$2['path'], 'characters/robot.glb');
      expect(model.renameFolder('scenes', 'scenes'), isFalse, reason: 'unchanged');
      expect(model.renameFolder('scenes', 'characters'), isFalse);
      expect(model.lastError, contains('already there'));

      // Deleting: refused with assets under it, done when empty.
      expect(model.deleteFolder('scenes'), isFalse);
      expect(model.lastError, contains('holds 1 asset'));
      project.extraFolders.add('empty');
      model.refresh();
      model.setFolder('empty');
      expect(model.deleteFolder('empty'), isTrue);
      expect(Directory('${tmp.path}/assets/empty').existsSync(), isFalse);
      expect(model.folder, '', reason: 'the shown folder went, its parent is shown');
    });

    test('delete: refused while referenced with the dependents parked for "delete anyway"; forced goes through; undo returns it', () {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      model.select(rusted);
      expect(model.delete(), isFalse);
      final refusal = model.pendingDelete!;
      expect(refusal.ids, [rusted]);
      expect(refusal.dependents.length, 2);
      expect(refusal.dependents.first.pointer, '/entities/e1/components/m/data/material');
      expect(refusal.dependents.first.document, '/fake/game/assets/scenes/main.scene.json');
      expect(model.entry(rusted), isNotNull, reason: 'nothing was touched');
      expect(model.lastError, isNull);
      model.cancelDelete();
      expect(model.pendingDelete, isNull);
      // Two selected, one referenced: the dialog lists that one's dependents.
      model.select(coinPrefab);
      model.select(swatch, toggle: true);
      expect(model.delete(), isFalse);
      expect(model.pendingDelete!.ids, unorderedEquals([coinPrefab, swatch]));
      expect(model.pendingDelete!.dependents.single.pointer, '/base_color_texture');
      expect(model.entry(coinPrefab), isNotNull, reason: 'the whole delete waits for the answer');
      expect(model.delete(ids: model.pendingDelete!.ids, force: true), isTrue);
      expect(model.entry(coinPrefab), isNull);
      expect(model.entry(swatch), isNull);
      expect(model.selection, isEmpty);
      expect(model.pendingDelete, isNull);
      expect(model.history.undo, ['Delete coin.prefab.json', 'Delete swatch.png']);
      model.undo();
      expect(model.entry(swatch), isNotNull);
      model.undo();
      expect(model.entry(coinPrefab), isNotNull);
      expect(model.history.canUndo, isFalse);
      expect(model.undo(), isNull);
      // Unreferenced: straight through.
      expect(model.delete(ids: [tone]), isTrue);
      expect(model.entry(tone), isNull);
    });

    test('import settings and reimport; a read-only project refuses every operation with the reason', () {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      expect(model.detail(swatch)!.settings, isEmpty);
      expect(model.setImportSettings(swatch, {'colorSpace': 'linear'}), isNotNull);
      expect(model.detail(swatch)!.settings, {'colorSpace': 'linear'});
      expect(model.history.undoLabel, 'Import settings of swatch.png');
      final ok = model.reimport(swatch)!;
      expect(ok.ok, isTrue);
      project.reimportRefusal = 'settings.colorSpace must be srgb or linear';
      final refused = model.reimport(swatch)!;
      expect(refused.ok, isFalse);
      expect(refused.diagnostics.single.message, contains('colorSpace'));
      expect(model.lastError, isNull, reason: 'the importer\'s answer is a result, not an error');
      expect(model.setImportSettings(rusted, {}), isNull);
      expect(model.lastError, contains('is a document'));

      final ro = ProjectBrowserModel(project: fakeProject(readOnly: 'lock from 9.1.0'));
      addTearDown(ro.dispose);
      expect(ro.canEdit, isFalse);
      expect(ro.newDocument('scene'), isNull);
      expect(ro.lastError, contains('read-only'));
      expect(ro.delete(ids: [tone]), isFalse);
      expect(ro.lastError, contains('read-only'));
    });

    test('a moved revision re-reads the entries and prunes the selection; the detail cache follows', () {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      var notified = 0;
      model.addListener(() => notified++);
      model.select(tone);
      expect(model.detail(tone)!.path, 'sounds/tone.wav');
      project.assetEntries.removeWhere((e) => e.id == tone);
      project.assetEntries.add(
        const AssetEntry(id: 'n1', path: 'sounds/new.wav', name: 'new', kind: 'source', importer: 'fynkel.audio'),
      );
      project.assetsRevision++;
      project.notifyListeners();
      expect(notified, greaterThanOrEqualTo(1));
      expect(model.entry(tone), isNull);
      expect(model.entry('n1'), isNotNull);
      expect(model.selection, isEmpty);
      expect(model.detail(tone), isNull);
      expect(model.detail('n1')!.path, 'sounds/new.wav');
    });
  });

  group('ProjectPanel', () {
    Future<(FakeProject, ProjectBrowserModel, List<String>, List<String>)> pumpPanel(WidgetTester tester) async {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      final opened = <String>[];
      final shown = <String>[];
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SizedBox(
              height: 600,
              child: ProjectPanel(
                model: model,
                onOpenDocument: opened.add,
                onShowAsset: () => shown.add(model.lead ?? ''),
                run: RecordingRunner().call,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      return (project, model, opened, shown);
    }

    Future<void> doubleTap(WidgetTester tester, Finder f) async {
      await tester.tap(f);
      await tester.pump(const Duration(milliseconds: 60));
      await tester.tap(f);
      await tester.pumpAndSettle();
    }

    testWidgets('tree, rows, the list view, search, status; double-click opens documents and shows sources', (
      tester,
    ) async {
      final (_, model, opened, shown) = await pumpPanel(tester);
      expect(find.byKey(const Key('project.folder.tree.root')), findsOneWidget);
      expect(find.byKey(const Key('project.folder.tree.scenes')), findsOneWidget);
      expect(find.byKey(const Key('project.folder.row.scenes')), findsOneWidget, reason: 'sub-folders are rows too');
      expect(find.byKey(const Key('project.asset.$loose')), findsOneWidget);
      expect(find.byKey(const Key('project.asset.$mainScene')), findsNothing);
      expect(find.byTooltip('1 of 7 assets in assets'), findsOneWidget);
      await tester.tap(find.byKey(const Key('project.folder.tree.scenes')));
      await tester.pumpAndSettle();
      expect(model.folder, 'scenes');
      expect(find.byKey(const Key('project.asset.$mainScene')), findsOneWidget);
      expect(find.byTooltip('1 of 7 assets in assets/scenes'), findsOneWidget);
      // The path header's `assets` goes back up.
      await tester.tap(find.byKey(const Key('project.path.0')));
      await tester.pumpAndSettle();
      expect(model.folder, '');
      await tester.tap(find.byKey(const Key('project.folder.row.models')));
      await tester.pumpAndSettle();
      expect(model.folder, 'models', reason: 'one click enters a folder');
      // A double-click on a source shows it in the Inspector; on a scene, opens the tab.
      await doubleTap(tester, find.byKey(const Key('project.asset.$robot')));
      expect(shown, [robot]);
      expect(opened, isEmpty);
      // The flat list shows every asset; the search narrows it.
      model.setView(BrowserView.list);
      await tester.pumpAndSettle();
      expect(model.view, BrowserView.list);
      expect(find.byKey(const Key('project.asset.$mainScene')), findsOneWidget);
      expect(find.byKey(const Key('project.asset.$tone')), findsOneWidget);
      await doubleTap(tester, find.byKey(const Key('project.asset.$mainScene')));
      expect(opened, [mainScene]);
      await tester.enterText(find.byKey(const Key('project.search')), 'coin');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('project.asset.$coinPrefab')), findsOneWidget);
      expect(find.byKey(const Key('project.asset.$tone')), findsNothing);
      expect(
        find.byWidgetPredicate((w) => w is Tooltip && (w.message ?? '').contains('1 of 7 assets in every folder')),
        findsOneWidget,
      );
      // Kind filter: textures only.
      await tester.enterText(find.byKey(const Key('project.search')), '');
      await tester.tap(find.byKey(const Key('project.filter')));
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.byKey(const Key('project.filter.texture')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('project.filter.texture')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('project.asset.$swatch')), findsOneWidget);
      expect(find.byKey(const Key('project.asset.$coinPrefab')), findsNothing);
      expect(find.byKey(const Key('project.noRows')), findsNothing);
    });

    testWidgets('New ▾ creates a selected, renaming row; ↩ commits the name; ⎋ cancels; the context menu renames too', (
      tester,
    ) async {
      final (project, model, _, _) = await pumpPanel(tester);
      await tester.tap(find.byKey(const Key('project.new')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('project.new.material')));
      await tester.pumpAndSettle();
      final created = model.renamingAsset!;
      expect(find.byKey(Key('project.rename.$created')), findsOneWidget);
      await tester.enterText(find.byKey(Key('project.rename.$created')), 'gold');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(model.entry(created)!.path, 'gold.material.json');
      expect(model.renamingAsset, isNull);
      expect(find.text('gold'), findsOneWidget, reason: 'the name as Unity shows it; the file on hover');
      // Context menu → Rename → ⎋ leaves the name.
      await tester.tap(find.byKey(Key('project.asset.$created')), buttons: kSecondaryButton);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Rename  ⇧⌥⌘R'));
      await tester.pumpAndSettle();
      expect(model.renamingAsset, created);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(model.renamingAsset, isNull);
      expect(model.entry(created)!.path, 'gold.material.json');
      // New Folder: a renaming folder row in the tree and in the list.
      await tester.tap(find.byKey(const Key('project.new')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('project.new.folder')));
      await tester.pumpAndSettle();
      expect(model.renamingFolder, 'New Folder');
      expect(find.byKey(const Key('project.renameFolder.New Folder')), findsWidgets);
      await tester.enterText(find.byKey(const Key('project.renameFolder.New Folder')).first, 'levels');
      await tester.testTextInput.receiveAction(TextInputAction.done);
      await tester.pumpAndSettle();
      expect(project.extraFolders, contains('New Folder'), reason: 'the fake\'s folder list is what it is');
      expect(model.renamingFolder, isNull);
      // Undo (Edit ▸ Undo Asset Operation) takes the material back.
      expect(model.history.canUndo, isTrue);
      model.undo();
      await tester.pumpAndSettle();
      expect(model.entry(created)!.path, 'New Material.material.json');
    });

    testWidgets('a Hierarchy object dropped on the list or a folder becomes a prefab there', (tester) async {
      final project = fakeProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      final made = <(List<String>, String)>[];
      model.cannotMakePrefab = (ids) => ids.contains('instance') ? 'a prefab instance already' : null;
      model.makePrefab = (ids, folder) async {
        made.add((ids, folder));
        return coinPrefab;
      };
      Widget source(String id) => Draggable<List<String>>(
        data: [id],
        feedback: const SizedBox(width: 10, height: 10),
        child: SizedBox(key: Key('source.$id'), width: 80, height: 20, child: Text(id)),
      );
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Column(
              children: [
                Row(children: [source('lamp'), source('instance')]),
                SizedBox(
                  height: 500,
                  child: ProjectPanel(model: model, onOpenDocument: (_) {}, onShowAsset: () {}, run: RecordingRunner().call),
                ),
              ],
            ),
          ),
        ),
      );
      model.setFolder('scenes');
      await tester.pumpAndSettle();
      Future<void> drag(String id, Finder to) async {
        final g = await tester.startGesture(tester.getCenter(find.byKey(Key('source.$id'))));
        await tester.pump(const Duration(milliseconds: 50));
        await g.moveTo(tester.getCenter(to));
        await tester.pump(const Duration(milliseconds: 50));
        await g.up();
        await tester.pumpAndSettle();
      }

      // Onto the list: the shown folder; the new prefab is selected and shown.
      await drag('lamp', find.byKey(const Key('project.asset.$mainScene')));
      expect(made.single.$1, ['lamp']);
      expect(made.single.$2, 'scenes');
      expect(model.lead, coinPrefab);
      expect(model.folder, 'prefabs');
      // Onto a folder in the tree: that folder.
      await drag('lamp', find.byKey(const Key('project.folder.tree.scenes')));
      expect(made.last.$2, 'scenes');
      // Refused: nothing is made.
      await drag('instance', find.byKey(const Key('project.folder.tree.scenes')));
      expect(made.length, 2);
    });

    testWidgets('a scene\'s context menu sets it as the startup scene, one asset-history step', (tester) async {
      final (project, model, _, _) = await pumpPanel(tester);
      model.setFolder('scenes');
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('project.asset.$mainScene')), buttons: kSecondaryButton);
      await tester.pumpAndSettle();
      await tester.tap(find.text('Set as Startup Scene'));
      await tester.pumpAndSettle();
      expect(project.startupScene, mainScene);
      expect(project.operations.last.$1, 'setStartupScene');
      expect(model.history.undoLabel, startsWith('Set Startup Scene'));
      // Again: refused, and the panel says why.
      model.setStartupScene(mainScene);
      await tester.pumpAndSettle();
      expect(model.lastError, contains('already the startup scene'));
      // Not offered for a prefab.
      model.setFolder('prefabs');
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('project.asset.$coinPrefab')), buttons: kSecondaryButton);
      await tester.pumpAndSettle();
      expect(find.text('Set as Startup Scene'), findsNothing);
    });

    testWidgets(
      'Delete on a referenced asset shows the dependents and "Delete anyway" forces it; the error bar shows refusals',
      (tester) async {
        final (_, model, _, _) = await pumpPanel(tester);
        model.setView(BrowserView.list);
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const Key('project.asset.$rusted')));
        await tester.pumpAndSettle();
        await tester.sendKeyEvent(LogicalKeyboardKey.backspace);
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('project.deleteRefused')), findsOneWidget);
        expect(find.text('scenes/main.scene.json'), findsOneWidget);
        expect(find.text('/entities/e1/components/m/data/material'), findsOneWidget);
        await tester.tap(find.text('Cancel'));
        await tester.pumpAndSettle();
        expect(model.entry(rusted), isNotNull);
        expect(model.pendingDelete, isNull);
        await tester.sendKeyEvent(LogicalKeyboardKey.backspace);
        await tester.pumpAndSettle();
        await tester.tap(find.byKey(const Key('project.deleteAnyway')));
        await tester.pumpAndSettle();
        expect(model.entry(rusted), isNull);
        expect(find.byKey(const Key('project.asset.$rusted')), findsNothing);
        // A refused rename (onto a taken name) shows in the error bar and is dismissable.
        model.move([loose], 'scenes');
        model.rename(mainScene, 'loose');
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('project.error')), findsOneWidget);
        await tester.tap(
          find.descendant(of: find.byKey(const Key('project.error')), matching: find.byIcon(FIcons.close)),
        );
        await tester.pumpAndSettle();
        expect(find.byKey(const Key('project.error')), findsNothing);
      },
    );

    testWidgets('the list has Name | Kind | Contents | Size columns; Name and Kind sort', (tester) async {
      final (_, model, _, _) = await pumpPanel(tester);
      expect(find.byKey(const Key('project.columns')), findsOneWidget);
      expect(find.text('Contents'), findsOneWidget);
      expect(tester.widget<Text>(find.byKey(const Key('project.contents.folder.models'))).data, '1 asset');
      model.setView(BrowserView.list);
      await tester.pumpAndSettle();
      expect(tester.widget<Text>(find.byKey(Key('project.contents.$robot'))).data, '3 parts');
      expect(find.byKey(Key('project.contents.$tone')), findsNothing, reason: 'a whole file has no parts to count');
      expect(find.byKey(Key('project.size.$tone')), findsOneWidget);
      await tester.tap(find.byKey(const Key('project.column.kind')));
      await tester.pumpAndSettle();
      expect(model.sort, AssetSort.kind);
      expect(model.descending, isFalse);
      await tester.tap(find.byKey(const Key('project.column.kind')));
      await tester.pumpAndSettle();
      expect(model.descending, isTrue, reason: 'again flips');
      await tester.tap(find.byKey(const Key('project.column.name')));
      await tester.pumpAndSettle();
      expect(model.sort, AssetSort.name);
      // A narrow list (the folder tree beside it) keeps Name and Kind only.
      model.setView(BrowserView.folders);
      tester.view.physicalSize = const Size(520, 600);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      await tester.pumpAndSettle();
      expect(find.text('Contents'), findsNothing);
      expect(find.text('Kind'), findsOneWidget);
    });

    testWidgets('dragging a row onto a folder in the tree moves it (the selection when the row is selected)', (
      tester,
    ) async {
      final (project, model, _, _) = await pumpPanel(tester);
      model.setView(BrowserView.list);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('project.asset.$tone')));
      await tester.pumpAndSettle();
      // Back to folders: the tree is there.
      model.setView(BrowserView.folders);
      await tester.pumpAndSettle();
      model.setFolder('sounds');
      await tester.pumpAndSettle();
      final from = tester.getCenter(find.byKey(const Key('project.asset.$tone')));
      final to = tester.getCenter(find.byKey(const Key('project.folder.tree.textures')));
      final g = await tester.startGesture(from);
      await tester.pump(const Duration(milliseconds: 50));
      await g.moveTo(to);
      await tester.pump(const Duration(milliseconds: 50));
      await g.up();
      await tester.pumpAndSettle();
      expect(model.entry(tone)!.path, 'textures/tone.wav');
      expect(project.operations.last.$1, 'renameAsset');
      expect(project.operations.last.$2, {'asset': tone, 'path': 'textures/tone.wav'});
    });
  });

  group('Inspector asset pages', () {
    Future<(FakeProject, ProjectBrowserModel, EditorSelection, List<String>, List<(String, String?)>)> pumpAsset(
      WidgetTester tester,
      String lead, {
      FakeProject? given,
    }) async {
      final project = given ?? fakeProject();
      final model = ProjectBrowserModel(project: project);
      final tabs = DocumentTabs(project: project);
      final inspector = InspectorModel(tabs: tabs, project: project);
      final selection = EditorSelection(inspector: inspector, browser: model);
      addTearDown(() {
        selection.dispose();
        inspector.dispose();
        tabs.dispose();
        model.dispose();
      });
      model.select(lead);
      final opened = <String>[];
      final refs = <(String, String?)>[];
      // Tall, so every section of the page's list is built.
      tester.view.physicalSize = const Size(600, 2200);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SizedBox(
              height: 2200,
              width: 500,
              child: InspectorPanel(
                model: inspector,
                selection: selection,
                browser: model,
                onOpenDocument: opened.add,
                onOpenReference: (d, ptr) => refs.add((d, ptr)),
                run: RecordingRunner().call,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      return (project, model, selection, opened, refs);
    }

    Future<void> chooseLinear(WidgetTester tester) async {
      await tester.tap(find.byKey(const Key('asset.settings.colorSpace')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('linear (data)').last);
      await tester.pumpAndSettle();
    }

    testWidgets('a source: import settings applied from the bar with a reimport, and what uses it', (tester) async {
      InspectorSection.setOpen('Info', null);
      addTearDown(() => InspectorSection.setOpen('Info', null));
      final (project, model, _, _, refs) = await pumpAsset(tester, swatch);
      expect(find.byKey(const Key('asset.title')), findsOneWidget);
      expect(find.text('swatch (Texture)'), findsOneWidget, reason: "Unity's `name (Kind)`");
      // The file's facts and its uses are folded under Info.
      expect(find.textContaining('fynkel.image'), findsNothing);
      await tester.tap(find.byKey(const Key('section.Info')));
      await tester.pumpAndSettle();
      expect(find.textContaining('fynkel.image'), findsOneWidget);
      expect(find.byKey(const Key('inspector.applyBar')), findsNothing, reason: 'nothing to apply yet');
      await chooseLinear(tester);
      expect(find.byKey(const Key('inspector.applyBar')), findsOneWidget);
      // Revert puts the sidecar's settings back and the bar goes away.
      await tester.tap(find.byKey(const Key('asset.revertSettings')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('inspector.applyBar')), findsNothing);
      expect(find.text('sRGB (colour)'), findsOneWidget);
      await chooseLinear(tester);
      await tester.tap(find.byKey(const Key('asset.applySettings')));
      await tester.pumpAndSettle();
      expect(project.settings[swatch], {'colorSpace': 'linear'});
      expect(project.operations.map((o) => o.$1).toList(), ['setImportSettings', 'reimport']);
      expect(find.byKey(const Key('asset.reimportResult')), findsOneWidget);
      expect(find.textContaining('Reimported'), findsOneWidget);
      expect(model.detail(swatch)!.settings, {'colorSpace': 'linear'});
      expect(find.byKey(const Key('inspector.applyBar')), findsNothing);
      // What uses it (an image has no parts of its own).
      expect(find.byKey(const Key('asset.sub.image/0')), findsNothing);
      expect(find.textContaining('Used by · 1'), findsOneWidget);
      expect(find.text('materials/rusted.material.json'), findsOneWidget);
      await tester.tap(find.byKey(const Key('asset.dependent.materials/rusted.material.json/base_color_texture')));
      await tester.pumpAndSettle();
      expect(refs, [('materials/rusted.material.json', '/base_color_texture')]);
      // A refused reimport shows the importer's diagnostics.
      project.reimportRefusal = 'settings.colorSpace must be srgb or linear';
      await tester.tap(find.byKey(const Key('asset.menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('asset.reimport')));
      await tester.pumpAndSettle();
      expect(find.textContaining('The importer refused'), findsOneWidget);
      expect(find.textContaining('must be srgb or linear'), findsOneWidget);
    });

    testWidgets('a model: materials remapped from the Materials section; Extract Materials is one step', (tester) async {
      final given = fakeProject();
      final at = given.assetEntries.indexWhere((e) => e.id == robot);
      given.assetEntries[at] = const AssetEntry(
        id: robot,
        path: 'models/robot.glb',
        name: 'robot',
        kind: 'source',
        importer: 'fynkel.gltf',
        subAssets: {'mesh/0': 'u-mesh', 'material/Painted': 'u-painted', 'material/Glass': 'u-glass', 'prefab': 'u-prefab'},
      );
      final (project, model, _, _, _) = await pumpAsset(tester, robot, given: given);
      expect(find.text('Materials'), findsOneWidget);
      expect(find.byKey(const Key('asset.materials.material/Painted')), findsOneWidget);
      expect(find.byKey(const Key('asset.materials.material/Glass')), findsOneWidget);
      expect(find.text('None (embedded)'), findsNWidgets(2));

      // Remap Painted to the project material through the picker: a draft
      // like any setting, applied with a reimport.
      await tester.tap(find.descendant(of: find.byKey(const Key('asset.materials.material/Painted')), matching: find.byIcon(FIcons.pick)));
      await tester.pumpAndSettle();
      await tester.tap(find.text('rusted').last);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('inspector.applyBar')), findsOneWidget);
      expect(
        tester.widget<PageAction>(find.byKey(const Key('asset.extractMaterials'))).onPressed,
        isNull,
        reason: 'not while the settings have unapplied changes',
      );
      await tester.tap(find.byKey(const Key('asset.applySettings')));
      await tester.pumpAndSettle();
      expect(project.settings[robot], {
        'materials': {
          'material/Painted': {'asset': rusted},
        },
      });

      // Extract writes the one left (Glass) and remaps it; one undo step.
      await tester.tap(find.byKey(const Key('asset.extractMaterials')));
      await tester.pumpAndSettle();
      expect(project.operations.map((o) => o.$1).toList(), ['setImportSettings', 'reimport', 'extractMaterials', 'reimport']);
      final remaps = project.settings[robot]!['materials']! as Map;
      expect(remaps.keys, ['material/Painted', 'material/Glass']);
      final glass = model.entries.singleWhere((e) => e.path == 'models/Materials/Glass.material.json');
      expect(remaps['material/Glass'], {'asset': glass.id});
      expect(find.text('Every material is remapped.'), findsOneWidget);
      expect(tester.widget<PageAction>(find.byKey(const Key('asset.extractMaterials'))).onPressed, isNull);
      expect(model.history.undoLabel, 'Extract Materials of robot.glb');
      model.undo();
      await tester.pumpAndSettle();
      expect(model.entries.any((e) => e.path == 'models/Materials/Glass.material.json'), isFalse);
    });

    testWidgets('settings JSON that does not parse keeps Apply off and says why', (tester) async {
      await pumpAsset(tester, swatch);
      await tester.tap(find.byKey(const Key('asset.settings.json')));
      await tester.pumpAndSettle();
      await tester.enterText(find.byKey(const Key('asset.settings.jsonField')), '{ nope');
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('inspector.applyBar')), findsOneWidget);
      expect(find.textContaining('does not parse'), findsWidgets);
      expect(tester.widget<FilledButton>(find.byKey(const Key('asset.applySettings'))).onPressed, isNull);
    });

    testWidgets('a material shows its swatch and factors; a scene has Open; a cleared selection empties the page', (
      tester,
    ) async {
      InspectorSection.setOpen('Info', true);
      addTearDown(() => InspectorSection.setOpen('Info', null));
      final (_, model, _, opened, _) = await pumpAsset(tester, rusted);
      expect(find.byKey(const Key('preview.swatch')), findsWidgets);
      expect(find.text('fynkel.pbr@1'), findsOneWidget);
      expect(find.text('base_color'), findsOneWidget);
      expect(find.text('[0.5,0.25,0.125,1.0]'), findsOneWidget);
      expect(find.textContaining('Used by · 2'), findsOneWidget);
      expect(find.byKey(const Key('asset.open')), findsNothing);
      model.select(mainScene);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('asset.open')), findsOneWidget);
      expect(find.byKey(const Key('asset.noDependents')), findsOneWidget);
      await tester.tap(find.byKey(const Key('asset.open')));
      expect(opened, [mainScene]);
      model.clearSelection();
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('inspector.empty')), findsOneWidget);
      model.select(robot);
      await tester.pumpAndSettle();
      expect(find.textContaining('Parts · 3'), findsOneWidget);
      expect(find.byKey(const Key('asset.sub.mesh/0')), findsOneWidget);
      expect(find.byKey(const Key('asset.sub.prefab')), findsOneWidget);
      expect(find.byKey(const Key('asset.settings.allowExternalFiles')), findsOneWidget);
    });

    testWidgets('a document kind registers its own sections; an unregistered kind shows the generic summary', (
      tester,
    ) async {
      addTearDown(() => AssetDocumentSections.unregister('scene'));
      final (_, model, _, _, _) = await pumpAsset(tester, mainScene);
      expect(find.textContaining('Scene'), findsWidgets, reason: 'the generic summary');
      expect(find.byKey(const Key('test.sceneSection')), findsNothing);
      // What a later data-asset kind does: one registration, no new panel.
      AssetDocumentSections.register(
        'scene',
        (context, e, d) => [
          InspectorSection(
            key: const Key('test.sceneSection'),
            title: 'Level',
            children: [InspectorValueRow('Entities', '${d.entities}')],
          ),
        ],
      );
      model.select(rusted);
      await tester.pumpAndSettle();
      model.select(mainScene);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('test.sceneSection')), findsOneWidget);
      expect(find.textContaining('Level'), findsOneWidget);
      expect(AssetDocumentSections.kinds, containsAll(['material', 'scene']));
      // The material's own sections are untouched.
      model.select(rusted);
      await tester.pumpAndSettle();
      expect(find.text('fynkel.pbr@1'), findsOneWidget);
    });

    testWidgets('a part opens its own page with a link back to its file; Open on an imported prefab', (tester) async {
      final (_, model, selection, opened, _) = await pumpAsset(tester, robot);
      await tester.tap(find.byKey(const Key('asset.part.mesh/0')));
      await tester.pumpAndSettle();
      expect(selection.target, const InspectAsset(robot, sub: 'mesh/0'));
      expect(find.byKey(const Key('asset.part.title')), findsOneWidget);
      expect(find.text('mesh · a part of robot.glb'), findsOneWidget);
      expect(find.byKey(const Key('asset.part.drag')), findsOneWidget);
      expect(find.byKey(const Key('asset.part.open')), findsNothing, reason: 'a mesh is not opened');
      await tester.tap(find.byKey(const Key('asset.part.back')));
      await tester.pumpAndSettle();
      expect(selection.target, const InspectAsset(robot));
      expect(find.byKey(const Key('asset.title')), findsOneWidget);
      await tester.tap(find.byKey(const Key('asset.part.prefab')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('asset.part.open')));
      expect(opened, ['$robot#prefab']);
      expect(model.lead, robot, reason: 'inspecting a part does not move the browser');
    });

    testWidgets('the padlock keeps the page while the browser selection moves; unlocking catches up', (tester) async {
      final (_, model, selection, _, _) = await pumpAsset(tester, rusted);
      await tester.tap(find.byKey(const Key('inspector.lock')));
      await tester.pumpAndSettle();
      expect(selection.locked, isTrue);
      model.select(swatch);
      await tester.pumpAndSettle();
      expect(selection.target, const InspectAsset(rusted));
      expect(find.text('fynkel.pbr@1'), findsOneWidget);
      await tester.tap(find.byKey(const Key('inspector.lock')));
      await tester.pumpAndSettle();
      expect(selection.locked, isFalse);
      expect(selection.target, const InspectAsset(swatch));
      expect(find.text('swatch (Texture)'), findsOneWidget);
    });

    testWidgets('a folder shows what is inside it by kind; Open goes there in the browser', (tester) async {
      final (_, model, selection, _, _) = await pumpAsset(tester, rusted);
      model.setFolder('materials');
      await tester.pumpAndSettle();
      expect(selection.target, const InspectFolder('materials'));
      expect(find.byKey(const Key('folder.title')), findsOneWidget);
      expect(find.byKey(const Key('folder.kind.material')), findsOneWidget);
      selection.inspect(const InspectFolder(''));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('folder.sub.models')), findsOneWidget);
      await tester.tap(find.byKey(const Key('folder.sub.models')));
      await tester.pumpAndSettle();
      expect(selection.target, const InspectFolder('models'));
      expect(model.folder, 'materials', reason: 'inspecting a folder does not move the browser');
      await tester.tap(find.byKey(const Key('folder.open')));
      await tester.pumpAndSettle();
      expect(model.folder, 'models');
    });
  });

  group('Hierarchy drop', () {
    testWidgets('a prefab dragged from the browser instantiates under the drop; other kinds are refused', (
      tester,
    ) async {
      final project = FakeProject(dir: '/fake/game');
      final doc = project.addDocument(
        FakeDocument(
          path: '/fake/game/assets/scenes/h.scene.json',
          id: 'h',
          kind: 'scene',
          name: 'H',
          relativePath: 'scenes/h.scene.json',
          entities: {
            'robot': FakeDocument.entity('Robot', children: ['arm']),
            'arm': FakeDocument.entity('Arm', parent: 'robot'),
            'crate': FakeDocument.entity(
              'Crate',
              prefab: {
                'asset': {'asset': 'prefab-crate'},
                'instance': 'i1',
                'overrides': <Object?>[],
              },
            ),
          },
        ),
      );
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final model = HierarchyModel(tabs: tabs, project: project);
      tabs.openDocument('h');
      const coin = AssetDragPayload(id: coinPrefab, kind: 'prefab', name: 'coin.prefab.json');
      const imported = AssetDragPayload(id: robot, sub: 'prefab', kind: 'prefab', name: 'robot.glb#prefab');
      const texture = AssetDragPayload(id: swatch, kind: 'texture', name: 'swatch.png');
      expect(model.canDropAsset(coin.toRef(), coin.kind, const DropTarget(parent: null, index: 3)), isTrue);
      expect(model.canDropAsset(texture.toRef(), texture.kind, const DropTarget(parent: null, index: 3)), isFalse);
      expect(
        model.canDropAsset(coin.toRef(), coin.kind, const DropTarget(parent: 'crate', index: 0)),
        isFalse,
        reason: 'not into a prefab instance',
      );

      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Row(
              children: [
                SizedBox(
                  width: 120,
                  child: Column(
                    children: [
                      Draggable<AssetDragPayload>(
                        key: const Key('drag.coin'),
                        data: coin,
                        dragAnchorStrategy: pointerDragAnchorStrategy, // as the browser's rows
                        feedback: const Text('coin'),
                        child: const SizedBox(width: 100, height: 30, child: Text('coin')),
                      ),
                      Draggable<AssetDragPayload>(
                        key: const Key('drag.robot'),
                        data: imported,
                        dragAnchorStrategy: pointerDragAnchorStrategy, // as the browser's rows
                        feedback: const Text('robot'),
                        child: const SizedBox(width: 100, height: 30, child: Text('robot#prefab')),
                      ),
                    ],
                  ),
                ),
                Expanded(
                  child: SizedBox(
                    height: 600,
                    child: HierarchyPanel(model: model, onOpenDocument: () {}),
                  ),
                ),
              ],
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      // Onto Robot's middle → an instance under Robot, selected.
      var g = await tester.startGesture(tester.getCenter(find.byKey(const Key('drag.coin'))));
      await tester.pump(const Duration(milliseconds: 50));
      await g.moveTo(tester.getCenter(find.byKey(const Key('hierarchy.name.robot'))));
      await tester.pump(const Duration(milliseconds: 50));
      await g.up();
      await tester.pumpAndSettle();
      final robotNode = model.tree!['robot']!;
      expect(robotNode.children.length, 2);
      final inst = robotNode.children.last;
      expect(model.tree![inst]!.isPrefabInstance, isTrue);
      expect(model.tree![inst]!.name, 'coin');
      expect(model.tree![inst]!.prefabAsset, coinPrefab);
      expect(model.selection, {inst});
      expect(doc.history, 1);
      // Onto the empty space below the rows → a root, named after the model.
      g = await tester.startGesture(tester.getCenter(find.byKey(const Key('drag.robot'))));
      await tester.pump(const Duration(milliseconds: 50));
      await g.moveTo(tester.getBottomLeft(find.byKey(const Key('hierarchy.list'))) + const Offset(40, -10));
      await tester.pump(const Duration(milliseconds: 50));
      await g.up();
      await tester.pumpAndSettle();
      expect(model.tree!.roots.length, 3);
      final root = model.tree!.roots.last;
      expect(model.tree![root]!.name, 'robot');
      expect(doc.entitiesView[root]!['prefab'], {
        'asset': {'asset': robot, 'sub': 'prefab'},
        'overrides': <Object?>[],
      });
      expect(doc.history, 2);
    });
  });

  group('shell', () {
    testWidgets('the Project panel is there; the asset commands run through the browser', (tester) async {
      final project = fakeProject();
      final prefs = Preferences.inMemory();
      await pumpLarge(tester, shellApp(prefs: prefs, projects: {'/fake/game': project}, initialProject: '/fake/game'));
      final shell = tester.state<EditorShellState>(find.byType(EditorShell));
      expect(shell.browser, isNotNull);
      expect(find.byKey(const Key('project.folder.tree.root')), findsOneWidget);
      expect(shell.workspace!.isVisible(PanelRegistry.inspector), isTrue);
      // New Scene from the menu: a renaming row in the root folder.
      expect(shell.registry.execute('assets.newScene'), isTrue);
      await tester.pumpAndSettle();
      final created = shell.browser!.renamingAsset!;
      expect(shell.browser!.entry(created)!.path, 'New Scene.scene.json');
      expect(find.byKey(Key('project.rename.$created')), findsOneWidget);
      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      // Undo Asset Operation is labelled after the history and takes it back.
      expect(shell.registry.byId('assets.undo')!.label, 'Undo New scene New Scene.scene.json');
      expect(shell.registry.execute('assets.undo'), isTrue);
      await tester.pumpAndSettle();
      expect(shell.browser!.entry(created), isNull);
      expect(shell.registry.byId('assets.redo')!.isEnabled, isTrue);
      // Open Asset on a source shows it in the Inspector; on a scene it opens the tab.
      shell.browser!.select(robot);
      expect(shell.registry.execute('assets.open'), isTrue);
      await tester.pumpAndSettle();
      expect(shell.selection!.target, const InspectAsset(robot));
      expect(find.byKey(const Key('asset.title')), findsOneWidget);
      shell.browser!.select(loose);
      project.addDocument(
        FakeDocument(
          path: '/fake/game/assets/loose.scene.json',
          id: loose,
          kind: 'scene',
          name: 'Loose',
          relativePath: 'loose.scene.json',
        ),
      );
      expect(shell.registry.execute('assets.open'), isTrue);
      await tester.pumpAndSettle();
      expect(shell.tabs!.focused?.document.id, loose);
      expect(shell.registry.execute('assets.toggleView'), isTrue);
      await tester.pumpAndSettle();
      expect(shell.browser!.view, BrowserView.list);
      expect(shell.registry.byId('assets.reimport')!.isEnabled, isFalse, reason: 'a scene is not reimported');
      shell.browser!.select(robot);
      expect(shell.registry.byId('assets.reimport')!.isEnabled, isTrue);
      // A dependent row opens the referencing scene and selects the entity.
      project.addDocument(
        FakeDocument(
          path: '/fake/game/assets/scenes/main.scene.json',
          id: mainScene,
          kind: 'scene',
          name: 'Main',
          relativePath: 'scenes/main.scene.json',
          entities: {'e1': FakeDocument.entity('One'), 'e2': FakeDocument.entity('Two')},
        ),
      );
      shell.openReference('scenes/main.scene.json', '/entities/e2/components/m/data/material');
      await tester.pumpAndSettle();
      expect(shell.tabs!.focused?.document.id, mainScene);
      expect(shell.hierarchy!.selection, {'e2'});
      // Closing the project drops the browser.
      await shell.closeProject();
      await tester.pumpAndSettle();
      expect(shell.browser, isNull);
    });
  });
}
