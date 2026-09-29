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
import 'package:fynkel/fynkel.dart' as fynkel;
import 'package:fynkel_editor/src/animator/animator_model.dart';
import 'package:fynkel_editor/src/hierarchy/presets.dart';
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
    await settleCooks(tester);
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
    await settleCooks(tester);
    expect(find.byKey(const Key('animator.empty')), findsOneWidget);

    // A controller picked in the Project shows.
    e.shell.selection!.inspect(const InspectAsset(controllerId));
    await settleCooks(tester);
    expect(find.text('Entry'), findsOneWidget);
    expect(find.text('Any State'), findsOneWidget);

    // Right-click the empty graph: Create State ▸ Empty. The first state is the default.
    final graph = tester.getRect(find.byKey(const Key('animator.graph')));
    await tester.tapAt(graph.center + const Offset(120, -40), buttons: kSecondaryMouseButton);
    await settleCooks(tester);
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
    await settleCooks(tester);

    // F frames the graph (bob was made out of view).
    await e.click(find.byKey(const Key('animator.state.Idle')));
    await e.press(LogicalKeyboardKey.keyF);
    final view = tester.getRect(find.byKey(const Key('animator.graph')));
    expect(view.contains(tester.getCenter(find.byKey(const Key('animator.state.bob')))), isTrue);
    // Make Transition from Idle, then a click on bob.
    await tester.tap(find.byKey(const Key('animator.state.Idle')), buttons: kSecondaryMouseButton);
    await settleCooks(tester);
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
    await settleCooks(tester);
    await e.click(e.row('Robot'));
    expect(e.shell.animator!.id, controllerId, reason: 'the Robot has no controller: the window stays');
  });

  testWidgets('Create Animator Controller on a model: a state per clip, the first the default', (tester) async {
    final e = await Editor.open(tester);
    e.shell.browser!.setFolder('models');
    await settleCooks(tester);
    await tester.tap(find.byKey(const Key('project.asset.$rigAsset')), buttons: kSecondaryMouseButton);
    await settleCooks(tester);
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

  testWidgets('during Play: the running state fills its bar, and the parameters are the live ones', (tester) async {
    final e = await Editor.open(
      tester,
      prepare: (dir) {
        writeController(dir);
        final f = File('${dir.path}/$controllerPath');
        final json = jsonDecode(f.readAsStringSync()) as Map<String, Object?>;
        json['parameters'] = [
          {'name': 'speed', 'type': 'float'},
          {'name': 'jump', 'type': 'trigger'},
        ];
        json['layers'] = [
          {
            'name': 'Base Layer',
            'defaultState': 'Idle',
            'states': [
              {'name': 'Idle'},
              {'name': 'Jump'},
            ],
            'transitions': [
              {
                'from': 'Idle',
                'to': 'Jump',
                'conditions': [
                  {'parameter': 'jump'},
                ],
              },
            ],
          },
        ];
        f.writeAsStringSync(jsonEncode(json));
      },
    );
    e.shell.workspace!.show(PanelRegistry.animation);
    final running = _FakeLive();
    final a = e.shell.animator!
      ..liveOf = ((object) => object == 'hero' ? running : null)
      ..show(controllerId, object: 'hero');
    a.playChanged();
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 16));
    Iterable<Container> barsIn(String state) => tester.widgetList<Container>(
      find.descendant(
        of: find.byKey(Key('animator.state.$state')),
        matching: find.byWidgetPredicate((w) => w is Container && w.foregroundDecoration != null),
      ),
    );
    expect(barsIn('Idle'), hasLength(1), reason: 'Idle runs');
    expect(barsIn('Jump'), isEmpty);
    expect(find.textContaining('live', findRichText: true), findsWidgets);

    // A trigger clicked goes to the running animator, not the file.
    await tester.tap(find.byKey(const Key('animator.parameter.jump.value')));
    await tester.pump();
    expect(running.set, [('jump', true)]);
    // The crossfade: both boxes fill.
    running.state = const fynkel.AnimatorStateInfo(
      layer: 0,
      name: 'Idle',
      normalizedTime: 0.6,
      length: 1,
      next: 'Jump',
      nextNormalizedTime: 0.1,
      transitionProgress: 0.4,
    );
    await tester.pump(const Duration(milliseconds: 16));
    expect(barsIn('Jump'), hasLength(1));
    // The slot row (ADR-0033 § 2): the layer's name, then the one-shot
    // over the machine and how much of it shows.
    expect(tester.widget<Text>(find.byKey(const Key('animator.slot.name'))).data, 'Base Layer');
    expect(find.byKey(const Key('animator.slot.oneShot')), findsNothing);
    running.state = const fynkel.AnimatorStateInfo(
      layer: 0,
      name: 'Idle',
      normalizedTime: 0.7,
      length: 1,
      slot: 'Base Layer',
      oneShot: fynkel.AssetRef(rigAsset, sub: 'anim/wave'),
      oneShotTime: 0.25,
      oneShotWeight: 0.5,
    );
    await tester.pump(const Duration(milliseconds: 16));
    expect(tester.widget<Text>(find.byKey(const Key('animator.slot.oneShot'))).data, endsWith('anim/wave'));
    expect(tester.widget<LinearProgressIndicator>(find.byKey(const Key('animator.slot.weight'))).value, 0.5);
    running.state = const fynkel.AnimatorStateInfo(
      layer: 0,
      name: 'Idle',
      normalizedTime: 0.7,
      length: 1,
      oneShot: fynkel.AssetRef('c0000000-0000-4000-8000-00000000abcd'),
      oneShotWeight: 1,
    );
    await tester.pump(const Duration(milliseconds: 16));
    expect(tester.widget<Text>(find.byKey(const Key('animator.slot.oneShot'))).data, 'a clip made in code');
    final saved = jsonDecode(File('${e.dir.path}/$controllerPath').readAsStringSync()) as Map;
    expect((saved['parameters'] as List).last, {'name': 'jump', 'type': 'trigger'}, reason: 'the file is left alone');

    // Stop: the window is the controller's again.
    a.liveOf = null;
    a.playChanged();
    await settleCooks(tester);
    expect(barsIn('Idle'), isEmpty);
  });

  testWidgets('a blend tree (ADR-0033): From New Blend Tree, its motions and map, a nested tree, one undo step each', (
    tester,
  ) async {
    final e = await Editor.open(
      tester,
      prepare: (dir) {
        writeController(dir);
        final f = File('${dir.path}/$controllerPath');
        final json = jsonDecode(f.readAsStringSync()) as Map<String, Object?>;
        json['parameters'] = [
          {'name': 'speed', 'type': 'float'},
          {'name': 'turn', 'type': 'float'},
        ];
        f.writeAsStringSync(jsonEncode(json));
      },
    );
    Map<String, Object?> saved() =>
        jsonDecode(File('${e.dir.path}/$controllerPath').readAsStringSync()) as Map<String, Object?>;
    Map<String, Object?> state(String name) => [
      for (final s in ((saved()['layers'] as List).first as Map)['states'] as List)
        if ((s as Map)['name'] == name) s.cast<String, Object?>(),
    ].single;
    Map tree(String name) => (state(name)['motion'] as Map)['blendTree'] as Map;
    final doc = e.shell.assetEdits!.document(controllerId)!;
    e.shell.workspace!.show(PanelRegistry.animation);
    e.shell.selection!.inspect(const InspectAsset(controllerId));
    await settleCooks(tester);

    // The preview plays in a loop: time is pumped, never settled.
    Future<void> settle() async {
      for (var i = 0; i < 8; i++) {
        await tester.pump(const Duration(milliseconds: 50));
      }
    }

    Future<void> click(Finder f) async {
      await tester.tap(f);
      await settle();
    }

    Future<void> pick(String name) async {
      await tester.enterText(find.byKey(const ValueKey('assetPicker.search')), name);
      await settle();
      // The one row the search leaves.
      await click(find.byWidgetPredicate((w) => '${w.key}'.startsWith("[<'assetPicker.row.")).first);
    }

    const wave = {'asset': rigAsset, 'sub': 'anim/wave'};
    const bob = {'asset': rigAsset, 'sub': 'anim/bob'};
    // Right-click the graph: Create State ▸ From New Blend Tree…, a 1D tree on the first
    // float holding the clip picked (a tree is never empty).
    final graph = tester.getRect(find.byKey(const Key('animator.graph')));
    await tester.tapAt(graph.center + const Offset(120, -40), buttons: kSecondaryMouseButton);
    await settle();
    await click(find.byKey(const Key('animator.menu.state.tree')));
    await pick('wave');
    expect(tree('Blend Tree'), {
      'type': '1d',
      'parameters': ['speed'],
      'children': [
        {'motion': wave, 'threshold': 0},
      ],
    });
    expect(find.byKey(const Key('animator.state.Blend Tree.tree')), findsOneWidget, reason: 'the box shows a tree');
    expect(find.byKey(const Key('blendTree.type')), findsOneWidget, reason: 'the Inspector edits it');
    expect(find.byKey(const Key('animatorItem.motion')), findsNothing);

    // + ▸ Add Motion…: bob, one past the last threshold. The last motion is not removed.
    await click(find.byKey(const Key('blendTree.add')));
    await click(find.byKey(const Key('blendTree.add.motion')));
    await pick('bob');
    expect(tree('Blend Tree')['children'], [
      {'motion': wave, 'threshold': 0},
      {'motion': bob, 'threshold': 1.0},
    ]);
    expect(doc.history, 2, reason: 'one step each');
    final c = e.shell.animator!.controller!;

    // speed 0: all wave. A click halfway along the line tries speed 0.5 (the file is left alone).
    String weight(int i) => tester.widget<Text>(find.byKey(Key('blendTree.child.$i.weight'))).data!;
    expect((weight(0), weight(1)), ('100 %', '0 %'));
    final map = tester.getRect(find.byKey(const Key('blendTree.map')));
    await tester.tapAt(map.centerLeft + Offset(map.width / 2, 0));
    await tester.pump();
    expect((weight(0), weight(1)), ('50 %', '50 %'));
    expect((saved()['parameters'] as List).first, {'name': 'speed', 'type': 'float'});
    expect(find.byKey(const Key('blendTree.preview')), findsOneWidget, reason: 'it plays on the clips\' model');

    // Dragging a diamond moves the child, in one step.
    final steps = doc.history;
    final right = map.centerLeft + Offset(map.width * (0.12 + 1) / 1.24, 0);
    final gesture = await tester.startGesture(right);
    await gesture.moveBy(Offset(-map.width / 1.24 / 2, 0));
    await gesture.moveBy(const Offset(-1, 0));
    await gesture.up();
    await settle();
    expect(doc.history, steps + 1);
    final moved = ((tree('Blend Tree')['children'] as List)[1] as Map)['threshold'] as num;
    expect(moved, closeTo(0.5, 0.05));

    // 2D Freeform Cartesian: two parameters, each threshold a position.
    c.setTreeType(0, 'Blend Tree', const [], 'freeformCartesian');
    expect(tree('Blend Tree')['parameters'], ['speed', 'turn']);
    expect(((tree('Blend Tree')['children'] as List)[0] as Map)['position'], [0.0, 0.0]);

    // + ▸ New Blend Tree: a child tree; open it, then back along the path.
    await settle();
    await click(find.byKey(const Key('blendTree.add')));
    await click(find.byKey(const Key('blendTree.add.tree')));
    await pick('wave');
    final nested = (((tree('Blend Tree')['children'] as List)[2] as Map)['motion'] as Map)['blendTree'] as Map;
    expect(nested['children'], [
      {'motion': wave, 'threshold': 0},
    ]);
    await click(find.byKey(const Key('blendTree.child.2.open')));
    expect(find.byKey(const Key('blendTree.path')), findsOneWidget);
    expect(find.byKey(const Key('blendTree.child.1')), findsNothing, reason: 'the nested tree holds one');
    expect(c.removeChild(0, 'Blend Tree', const [2], 0), isFalse, reason: 'a tree keeps one motion');
    await click(find.byKey(const Key('blendTree.path.0')));
    expect(find.byKey(const Key('blendTree.path')), findsNothing);
    expect(find.byKey(const Key('blendTree.child.2.open')), findsOneWidget);

    // A parameter renamed reaches the tree.
    c.renameParameter('speed', 'pace');
    expect(tree('Blend Tree')['parameters'], ['pace', 'turn']);

    // Make Blend Tree on a state playing a clip: a 1D tree holding the clip.
    final made = e.shell.animator!.createState(const Offset(0, 200), motion: bob)!;
    await settle();
    await click(find.byKey(const Key('animatorItem.makeTree')));
    expect(tree(made), {
      'type': '1d',
      'parameters': ['pace'],
      'children': [
        {'motion': bob, 'threshold': 0},
      ],
    });
  });

  testWidgets('a sub-state machine (ADR-0033 § 4): made, opened with a double-click, a path bar back, interruptions', (
    tester,
  ) async {
    final e = await Editor.open(tester, prepare: writeController);
    Map<String, Object?> saved() =>
        jsonDecode(File('${e.dir.path}/$controllerPath').readAsStringSync()) as Map<String, Object?>;
    Map<String, Object?> base() => (saved()['layers'] as List).first as Map<String, Object?>;
    final doc = e.shell.assetEdits!.document(controllerId) ?? (throw StateError(e.shell.assetEdits!.lastError ?? "?"));
    e.shell.workspace!.show(PanelRegistry.animation);
    e.shell.selection!.inspect(const InspectAsset(controllerId));
    await settleCooks(tester);
    final model = e.shell.animator!;
    expect(model.createState(const Offset(0, 0)), 'New State');

    // Right-click the graph: Create Sub-State Machine — one state inside, its default.
    final graph = tester.getRect(find.byKey(const Key('animator.graph')));
    await tester.tapAt(graph.center + const Offset(150, 60), buttons: kSecondaryMouseButton);
    await settleCooks(tester);
    await e.click(find.byKey(const Key('animator.menu.machine')));
    expect((base()['states'] as List)[1], {
      'name': 'New Sub-State Machine',
      'defaultState': 'New State',
      'states': [
        {'name': 'New State'},
      ],
    });
    expect(find.byKey(const Key('animator.state.New Sub-State Machine.machine')), findsOneWidget);
    expect(find.byKey(const Key('animatorItem.machineHint')), findsOneWidget);
    expect(model.renameState('New Sub-State Machine', 'Airborne'), isTrue);
    await settleCooks(tester);
    final history = doc.history;

    // A double-click opens it: the path bar, Entry, no Any State.
    await tester.tap(find.byKey(const Key('animator.state.Airborne')));
    await tester.pump(kDoubleTapMinTime);
    await tester.tap(find.byKey(const Key('animator.state.Airborne')));
    await settleCooks(tester);
    expect(model.machine, ['Airborne']);
    expect(find.byKey(const Key('animator.path')), findsOneWidget);
    expect(find.text('Any State'), findsNothing);

    // A state made inside lands inside, placed by path; a transition between them.
    expect(model.createState(const Offset(220, 0)), 'New State 1');
    await settleCooks(tester);
    expect(model.renameState('New State 1', 'Fall'), isTrue);
    final c = model.controller!;
    expect(c.makeTransition(0, 'New State', 'Fall'), 0);
    final airborne = (base()['states'] as List)[1] as Map;
    expect((airborne['states'] as List).map((s) => (s as Map)['name']), ['New State', 'Fall']);
    expect(airborne['transitions'], [
      {'from': 'New State', 'to': 'Fall', 'exitTime': 0.75, 'duration': 0.25},
    ]);
    expect(((saved()['editor'] as Map)['Base Layer'] as Map).containsKey('Airborne/Fall'), isTrue);
    expect(c.makeTransition(0, 'any', 'Fall'), isNull, reason: 'Any State belongs to the layer');
    expect(doc.history, history + 3);

    // The transition's interruption, in words.
    model.selectTransition(0);
    await settleCooks(tester);
    expect(find.textContaining('Can be interrupted by: nothing'), findsOneWidget);
    expect(c.setTransitionField(0, 0, 'interruptibleBy', 'next'), isTrue);
    await settleCooks(tester);
    expect(find.textContaining('Can be interrupted by: the next state'), findsOneWidget);

    // The path bar goes back; a rename from outside moves the places inside.
    await e.click(find.byKey(const Key('animator.path.0')));
    expect(model.machine, isEmpty);
    expect(find.text('Any State'), findsOneWidget);
    expect(model.renameState('Airborne', 'Air'), isTrue);
    final places = (saved()['editor'] as Map)['Base Layer'] as Map;
    expect(places.containsKey('Air/Fall'), isTrue);
    expect(places.containsKey('Airborne/Fall'), isFalse);

    // Out of the machine: no exit time (its states have their own clocks).
    expect(model.controller!.makeTransition(0, 'Air', 'New State'), 0);
    expect((base()['transitions'] as List).single, {'from': 'Air', 'to': 'New State', 'duration': 0.25});
  });
  testWidgets('masks and IK (ADR-0033 § 5): the IK tab, a layer\'s Mask ticked on the object\'s nodes, one undo step each', (
    tester,
  ) async {
    final e = await Editor.open(
      tester,
      prepare: (dir) => File('${dir.path}/$controllerPath').writeAsStringSync(
        jsonEncode({
          r'$schema': 'https://fynkel.dev/schemas/controller.schema.json',
          'formatVersion': 1,
          'id': controllerId,
          'name': 'Rig',
          'parameters': [
            {'name': 'aim', 'type': 'float'},
          ],
          'layers': [
            {'name': 'Base Layer'},
            {'name': 'Arms'},
          ],
        }),
      ),
    );
    Map<String, Object?> saved() =>
        jsonDecode(File('${e.dir.path}/$controllerPath').readAsStringSync()) as Map<String, Object?>;
    List<Object?>? mask() => ((saved()['layers'] as List)[1] as Map)['mask'] as List?;
    final h = e.shell.hierarchy!;
    expect(h.createPreset(GameObjectPreset.cube, parent: robot), isTrue);
    expect(h.assignController({'asset': controllerId}, robot), isTrue);
    await settleCooks(tester);
    await e.click(e.row('Robot'));
    e.shell.workspace!.show(PanelRegistry.animation);
    await settleCooks(tester);
    final model = e.shell.animator!;
    expect(model.object, robot);
    expect(model.nodes.map((n) => n.name), ['Robot', 'Cube']);
    final doc = e.shell.assetEdits!.document(controllerId)!;
    final history = doc.history;

    // The IK tab beside Parameters: + ▸ Look At.
    await e.click(find.byKey(const Key('animator.tab.ik')));
    expect(find.byKey(const Key('animator.ik.empty')), findsOneWidget);
    await e.click(find.byKey(const Key('animator.addIk')));
    await e.click(find.byKey(const Key('animator.addIk.lookAt')));
    expect(saved()['ik'], [
      {'name': 'look', 'type': 'lookAt', 'head': 'Head'},
    ]);
    expect(find.byKey(const Key('animator.ik.0.head')), findsOneWidget);
    expect(doc.history, history + 1);

    // Its weight from a float parameter; a rename follows, a delete drops it.
    final c = model.controller!;
    expect(c.setIkField(0, 'weight', 'aim'), isTrue);
    expect(c.renameParameter('aim', 'aiming'), isTrue);
    expect((saved()['ik'] as List).single, containsPair('weight', 'aiming'));
    expect(c.deleteParameter('aiming'), isTrue);
    expect((saved()['ik'] as List).single, isNot(contains('weight')));

    // The Arms layer's Mask: the object's nodes, ticked; Clear, the whole character.
    await e.click(find.byKey(const Key('animator.layer.1')));
    expect(find.textContaining('Mask: Whole character'), findsOneWidget);
    await e.click(find.byKey(const Key('animator.layer.mask')));
    expect(find.byKey(const Key('animator.mask')), findsOneWidget);
    expect(tester.widget<Checkbox>(find.byKey(const Key('animator.mask.node.Robot.check'))).value, isTrue);
    final before = doc.history;
    await e.click(find.byKey(const Key('animator.mask.node.Cube')));
    expect(mask(), [
      {'node': 'Cube', 'include': false},
    ]);
    await e.click(find.byKey(const Key('animator.mask.node.Robot')));
    expect(mask(), [
      {'node': 'Robot', 'include': false},
    ], reason: 'a tick says it for what is under the node too');
    expect(tester.widget<Checkbox>(find.byKey(const Key('animator.mask.node.Cube.check'))).value, isFalse);
    expect(find.byKey(const Key('animator.mask.entry.0')), findsOneWidget);
    expect(doc.history, before + 2);
    await e.click(find.byKey(const Key('animator.mask.clear')));
    expect(mask(), isNull);
    await e.click(find.byKey(const Key('animator.mask.close')));
    expect(find.byKey(const Key('animator.mask')), findsNothing);
  });
}

final class _FakeLive implements AnimatorLive {
  fynkel.AnimatorStateInfo state = const fynkel.AnimatorStateInfo(
    layer: 0,
    name: 'Idle',
    normalizedTime: 1.25,
    length: 1,
  );
  final set = <(String, Object)>[];

  @override
  List<fynkel.AnimatorStateInfo> get layers => [state];

  @override
  List<fynkel.AnimatorParameter> get parameters => [
    const fynkel.AnimatorParameter('speed', fynkel.AnimatorParameterKind.float, 2.5),
    const fynkel.AnimatorParameter('jump', fynkel.AnimatorParameterKind.trigger, false),
  ];

  @override
  void setParameter(String name, Object value) => set.add((name, value));
}
