// Unity parity: drag a script onto an object (docs/unity-parity.md § Tier 2).
// A behaviour's file in the Project panel's Scripts drags the behaviour;
// dropped on a Hierarchy row, the Inspector or an object in the Scene view
// it is added with its schema defaults — an override on a prefab instance —
// in one undo step, the selection kept, and not twice on one object.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const robot = 'c0000000-0000-4000-8000-0000000000e1';
const crateA = 'c0000000-0000-4000-8000-0000000000e2';
const sun = 'c0000000-0000-4000-8000-0000000000e3';
const crateB = 'c0000000-0000-4000-8000-0000000000e5';

/// The fixture copy with its game code: `lib/game/spinner.dart` declares
/// the Spinner, `lib/game/util.dart` nothing, `lib/game/pair.dart` two
/// behaviours, one named after the file.
void addScripts(Directory dir) {
  for (final f in ['game/spinner.dart', 'game/util.dart', 'game/pair.dart']) {
    File('${dir.path}/lib/$f')
      ..createSync(recursive: true)
      ..writeAsStringSync('// $f\n');
  }
  File('${dir.path}/lib/generated/behaviours.g.json')
    ..createSync(recursive: true)
    ..writeAsStringSync(
      jsonEncode({
        'package': 'fixture',
        'behaviours': [
          {'id': 'game.Spinner', 'version': 1, 'class': 'Spinner', 'library': 'package:fixture/game/spinner.dart'},
          {'id': 'game.Helper', 'version': 1, 'class': 'Helper', 'library': 'package:fixture/game/pair.dart'},
          {'id': 'game.Pair', 'version': 1, 'class': 'Pair', 'library': 'package:fixture/game/pair.dart'},
        ],
      }),
    );
}

void main() {
  Future<Editor> open(WidgetTester tester) async {
    final e = await Editor.open(tester, prepare: addScripts);
    e.shell.browser!.showScripts('game');
    await settleCooks(tester);
    return e;
  }

  final spinnerFile = find.byKey(const Key('project.script.game/spinner.dart'));

  Map<String, Object?> entity(Editor e, String id) =>
      (e.shell.tabs!.focused!.document.root()['entities'] as Map)[id] as Map<String, Object?>;

  List<Map<String, Object?>> spinners(Editor e, String id) => [
    for (final c in (entity(e, id)['components'] as Map).values)
      if ((c as Map)['type'] == 'game.Spinner') c.cast<String, Object?>(),
  ];

  List<Object?> addedOnInstance(Editor e, String id) => [
    for (final o in ((entity(e, id)['prefab'] as Map)['overrides'] as List? ?? const []))
      if ((o as Map)['op'] == 'addComponent') o['type'],
  ];

  testWidgets('what a script row drags: its behaviour; of two, the one named after the file; none without one', (
    tester,
  ) async {
    final e = await open(tester);
    final browser = e.shell.browser!;
    expect(browser.scriptDragPayload('game/spinner.dart')?.id, 'game.Spinner');
    expect(browser.scriptDragPayload('game/pair.dart')?.id, 'game.Pair');
    expect(browser.scriptDragPayload('game/util.dart'), isNull);
  });

  testWidgets('dropped on Hierarchy rows: added with its defaults, an override on a prefab instance, never twice', (
    tester,
  ) async {
    final e = await open(tester);
    await e.drag(spinnerFile, tester.getCenter(e.row('Sun')));
    final added = spinners(e, sun).single;
    expect(added['version'], 1);
    expect((added['data'] as Map)['speed'], 1.0, reason: 'the schema defaults');
    expect(e.selectedNames, isEmpty, reason: 'the selection stays');

    // The Robot has one already: an entity holds one component per type.
    final robotBefore = entity(e, robot);
    await e.drag(spinnerFile, tester.getCenter(e.row('Robot')));
    expect(entity(e, robot), robotBefore);

    await e.drag(spinnerFile, tester.getCenter(e.row('Crate A')));
    expect(addedOnInstance(e, crateA), ['game.Spinner'], reason: 'the instance keeps its prefab, with an override');
    expect(e.shell.tabs!.focused!.document.history, 2, reason: 'one undo step per drop');

    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(addedOnInstance(e, crateA), isEmpty);
    expect(spinners(e, sun), hasLength(1));
  });

  testWidgets(
    'dropped on the Inspector it goes onto the selection; on the Scene view, onto the object under the pointer',
    (tester) async {
      final e = await open(tester);
      await e.click(e.row('Crate B'));
      await e.drag(spinnerFile, tester.getCenter(find.byKey(const ValueKey('inspector.list'))));
      expect(spinners(e, crateB), hasLength(1));
      expect(e.shell.inspector!.slots.map((s) => s.type), contains('game.Spinner'), reason: 'its card is shown');
      expect(e.selectedNames, ['Crate B']);

      // Frame Crate A in the Scene view and drop on its middle.
      final sv = e.shell.scene!;
      await e.click(e.row('Crate A'));
      sv.frameSelected();
      await settleCooks(tester);
      final world = sv.current!.preview!;
      final m = world.worldMatrices([crateA])[crateA]!;
      final px = sv.viewProjection!.project([m[12], m[13], m[14]])!;
      final picture = find.byWidgetPredicate((w) => w.key is GlobalKey && '${w.key}'.contains('scene.picture'));
      await e.drag(spinnerFile, tester.getTopLeft(picture) + Offset(px[0], px[1]));
      expect(addedOnInstance(e, crateA), ['game.Spinner']);
      expect(e.selectedNames, ['Crate A']);
    },
  );
}
