// Unity parity: the Animator Controller (ADR-0032 § 8). Create ▸
// Animator Controller in the Project makes a `.controller.json`;
// dropping it on an object sets its Animator's controller, adding the
// Animator when it has none.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'harness.dart';

const robot = 'c0000000-0000-4000-8000-0000000000e1';

void main() {
  Map<String, Object?> entity(Editor e, String id) =>
      (e.shell.tabs!.focused!.document.root()['entities'] as Map)[id] as Map<String, Object?>;

  List<Map> animators(Editor e, String id) => [
    for (final c in (entity(e, id)['components'] as Map).values)
      if ((c as Map)['type'] == 'fynkel.Animator') c['data'] as Map,
  ];

  testWidgets('New Animator Controller; dropped on an object it becomes its Animator\'s controller', (tester) async {
    final e = await Editor.open(tester);
    e.shell.browser!.setFolder('');
    await tester.pumpAndSettle();
    await e.click(find.byKey(const Key('project.new')));
    await e.click(find.byKey(const Key('project.new.controller')));
    final file = File('${e.dir.path}/assets/New Animator Controller.controller.json');
    expect(file.existsSync(), isTrue);
    final saved = jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
    expect((saved['layers'] as List).single, containsPair('name', 'Base Layer'));
    await e.submit();
    final id = saved['id'] as String;

    // The Robot has no Animator: the drop adds one, with the controller.
    expect(animators(e, robot), isEmpty);
    await e.drag(find.byKey(Key('project.asset.$id')), tester.getCenter(e.row('Robot')));
    expect(animators(e, robot).single['controller'], {'asset': id});
    expect(e.shell.tabs!.focused!.document.history, 1, reason: 'one undo step');

    // The Inspector shows it first, as Unity does.
    await e.click(e.row('Robot'));
    expect(find.byKey(const Key('field.fynkel.Animator#0.controller')), findsOneWidget);
  });
}
