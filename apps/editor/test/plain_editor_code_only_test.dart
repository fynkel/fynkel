// ADR-0028 step 7: the plain editor — no game compiled in, no editor
// extensions — still edits every field. code_only's own project (a copy):
// every script on the level's objects and every data asset, each field
// the schemas expose drawn by a real editor (never the JSON-text fallback)
// and editable; a record's fields too (a coin kind's Sparks).
import 'dart:convert';
import 'dart:io';

import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/inspector/field_editor.dart';
import 'package:fynkel_editor/src/inspector/field_spec.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';

import 'support/fakes.dart';

final _codeOnly = Directory('../../examples/code_only').absolute;

/// The project's documents: assets, schemas and the project files.
Directory _copy() {
  final dir = Directory.systemTemp.createTempSync('fynkel_editor_plain_code_only');
  void copy(Directory from, Directory to) {
    to.createSync(recursive: true);
    for (final e in from.listSync()) {
      final name = e.uri.pathSegments.where((s) => s.isNotEmpty).last;
      if (name.startsWith('.')) continue;
      if (e is Directory) {
        copy(e, Directory('${to.path}/$name'));
      } else if (e is File) {
        e.copySync('${to.path}/$name');
      }
    }
  }

  for (final d in ['assets', 'schemas']) {
    copy(Directory('${_codeOnly.path}/$d'), Directory('${dir.path}/$d'));
  }
  for (final f in ['fynkel.project.json', 'fynkel.lock.json']) {
    File('${_codeOnly.path}/$f').copySync('${dir.path}/$f');
  }
  Directory('${dir.path}/lib/generated').createSync(recursive: true);
  File('${_codeOnly.path}/lib/generated/behaviours.g.json').copySync('${dir.path}/lib/generated/behaviours.g.json');
  return Directory(dir.resolveSymbolicLinksSync());
}

Map<String, Object?> _json(String path) => jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>;

/// A schema's exposed fields, and a record's own ones by field name.
(List<String>, Map<String, List<String>>) _fields(String path) {
  final props = _json(path)['properties'] as Map<String, Object?>;
  return (
    props.keys.toList(),
    {
      for (final MapEntry(:key, :value) in props.entries)
        if ((value as Map)['properties'] case final Map sub) key: [...sub.keys.cast<String>()],
    },
  );
}

void main() {
  testWidgets('the plain editor edits every field of code_only: scripts, data assets and records', (tester) async {
    final dir = _copy();
    final project = (await tester.runAsync(() => SessionProject.open(dir.path, watch: false)))!;
    addTearDown(() {
      project.close();
      dir.deleteSync(recursive: true);
    });
    await pumpLarge(
      tester,
      shellApp(prefs: Preferences.inMemory(), projects: {dir.path: project}, initialProject: dir.path),
    );
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    expect(shell.widget.game, isNull, reason: 'the plain editor: no game compiled in');

    /// The field editors shown for [key] (a script card's `type#i`, a data
    /// page's `data.<id>`), by path.
    Map<String, FieldEditor> shown(String key) => {
      for (final f in tester.widgetList<FieldEditor>(find.byType(FieldEditor)))
        if (f.target.key == key) f.path: f,
    };

    void expectEditable(Map<String, FieldEditor> editors, List<String> paths, String what) {
      for (final p in paths) {
        final f = editors[p];
        expect(f, isNotNull, reason: '$what: $p has an editor');
        expect(f!.spec.kind, isNot(FieldKind.any), reason: '$what: $p is not edited as JSON text');
        expect(f.spec.readOnly, isFalse, reason: '$what: $p');
        expect(f.target.canEdit, isTrue, reason: '$what: $p');
      }
    }

    // Every script on the level's objects (the coins' Collectible comes
    // from their prefab).
    final level = _json('${dir.path}/assets/scenes/level.scene.json');
    final entities = level['entities'] as Map<String, Object?>;
    String idOf(String name) => entities.entries.firstWhere((e) => (e.value as Map)['name'] == name).key;
    final scripts = {
      'Player': 'game.Player',
      'Gate': 'game.Gate',
      'Switch': 'game.Switch',
      'Coin 1': 'game.Collectible',
    };
    final doc = shell.openDocument(level['id'] as String)!;
    for (final MapEntry(key: name, value: type) in scripts.entries) {
      doc.setSelection([idOf(name)]);
      await tester.pumpAndSettle();
      final (fields, _) = _fields('${dir.path}/schemas/behaviours/$type.schema.json');
      expect(fields, isNotEmpty);
      expectEditable(shown('$type#0'), fields, '$name\'s $type');
    }

    // Every data asset, a record's fields included.
    for (final file in Directory('${dir.path}/assets/data').listSync().whereType<File>()) {
      if (!file.path.endsWith('.data.json')) continue;
      final data = _json(file.path);
      shell.browser!.select(data['id'] as String);
      await tester.pumpAndSettle();
      final (fields, records) = _fields('${dir.path}/schemas/data/${data['type']}.schema.json');
      final key = 'data.${data['id']}';
      expectEditable(shown(key), fields, data['name'] as String);
      // A record's foldout starts open: its fields, each by its own editor.
      for (final MapEntry(key: record, value: sub) in records.entries) {
        expect(find.byKey(ValueKey('foldout.$key.$record')), findsOneWidget);
        expectEditable(shown(key), [for (final f in sub) '/$record/$f'], '${data['name']}\'s $record');
      }
    }
  });
}
