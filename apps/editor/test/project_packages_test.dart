// A package's assets in the Project browser (ADR-0029 step 3): Unity's
// Packages root beside assets, the package's assets kept as they are
// (no rename, move, delete or New … there; read-only in the Inspector
// when pub keeps them), Copy into Project and Import Package Assets.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/inspector/asset_edits.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/project_browser/project_browser_model.dart';
import 'package:fynkel_editor/src/project_browser/project_panel.dart';

import 'support/fakes.dart';

const scene = 's1-0000-0000-0000-000000000001';
const glow = 'c0000000-0000-4000-8000-00000000abcd';
const spin = 'c0000000-0000-4000-8000-00000000abce';
const hosted = 'c0000000-0000-4000-8000-00000000abcf';

FakeProject packagedProject() {
  final p = FakeProject(dir: '/fake/game');
  p.assetEntries = [
    const AssetEntry(id: scene, path: 'scenes/main.scene.json', name: 'Main', kind: 'document', document: 'scene'),
    const AssetEntry(
      id: glow,
      path: 'Packages/pickups/materials/glow.material.json',
      name: 'glow',
      kind: 'document',
      document: 'material',
      package: 'pickups',
      file: '/fake/pickups/assets/materials/glow.material.json',
    ),
    const AssetEntry(
      id: spin,
      path: 'Packages/pickups/prefabs/spin.prefab.json',
      name: 'spin',
      kind: 'document',
      document: 'prefab',
      package: 'pickups',
      file: '/fake/pickups/assets/prefabs/spin.prefab.json',
    ),
    const AssetEntry(
      id: hosted,
      path: 'Packages/props/crate.material.json',
      name: 'crate',
      kind: 'document',
      document: 'material',
      package: 'props',
      file: '/pub-cache/props-1.0.0/assets/crate.material.json',
      readOnly: "belongs to the package 'props', which pub keeps: Copy into Project to change a copy",
    ),
  ];
  p.assetCount = p.assetEntries.length;
  return p;
}

void main() {
  group('ProjectBrowserModel', () {
    test('Packages is a root of its own; its folders come from the package assets', () {
      final model = ProjectBrowserModel(project: packagedProject());
      addTearDown(model.dispose);
      expect(
        model.folders,
        containsAll(['Packages', 'Packages/pickups', 'Packages/pickups/materials', 'Packages/props']),
      );
      expect(model.visibleFolders, ['scenes'], reason: 'the assets root does not list Packages');
      expect(model.folderCount(''), 1, reason: 'the root counts the project\'s own');
      expect(model.folderCount('Packages/pickups'), 2);
      model.setFolder('Packages');
      expect(model.visibleFolders, ['Packages/pickups', 'Packages/props']);
      model.setFolder('Packages/pickups/materials');
      expect(model.visibleAssets.map((e) => e.id), [glow]);
      expect(ProjectBrowserModel.packageOfFolder('Packages/pickups/materials'), 'pickups');
      expect(ProjectBrowserModel.packageOfFolder('Packages'), isNull);
    });

    test('a package\'s assets are not renamed, moved, deleted or duplicated; nothing is made in Packages', () {
      final project = packagedProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      final e = model.entry(glow)!;
      expect(model.canEditEntry(e), isFalse);
      expect(model.canEditEntry(model.entry(scene)!), isTrue);
      model.beginRename(glow);
      expect(model.renamingAsset, isNull);
      expect(model.canMove([glow], 'scenes'), isFalse);
      expect(model.canMove([scene], 'Packages/pickups'), isFalse);
      expect(model.move([glow], 'scenes'), 0);
      model.select(glow);
      model.requestDelete();
      expect(model.pendingConfirm, isNull);
      expect(model.lastError, contains('cannot be deleted'));
      expect(model.duplicate(), isFalse);
      model.setFolder('Packages/pickups');
      expect(model.canCreateHere, isFalse);
      expect(model.newDocument('material'), isNull);
      expect(model.lastError, contains('belongs to a package'));
      expect(model.newFolder(), isFalse);
      expect(model.renameFolder('Packages/pickups', 'other'), isFalse);
      expect(model.deleteFolder('Packages/pickups/materials'), isFalse);
      expect(project.assetEntries, hasLength(4), reason: 'nothing changed');
    });

    test('Copy into Project makes a fresh-id copy under <package>/, selected; undo takes it back', () {
      final project = packagedProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      model.select(glow);
      expect(model.canCopyIntoProject, isTrue);
      expect(model.copyIntoProject(), isTrue);
      final copy = model.entry(model.lead!)!;
      expect(copy.id, isNot(glow));
      expect(copy.path, 'pickups/materials/glow.material.json');
      expect(copy.inPackage, isFalse);
      expect(model.folder, 'pickups/materials');
      expect(model.entry(glow), isNotNull, reason: 'the package\'s own stays');
      expect(model.history.undoLabel, 'Copy glow.material.json into Project');
      model.undo();
      expect(model.entry(copy.id), isNull);
    });

    test('Import Package Assets copies every asset with its id, shadowing the package\'s; undo gives them back', () {
      final project = packagedProject();
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      expect(model.importPackageAssets('pickups'), isTrue);
      expect(model.entry(glow)!.path, 'pickups/materials/glow.material.json');
      expect(model.entry(spin)!.inPackage, isFalse);
      expect(model.folder, 'pickups');
      expect(model.folders.where((f) => f.startsWith('Packages/pickups')), isEmpty);
      expect(model.history.undoLabel, 'Import pickups Assets');
      model.undo();
      expect(model.entry(glow)!.inPackage, isTrue);
    });
  });

  test('AssetEdits: a package asset pub keeps is read-only; a path package\'s is not', () {
    final edits = AssetEdits(packagedProject());
    addTearDown(edits.dispose);
    expect(edits.canEditAsset(glow), isTrue);
    expect(edits.canEditAsset(hosted), isFalse);
    expect(edits.readOnlyReason(hosted), contains('Copy into Project'));
    expect(edits.set(hosted, {'metallic': 1.0}), isFalse);
  });

  group('ProjectPanel', () {
    Future<ProjectBrowserModel> pumpPanel(WidgetTester tester, FakeProject project) async {
      final model = ProjectBrowserModel(project: project);
      addTearDown(model.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SizedBox(
              height: 600,
              child: ProjectPanel(
                model: model,
                onOpenDocument: (_) {},
                onShowAsset: () {},
                run: RecordingRunner().call,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      return model;
    }

    testWidgets('the tree shows Packages after assets; the menus offer Copy into Project and Import', (tester) async {
      final model = await pumpPanel(tester, packagedProject());
      final root = find.byKey(const Key('project.folder.tree.Packages'));
      expect(root, findsOneWidget);
      expect(find.byKey(const Key('project.folder.tree.Packages/pickups')), findsOneWidget);
      expect(find.byKey(const Key('project.folder.row.Packages')), findsNothing, reason: 'not a folder of assets');
      expect(
        tester.getTopLeft(root).dy,
        greaterThan(tester.getTopLeft(find.byKey(const Key('project.folder.tree.scenes'))).dy),
      );
      await tester.tap(find.byKey(const Key('project.folder.tree.Packages/pickups/materials')));
      await tester.pumpAndSettle();
      expect(find.text('1 of 4 assets in Packages/pickups/materials'), findsOneWidget);
      // The asset's menu: Copy into Project, Rename off.
      await tester.tap(find.byKey(const Key('project.asset.$glow')), buttons: 2);
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('project.menu.copyIntoProject')), findsOneWidget);
      final rename = tester.widget<PopupMenuItem<String>>(
        find.ancestor(of: find.text('Rename  ⇧⌥⌘R'), matching: find.byType(PopupMenuItem<String>)),
      );
      expect(rename.enabled, isFalse);
      await tester.tap(find.byKey(const Key('project.menu.copyIntoProject')));
      await tester.pumpAndSettle();
      expect(model.entry(model.lead!)!.path, 'pickups/materials/glow.material.json');
      // The package folder's menu: Import, after a question.
      await tester.tap(find.byKey(const Key('project.folder.tree.Packages/pickups')), buttons: 2);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('project.menu.importPackageAssets')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('project.importPackageConfirm')), findsOneWidget);
      await tester.tap(find.byKey(const Key('project.importPackageConfirm.yes')));
      await tester.pumpAndSettle();
      expect(model.lastError, contains('holds files already'), reason: 'the copy is there already');
    });
  });
}
