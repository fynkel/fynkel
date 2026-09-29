// Project › Persistent Objects… (ADR-0039): rows of a name and a prefab;
// a prefab picked into a new row names it; a name that is not an
// identifier or is used twice, or a row without a prefab, keeps Apply off;
// Apply writes the list in order as one step, and undo takes it back.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/inspector/field_widgets.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/project/persistent_objects_dialog.dart';

import 'inspector_test.dart' show rig;

void main() {
  test('a name is made from a prefab name', () {
    expect(PersistentObjectEntry.nameFrom('Game Manager'), 'GameManager');
    expect(PersistentObjectEntry.nameFrom('2 Music'), 'Object2Music');
    expect(PersistentObjectEntry.nameFrom('—'), 'Object');
  });

  testWidgets('rows are added, named from their prefab, checked, reordered, applied and undone', (tester) async {
    final r = rig();
    r.project.assetEntries = [
      const AssetEntry(
        id: 'pm',
        path: 'prefabs/game manager.prefab.json',
        name: 'Game Manager',
        kind: 'document',
        document: 'prefab',
      ),
      const AssetEntry(id: 'm', path: 'rust.material.json', name: 'Rust', kind: 'document', document: 'material'),
    ];
    final file = File('${r.project.dir}/fynkel.project.json')..writeAsStringSync(jsonEncode({'name': 'x'}));
    tester.view.physicalSize = const Size(1200, 1000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Builder(
            builder: (context) =>
                TextButton(onPressed: () => showPersistentObjectsDialog(context, r.project), child: const Text('open')),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    FilledButton apply() => tester.widget<FilledButton>(find.byKey(const Key('persistentObjects.apply')));
    expect(apply().onPressed, isNull, reason: 'nothing changed');
    expect(find.byKey(const Key('persistentObjects.empty')), findsOneWidget);

    Future<void> pick(int row, String id) async {
      tester.widget<AssetField>(find.byKey(Key('persistentObjects.prefab.$row'))).onCommit({'asset': id});
      await tester.pump();
    }

    Future<void> name(int row, String text) async {
      await tester.enterText(find.byKey(Key('persistentObjects.name.$row')), text);
      await tester.pump();
    }

    await tester.tap(find.byKey(const Key('persistentObjects.add')));
    await tester.pump();
    expect(apply().onPressed, isNull, reason: 'the row has no prefab');
    // Only prefabs are offered.
    final field = tester.widget<AssetField>(find.byKey(const Key('persistentObjects.prefab.0')));
    expect(field.candidates().map((a) => a.id), ['pm']);
    await pick(0, 'pm');
    expect(tester.widget<TextField>(find.byKey(const Key('persistentObjects.name.0'))).controller!.text, 'GameManager');
    expect(apply().onPressed, isNotNull);

    // A second row from the same prefab gets a name of its own.
    await tester.tap(find.byKey(const Key('persistentObjects.add')));
    await tester.pump();
    await pick(1, 'pm');
    expect(
      tester.widget<TextField>(find.byKey(const Key('persistentObjects.name.1'))).controller!.text,
      'GameManager2',
    );

    await name(1, 'GameManager');
    expect(find.textContaining('used twice'), findsOneWidget);
    expect(apply().onPressed, isNull);
    await name(1, 'music player');
    expect(find.textContaining('letter'), findsOneWidget);
    expect(apply().onPressed, isNull);
    await name(1, 'Music');
    expect(find.byKey(const Key('persistentObjects.error')), findsNothing);

    await tester.tap(find.byKey(const Key('persistentObjects.up.1')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('persistentObjects.apply')));
    await tester.pumpAndSettle();
    expect(find.byType(PersistentObjectsDialog), findsNothing);
    expect(jsonDecode(file.readAsStringSync())['persistentObjects'], [
      {
        'name': 'Music',
        'prefab': {'asset': 'pm'},
      },
      {
        'name': 'GameManager',
        'prefab': {'asset': 'pm'},
      },
    ]);
    expect(r.project.persistentObjects().map((o) => o.name), ['Music', 'GameManager']);

    // Opened again, a row removed.
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('persistentObjects.remove.0')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('persistentObjects.apply')));
    await tester.pumpAndSettle();
    expect(r.project.persistentObjects().map((o) => o.name), ['GameManager']);

    r.project.undoAsset();
    r.project.undoAsset();
    expect(r.project.persistentObjects(), isEmpty);
  });
}
