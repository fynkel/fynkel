// Tags and Layers (ADR-0020, L4c): the list drafts (what `setLayersAndTags`
// is given), the dialog opened from the Inspector header's "Edit Layers…"
// over the fakes, and the real session rewriting an open scene when a
// layer moves.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/inspector/inspector_model.dart';
import 'package:fynkel_editor/src/inspector/inspector_panel.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor/src/project/tags_and_layers_dialog.dart';

import 'documents_session_test.dart' show mainScene, crateB;
import 'inspector_test.dart' show rig;
import 'project_test.dart' show copyFixture;

void main() {
  group('ListDraft', () {
    test('renames and additions line up by index; moves and removals rewrite', () {
      final layers = ListDraft(['default', 'ground', 'props'], noun: 'layer', fixedFirst: true);
      expect(layers.changed, isFalse);
      layers.rename(1, 'floor');
      final added = layers.add();
      expect(added.name, 'Layer 3');
      expect(layers.names, ['default', 'floor', 'props', 'Layer 3']);
      expect(layers.from, [0, 1, 2, null]);
      expect(layers.changed, isTrue);
      expect(layers.rewrites, isFalse, reason: 'a rename and an addition keep every index');

      layers.move(3, -1);
      expect(layers.names, ['default', 'floor', 'Layer 3', 'props']);
      expect(layers.from, [0, 1, null, 2]);
      expect(layers.rewrites, isTrue);
      layers.remove(1);
      expect(layers.removed, ['ground']);
      expect(layers.from, [0, null, 2]);
    });

    test('default stays first; names must be unique and present', () {
      final layers = ListDraft(['default', 'props'], noun: 'layer', fixedFirst: true);
      expect(layers.canMove(0, 1), isFalse);
      expect(layers.canMove(1, -1), isFalse, reason: 'nothing moves above default');
      layers.remove(0);
      expect(layers.names, ['default', 'props']);
      layers.rename(1, 'default');
      expect(layers.error, '"default" is there twice');
      layers.rename(1, '  ');
      expect(layers.error, 'Layer 1 has no name');

      final tags = ListDraft(List.generate(32, (i) => 't$i'), noun: 'tag');
      expect(tags.canAdd, isFalse);
      tags.move(0, 1);
      expect(tags.names.take(2), ['t1', 't0']);
      expect(tags.rewrites, isTrue);
    });
  });

  testWidgets('Edit Layers… in the header opens the dialog; Apply sends the lists and where each entry came from', (
    tester,
  ) async {
    final r = rig();
    File('${r.project.dir}/fynkel.project.json').writeAsStringSync(
      jsonEncode({
        'name': 'x',
        'layers': ['default', 'player', 'props'],
        'tags': ['pickup'],
      }),
    );
    r.tabs.openDocument('main')!.select('a');
    tester.view.physicalSize = const Size(1200, 1200);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Align(
            alignment: Alignment.topLeft,
            child: SizedBox(width: 420, height: 1200, child: InspectorPanel(model: r.model)),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('layer.summary')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('layer.edit')));
    await tester.pumpAndSettle();
    expect(find.text('Tags and Layers'), findsOneWidget);
    FilledButton apply() => tester.widget<FilledButton>(find.byKey(const Key('tagsLayers.apply')));
    expect(apply().onPressed, isNull, reason: 'nothing changed yet');

    // default cannot be renamed, moved or removed.
    expect(tester.widget<TextField>(find.byKey(const Key('layers.name.0'))).enabled, isFalse);
    expect(
      tester
          .widget<IconButton>(
            find.descendant(of: find.byKey(const Key('layers.remove.0')), matching: find.byType(IconButton)),
          )
          .onPressed,
      isNull,
    );

    // Rename a layer and add a tag: no rewrite warning.
    await tester.enterText(find.byKey(const Key('layers.name.1')), 'hero');
    await tester.tap(find.byKey(const Key('tags.add')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('tags.name.1')), 'enemy');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('tagsLayers.rewrites')), findsNothing);

    // A duplicate name blocks Apply.
    await tester.enterText(find.byKey(const Key('tags.name.1')), 'pickup');
    await tester.pumpAndSettle();
    expect(find.text('"pickup" is there twice'), findsOneWidget);
    expect(apply().onPressed, isNull);
    await tester.enterText(find.byKey(const Key('tags.name.1')), 'enemy');
    await tester.pumpAndSettle();

    // Moving props up says the documents are rewritten.
    await tester.tap(find.byKey(const Key('layers.up.2')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('tagsLayers.rewrites')), findsOneWidget);

    await tester.tap(find.byKey(const Key('tagsLayers.apply')));
    await tester.pumpAndSettle();
    expect(find.text('Tags and Layers'), findsNothing);
    final (kind, args) = r.project.operations.last;
    expect(kind, 'setLayersAndTags');
    expect(args, {
      'layers': ['default', 'props', 'hero'],
      'tags': ['pickup', 'enemy'],
      'layersFrom': [0, 2, 1],
      'tagsFrom': [0, null],
    });
    // The header reads the new lists.
    expect(r.model.layerNames, ['default', 'props', 'hero']);
    await tester.tap(find.byKey(const ValueKey('tag.add')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('tag.1')), findsOneWidget);
    expect(find.byKey(const ValueKey('tag.edit')), findsOneWidget);
  });

  test('the real session: a layer moved rewrites and re-reads the open scene; the asset undo puts it back', () async {
    final dir = copyFixture('tags_layers');
    addTearDown(() => dir.deleteSync(recursive: true));
    final project = await SessionProject.open(dir.path, watch: false);
    addTearDown(project.close);
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final model = InspectorModel(tabs: tabs, project: project);
    addTearDown(model.dispose);
    final tab = tabs.openDocument(mainScene)!;
    tab.select(crateB);
    expect(model.layer, 1);

    // A layer inserted before props: Crate B follows it to index 2.
    final r = project.setLayersAndTags(
      layers: ['default', 'ghosts', 'props'],
      layersFrom: [0, null, 1],
      tags: ['pickup'],
      tagsFrom: [null],
    );
    expect(r.reload, isNotEmpty);
    expect(model.layerNames, ['default', 'ghosts', 'props']);
    expect(model.tagNames, ['pickup']);
    expect(model.layer, 2);
    expect(tab.document.dirty, isFalse);
    expect(project.assetHistory().undoLabel, 'Edit Tags and Layers');

    project.undoAsset();
    expect(model.layerNames, ['default', 'props']);
    expect(model.layer, 1);

    // An unsaved edit protects the scene: refused, nothing written.
    expect(model.setLayer(0), isTrue);
    final file = File('${dir.path}/fynkel.project.json').readAsStringSync();
    expect(
      () => project.setLayersAndTags(layers: ['default', 'ghosts', 'props'], layersFrom: [0, null, 1], tags: []),
      throwsA(isA<AuthoringException>().having((e) => e.code, 'code', 'FYN-T0003')),
    );
    expect(File('${dir.path}/fynkel.project.json').readAsStringSync(), file);
  });
}
