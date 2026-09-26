// Unity parity: the Animator Controller (ADR-0032 § 8). Create ▸
// Animator Controller in the Project makes a `.controller.json`;
// dropping it on an object sets its Animator's controller, adding the
// Animator when it has none. The Animator window shows the controller of
// the selection: Create State on a right-click, Make Transition then a
// click on the target, parameters with +, the Inspector for a state or a
// transition, and every change one undo step (⌘Z in the window).
import 'dart:convert';
import 'dart:io';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_editor/src/shell/editor_selection.dart';
import 'package:fynkel_editor/src/shell/workspace.dart';

import 'harness.dart';

const robot = 'c0000000-0000-4000-8000-0000000000e1';
const rigAsset = 'c0000000-0000-4000-8000-000000000005';
const controllerId = 'c0000000-0000-4000-8000-0000000000f7';
const controllerPath = 'assets/rigs/rig.controller.json';

/// An empty controller beside the rig.
void writeController(Directory dir) => File('${dir.path}/$controllerPath').writeAsStringSync(
  jsonEncode({
    r'$schema': 'https://fynkel.dev/schemas/controller.schema.json',
    'formatVersion': 1,
    'id': controllerId,
    'name': 'Rig',
    'layers': [
      {'name': 'Base Layer'},
    ],
  }),
);

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

  testWidgets('the Animator window: states, a transition with a condition, parameters, one undo step each', (
    tester,
  ) async {
    final e = await Editor.open(tester, prepare: writeController);
    Map<String, Object?> saved() =>
        jsonDecode(File('${e.dir.path}/$controllerPath').readAsStringSync()) as Map<String, Object?>;
    Map<String, Object?> base() => (saved()['layers'] as List).first as Map<String, Object?>;
    final doc = e.shell.assetEdits!.document(controllerId) ?? (throw StateError(e.shell.assetEdits!.lastError ?? "?"));
    e.shell.workspace!.show(PanelRegistry.animation);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('animator.empty')), findsOneWidget);

    // A controller picked in the Project shows.
    e.shell.selection!.inspect(const InspectAsset(controllerId));
    await tester.pumpAndSettle();
    expect(find.text('Entry'), findsOneWidget);
    expect(find.text('Any State'), findsOneWidget);

    // Right-click the empty graph: Create State ▸ Empty. The first state is the default.
    final graph = tester.getRect(find.byKey(const Key('animator.graph')));
    await tester.tapAt(graph.center + const Offset(120, -40), buttons: kSecondaryMouseButton);
    await tester.pumpAndSettle();
    await e.click(find.byKey(const Key('animator.menu.state.empty')));
    expect(base()['states'], [
      {'name': 'New State'},
    ]);
    expect(base()['defaultState'], 'New State');
    expect(doc.history, 1);
    // It is selected, and the Inspector shows it: rename it there.
    expect(
      find.descendant(of: find.byKey(const Key('animatorItem.title')), matching: find.text('New State')),
      findsOneWidget,
    );
    await e.click(find.descendant(of: find.byKey(const Key('animatorItem.name')), matching: find.byType(TextField)));
    await e.type('Idle');
    await e.submit();
    expect(base()['defaultState'], 'Idle');
    expect(doc.history, 2);

    // A state that plays a clip (what a drop from the Project makes), named after it.
    expect(e.shell.animator!.createState(const Offset(260, 0), motion: {'asset': rigAsset, 'sub': 'anim/bob'}), 'bob');
    await tester.pumpAndSettle();

    // F frames the graph (bob was made out of view).
    await e.click(find.byKey(const Key('animator.state.Idle')));
    await e.press(LogicalKeyboardKey.keyF);
    final view = tester.getRect(find.byKey(const Key('animator.graph')));
    expect(view.contains(tester.getCenter(find.byKey(const Key('animator.state.bob')))), isTrue);
    // Make Transition from Idle, then a click on bob.
    await tester.tap(find.byKey(const Key('animator.state.Idle')), buttons: kSecondaryMouseButton);
    await tester.pumpAndSettle();
    await e.click(find.byKey(const Key('animator.menu.link')));
    await e.click(find.byKey(const Key('animator.state.bob')));
    expect(base()['transitions'], [
      {'from': 'Idle', 'to': 'bob', 'exitTime': 0.75, 'duration': 0.25},
    ]);
    expect(
      find.descendant(of: find.byKey(const Key('animatorItem.title')), matching: find.text('Idle → bob')),
      findsOneWidget,
    );

    // A trigger parameter, renamed; a condition on it; a rename reaches the condition.
    await e.click(find.byKey(const Key('animator.addParameter')));
    await e.click(find.byKey(const Key('animator.addParameter.trigger')));
    expect(saved()['parameters'], [
      {'name': 'newTrigger', 'type': 'trigger'},
    ]);
    await e.click(find.byKey(const Key('animatorItem.addCondition')));
    expect(((base()['transitions'] as List).single as Map)['conditions'], [
      {'parameter': 'newTrigger'},
    ]);
    await e.click(
      find.descendant(
        of: find.byKey(const Key('animator.parameter.newTrigger.name')),
        matching: find.byType(TextField),
      ),
    );
    await e.type('jump');
    await e.submit();
    expect(((base()['transitions'] as List).single as Map)['conditions'], [
      {'parameter': 'jump'},
    ]);
    final steps = doc.history;

    // ⌘Z in the window undoes the rename, in one step, and saves.
    await e.click(find.byKey(const Key('animator.state.bob')));
    await e.press(LogicalKeyboardKey.keyZ, command: true);
    expect(((base()['transitions'] as List).single as Map)['conditions'], [
      {'parameter': 'newTrigger'},
    ]);
    expect(doc.history, steps - 1);

    // ⌫ deletes the selected state with its transitions.
    await e.click(find.byKey(const Key('animator.state.bob')));
    await e.press(LogicalKeyboardKey.backspace);
    expect([for (final s in base()['states'] as List) (s as Map)['name']], ['Idle']);
    expect(base().containsKey('transitions'), isFalse);
    expect(find.byKey(const Key('animator.state.bob')), findsNothing);

    // Selecting an object whose Animator names another controller follows it.
    e.shell.selection!.inspect(const InspectAsset(controllerId));
    await tester.pumpAndSettle();
    await e.click(e.row('Robot'));
    expect(e.shell.animator!.id, controllerId, reason: 'the Robot has no controller: the window stays');
  });

  testWidgets('Create Animator Controller on a model: a state per clip, the first the default', (tester) async {
    final e = await Editor.open(tester);
    e.shell.browser!.setFolder('models');
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('project.asset.$rigAsset')), buttons: kSecondaryMouseButton);
    await tester.pumpAndSettle();
    await e.click(find.byKey(const Key('project.menu.createController')));
    final file = File('${e.dir.path}/assets/models/rig.controller.json');
    final saved = jsonDecode(file.readAsStringSync()) as Map<String, Object?>;
    final base = (saved['layers'] as List).single as Map;
    // In the order the project lists the model's parts.
    expect(base['defaultState'], 'bob');
    expect(base['states'], [
      {
        'name': 'bob',
        'motion': {'asset': rigAsset, 'sub': 'anim/bob'},
      },
      {
        'name': 'wave',
        'motion': {'asset': rigAsset, 'sub': 'anim/wave'},
      },
    ]);
    expect(e.shell.browser!.lead, saved['id']);
  });
}
