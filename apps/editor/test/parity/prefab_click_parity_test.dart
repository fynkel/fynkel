// Unity parity: clicking into a prefab in the Scene view (docs/unity-parity.md
// § Tier 3). The first click on a prefab instance selects its root; the
// next click there selects the child under the pointer, and further clicks
// cycle on to what is behind. With the instance (or something in it)
// selected already, a click goes straight to the child.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/scene_view/scene_math.dart';

import 'harness.dart';

const crateA = 'c0000000-0000-4000-8000-0000000000e2';

/// The fixture copy whose Crate prefab has a Lid child one metre above
/// its root, drawn with the same mesh.
void addLid(Directory dir) {
  final file = File('${dir.path}/assets/prefabs/crate.prefab.json');
  final prefab = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
  final entities = prefab['entities'] as Map<String, dynamic>;
  final root = entities[prefab['root']] as Map<String, dynamic>;
  final renderer = (root['components'] as Map).values.firstWhere((c) => c['type'] == 'fynkel.MeshRenderer');
  const lid = 'c0000000-0000-4000-8000-0000000000b9';
  root['children'] = [lid];
  entities[lid] = {
    'name': 'Lid',
    'parent': prefab['root'],
    'children': <String>[],
    'components': {
      'c0000000-0000-4000-8000-0000000000d8': {
        'type': 'fynkel.Transform',
        'version': 1,
        'data': {
          'position': [0, 1.5, 0],
          'rotation': [0, 0, 0, 1],
          'scale': [0.5, 0.5, 0.5],
        },
      },
      'c0000000-0000-4000-8000-0000000000d9': renderer,
    },
  };
  file.writeAsStringSync(jsonEncode(prefab));
}

void main() {
  testWidgets('first click the prefab root, then the child under the pointer, then what is behind', (tester) async {
    final e = await Editor.open(tester, prepare: addLid);
    final sv = e.shell.scene!;
    final tab = e.shell.tabs!.focused!;
    await e.click(e.row('Crate A'));
    sv.frameSelected();
    await settleCooks(tester);
    tab.clearSelection();
    // The Select tool: a gizmo on the clicked object would take the next click.
    await e.press(LogicalKeyboardKey.keyQ);

    final lid = tab.tree.nodes.values.singleWhere((n) => n.name == 'Lid').id;
    final world = sv.current!.preview!;
    final m = world.worldMatrices([lid])[lid]!;
    final px = sv.viewProjection!.project(matTranslation(m))!;
    final picture = find.byWidgetPredicate((w) => w.key is GlobalKey && '${w.key}'.contains('scene.picture'));
    final at = tester.getTopLeft(picture) + Offset(px[0], px[1]) / tester.view.devicePixelRatio;

    Future<void> tap(Offset p) async {
      await tester.tapAt(p);
      await tester.pump(const Duration(milliseconds: 600)); // not a double-click
      await settleCooks(tester);
    }

    await tap(at);
    expect(tab.selection, [crateA], reason: 'the prefab root first');
    await tap(at);
    expect(tab.selection, [lid], reason: 'then the child under the pointer');
    await tap(at);
    expect(tab.selection, isNot(contains(lid)), reason: 'then what is behind');

    // With the Lid selected, a click on it (anywhere on it) stays on it.
    tab.select(lid);
    await settleCooks(tester);
    await tap(at + const Offset(6, 4));
    expect(tab.selection, [lid]);
    // Selected from the Hierarchy, the root: a click on the Lid goes into it.
    tab.select(crateA);
    await settleCooks(tester);
    await tap(at + const Offset(-6, 3));
    expect(tab.selection, [lid]);
  });
}
