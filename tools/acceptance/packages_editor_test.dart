// ADR-0029 step 7, the editor half: code_only's project editor with the
// pickups package's real editor part compiled in (`fynkel editor
// --write-host`; tools/package_acceptance.py copies this file into the
// host's test/ and runs it there, once per way of depending on pickups).
//
//   FYNKEL_ACCEPTANCE_PROJECT  the code_only copy
//   FYNKEL_ACCEPTANCE_MODE     `path` (pickups next to it: its assets edit
//                              in place) or `git` (pub keeps it: read-only,
//                              Copy into Project makes the game's own copy)
//
// ignore_for_file: implementation_imports, depend_on_referenced_packages
import 'dart:convert';
import 'dart:io';

import 'package:code_only/fynkel_game.dart' as game;
import 'package:code_only/generated/packages.g.dart' as behaviours;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor/src/play/editor_game.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';
import 'package:pickups/editor/editor.dart' as pickups;

const gemKind = '33ab781f-1057-5547-b2e4-3c17aa0896e6';
const gemMaterial = 'cdc6e6dc-9175-5460-8e3f-af15fac42617';
const pickupPrefab = '0dd85a60-b1ab-52b3-999e-09eb00a048f4';

Future<Process> _noProcess(
  String executable,
  List<String> arguments, {
  String? workingDirectory,
  Map<String, String>? environment,
}) async => throw StateError('the acceptance starts no $executable');

Future<ProcessResult> _noRun(String executable, List<String> args, {String? workingDirectory}) async =>
    ProcessResult(0, 0, '', '');

Map<String, Object?> _json(String path) => jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>;

void main() {
  final dir = Platform.environment['FYNKEL_ACCEPTANCE_PROJECT'];
  final mode = Platform.environment['FYNKEL_ACCEPTANCE_MODE'];

  testWidgets('code_only with the pickups package ($mode): its prefab placed, its Inspector shown, its assets '
      '${mode == 'git' ? 'read-only and copied into the project' : 'edited in place'}', (tester) async {
    expect(dir, isNotNull, reason: 'run through tools/package_acceptance.py');
    expect(mode, anyOf('path', 'git'));
    final project = (await tester.runAsync(() => SessionProject.open(dir!, watch: false)))!;
    addTearDown(project.close);

    tester.view.physicalSize = const Size(1600, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      FynkelEditorApp(
        prefs: Preferences.inMemory(),
        layoutStore: MemoryLayoutStore(),
        openProject: (_) async => project,
        run: _noRun,
        startPlayProcess: _noProcess,
        startCodegen: _noProcess,
        sceneViewport: false,
        nativeMenuBar: false,
        quit: () {},
        initialProject: dir,
        startEditor: null,
        game: EditorGame(
          name: 'code_only',
          projectDir: dir!,
          build: game.buildGame,
          registerBehaviours: behaviours.registerPackages,
        ),
        extensions: const [EditorPart('pickups', pickups.registerEditor)],
      ),
    );
    await tester.pumpAndSettle();
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    expect(shell.extensions.log.where((d) => d.message.contains('pickups')), isEmpty, reason: '${shell.extensions.log}');

    // The package's assets under Packages/pickups, where pub put them.
    final packaged = {
      for (final a in project.assets())
        if (a.package == 'pickups') a.id: a,
    };
    expect(packaged.keys, containsAll([gemKind, gemMaterial, pickupPrefab]));
    expect(packaged[pickupPrefab]!.path, 'Packages/pickups/prefabs/pickup.prefab.json');
    final materialFile = packaged[gemMaterial]!.file!;
    if (mode == 'path') {
      expect(packaged.values.every((a) => a.readOnly == null), isTrue, reason: 'a path package is the user\'s');
    } else {
      expect(materialFile, contains('/git/'), reason: 'a git dependency lives in pub\'s cache');
      for (final a in packaged.values) {
        expect(a.readOnly, contains('Copy into Project'), reason: a.path);
      }
    }

    // Its prefab placed in the level through its own menu item.
    final level = project.assets().firstWhere((a) => a.path == 'scenes/level.scene.json');
    final doc = shell.openDocument(level.id)!;
    await tester.pumpAndSettle();
    final before = doc.tree.nodes.length;
    expect(shell.registry.execute('pickups.addPickup'), isTrue);
    await tester.pumpAndSettle();
    expect(doc.tree.nodes.length, greaterThan(before));
    final added = doc.tree[doc.selection.single]!;
    expect(added.name, 'Pickup');
    expect(added.isPrefabInstance, isTrue);
    expect(added.prefabAsset, pickupPrefab);
    expect(doc.save(), isTrue);
    final saved = jsonEncode(_json('${dir}/assets/scenes/level.scene.json'));
    expect(saved, contains(pickupPrefab));

    // Its Inspector: a pickup kind's page shows the package's gem preview.
    shell.browser!.select(gemKind);
    await tester.pumpAndSettle();
    expect(find.byType(pickups.GemPreview), findsOneWidget);

    final edits = shell.assetEdits!;
    final original = File(materialFile).readAsStringSync();
    if (mode == 'path') {
      // Edited in place: the package's own file changes.
      expect(edits.canEditAsset(gemMaterial), isTrue);
      expect(edits.set(gemMaterial, {'metallic': 0.75}), isTrue);
      await tester.pumpAndSettle();
      expect(File(materialFile).readAsStringSync(), isNot(original));
      expect(File(materialFile).readAsStringSync(), contains('0.75'));
    } else {
      // Read-only where pub keeps it; Copy into Project gives the game its
      // own copy (a fresh id) that edits like any asset.
      expect(edits.canEditAsset(gemMaterial), isFalse);
      expect(edits.readOnlyReason(gemMaterial), contains('Copy into Project'));
      expect(edits.set(gemMaterial, {'metallic': 0.75}), isFalse);
      expect(File(materialFile).readAsStringSync(), original);

      final browser = shell.browser!..select(gemMaterial);
      await tester.pumpAndSettle();
      expect(browser.canCopyIntoProject, isTrue);
      expect(browser.copyIntoProject(), isTrue);
      await tester.pumpAndSettle();
      final copyPath = '$dir/assets/pickups/materials/gem.material.json';
      final copyId = _json(copyPath)['id'] as String;
      expect(copyId, isNot(gemMaterial));
      expect(browser.selection, [copyId], reason: 'the copy is selected');
      expect(edits.canEditAsset(copyId), isTrue);
      expect(edits.set(copyId, {'metallic': 0.75}), isTrue);
      await tester.pumpAndSettle();
      expect(File(copyPath).readAsStringSync(), contains('0.75'));
      expect(File(materialFile).readAsStringSync(), original, reason: 'the package itself is untouched');
    }
  });
}
