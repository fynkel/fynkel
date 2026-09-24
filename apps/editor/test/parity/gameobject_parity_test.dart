// Unity parity: GameObject → 3D Object / Light / Camera (ADR-0023),
// driven through the whole shell over a real project, and the saved scene
// rendered by the real `fynkel render` to see that the cube is there
// (docs/unity-parity.md § Hierarchy).
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/hierarchy/presets.dart';
import 'package:fynkel_editor/src/scene_view/png.dart';

import 'harness.dart';

/// `fynkel render` of the main scene: the summary line and the pixels.
Future<(String, Uint8List)> _render(Editor e, String name) async {
  final cli = Platform.environment['FYNKEL_CLI'] ?? 'fynkel';
  final out = '${e.dir.path}/$name.png';
  final r = (await e.tester.runAsync(
    () =>
        Process.run(cli, ['render', '--project', e.dir.path, '--scene', mainScene, '--out', out, '--size', '320x180']),
  ))!;
  expect(r.exitCode, 0, reason: '${r.stdout}\n${r.stderr}');
  final (_, _, rgba) = decodePng(File(out).readAsBytesSync());
  return ('${r.stdout}', rgba);
}

/// The saved entity named [name] and its components by type.
(Map<String, dynamic>, Map<String, Map<String, dynamic>>) _saved(Editor e, String name) {
  final entities = (e.savedScene()['entities'] as Map<String, dynamic>).values.cast<Map<String, dynamic>>();
  final entity = entities.singleWhere((x) => x['name'] == name);
  final components = <String, Map<String, dynamic>>{
    for (final c in (entity['components'] as Map<String, dynamic>).values.cast<Map<String, dynamic>>())
      c['type'] as String: c['data'] as Map<String, dynamic>,
  };
  return (entity, components);
}

void main() {
  testWidgets('GameObject → 3D Object → Cube makes a cube you can see, selected and renaming; ⌘Z removes it', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    final (before, beforePixels) = await _render(e, 'before');
    expect(before, contains('2 instances'));

    await e.click(find.byKey(const Key('menu.gameObject')));
    await e.click(find.byKey(const Key('submenu.gameObject.3D Object')));
    await e.click(find.byKey(const Key('menuitem.gameobject.cube')));

    // Selected, its name field open on "Cube", as Unity.
    final tab = e.shell.tabs!.focused!;
    expect(e.selectedNames, ['Cube']);
    final id = tab.selection.single;
    expect(tab.tree[id]!.parent, isNull);
    final field = tester.widget<EditableText>(
      find.descendant(of: find.byKey(Key('hierarchy.rename.$id')), matching: find.byType(EditableText)),
    );
    expect(field.controller.text, 'Cube');
    await e.submit();
    expect(e.rows.last, 'Cube');

    // The shapes came into the project, and the Project panel lists them.
    expect(File('${e.dir.path}/assets/Fynkel/Primitives.glb').existsSync(), isTrue);
    expect(File('${e.dir.path}/assets/Fynkel/Primitives.glb.meta.json').existsSync(), isTrue);

    await e.press(LogicalKeyboardKey.keyS, command: true);
    final (_, components) = _saved(e, 'Cube');
    expect(components['fynkel.MeshRenderer']!['mesh'], {'asset': primitivesId, 'sub': 'mesh/Cube'});
    expect(components['fynkel.MeshRenderer']!['material'], {'asset': primitivesId, 'sub': 'material/Default'});
    expect(components['fynkel.Collider']!['shape'], 'box');
    expect(components['fynkel.Collider']!['size'], [0.5, 0.5, 0.5]);

    // Rendered: one more instance, nothing missing, and the picture changed.
    final (after, afterPixels) = await _render(e, 'after');
    expect(after, contains('3 instances'));
    expect(after, contains('0 placeholders, 0 missing'));
    var changed = 0;
    for (var i = 0; i < afterPixels.length; i += 4) {
      final d =
          (afterPixels[i] - beforePixels[i]).abs() +
          (afterPixels[i + 1] - beforePixels[i + 1]).abs() +
          (afterPixels[i + 2] - beforePixels[i + 2]).abs();
      if (d > 24) changed++;
    }
    expect(changed, greaterThan(320 * 180 ~/ 200), reason: 'the cube covers part of the picture');

    // One ⌘Z removes the object; the model stays (the Project's own history).
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(e.rows, isNot(contains('Cube')));
    expect(File('${e.dir.path}/assets/Fynkel/Primitives.glb').existsSync(), isTrue);
  });

  testWidgets('the + menu makes a Directional Light and a Camera; a row\'s menu puts a Sphere under it', (
    tester,
  ) async {
    final e = await Editor.open(tester);
    await e.click(find.byKey(const Key('hierarchy.create')));
    await e.click(find.byKey(const Key('hierarchy.create.light')));
    await e.click(find.byKey(const Key('hierarchy.create.directionalLight')));
    await e.submit();
    await e.click(find.byKey(const Key('hierarchy.create')));
    await e.click(find.byKey(const Key('hierarchy.create.camera')));
    await e.submit();
    // + puts it under the selection, as Unity's Hierarchy does.
    expect(e.rows, containsAllInOrder(['Directional Light', 'Camera']));

    await tester.tapAt(tester.getCenter(e.row('Robot')), buttons: kSecondaryMouseButton);
    await tester.pumpAndSettle();
    await e.click(find.byKey(const Key('hierarchy.menu.3dObject')));
    await e.click(find.byKey(const Key('hierarchy.menu.sphere')));
    await e.submit();
    final tab = e.shell.tabs!.focused!;
    final sphere = tab.selection.single;
    expect(tab.tree[sphere]!.name, 'Sphere');
    expect(tab.tree[tab.tree[sphere]!.parent!]!.name, 'Robot');
    // The shapes came in with the Sphere: the model and its sidecar, nothing else.
    expect(Directory('${e.dir.path}/assets/Fynkel').listSync().map((f) => f.uri.pathSegments.last).toSet(), {
      'Primitives.glb',
      'Primitives.glb.meta.json',
    });

    await e.press(LogicalKeyboardKey.keyS, command: true);
    final (_, light) = _saved(e, 'Directional Light');
    expect(light['fynkel.DirectionalLight']!['intensity'], 10000);
    final rotation = (light['fynkel.Transform']!['rotation'] as List).cast<num>();
    expect(rotation[0], lessThan(-0.3), reason: 'tilted down, as Unity\'s sun');
    final (_, sphereComponents) = _saved(e, 'Sphere');
    expect(sphereComponents['fynkel.Collider']!['shape'], 'sphere');
    expect(sphereComponents['fynkel.MeshRenderer']!['mesh'], {'asset': primitivesId, 'sub': 'mesh/Sphere'});
  });
}
