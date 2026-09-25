// The Hierarchy: the tree model (rows, expansion, filter, incremental
// patches), the operations as single transactions (rename, duplicate,
// delete, create, reparent with "keep world transform"), the 10 000-entity
// scene, and the panel widget (virtualised rows, search, rename in place,
// keyboard, drag-and-drop).
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_model.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_panel.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_tree.dart';
import 'package:fynkel_editor/src/hierarchy/transform_math.dart';

import 'support/fakes.dart';

FakeDocument scene({String kind = 'scene'}) => FakeDocument(
  path: '/fake/game/assets/${kind}s/h.$kind.json',
  id: 'h',
  kind: kind,
  name: 'H',
  relativePath: '${kind}s/h.$kind.json',
  entities: {
    'cam': FakeDocument.entity(
      'Camera',
      position: [0, 2, 6],
      components: {
        'c': {'type': 'fynkel.Camera', 'version': 1, 'data': {}},
      },
    ),
    'robot': FakeDocument.entity('Robot', children: ['arm', 'head'], position: [10, 0, 0], scale: [2, 2, 2]),
    'arm': FakeDocument.entity('Arm', parent: 'robot', position: [1, 0, 0]),
    'head': FakeDocument.entity(
      'Head',
      parent: 'robot',
      position: [0, 1, 0],
      components: {
        'm': {'type': 'fynkel.MeshRenderer', 'version': 1, 'data': {}},
      },
    ),
    'floor': FakeDocument.entity('Floor'),
    'crate': FakeDocument.entity(
      'Crate',
      position: [3, 0, 0],
      prefab: {
        'asset': {'asset': 'prefab-crate'},
        'instance': 'i1',
        'overrides': [
          {'path': 'x'},
        ],
      },
    ),
  },
);

(FakeProject, DocumentTabs, HierarchyModel) setUpModel({FakeDocument? doc}) {
  final project = FakeProject(dir: '/fake/game');
  project.addDocument(doc ?? scene());
  final tabs = DocumentTabs(project: project);
  final model = HierarchyModel(tabs: tabs, project: project);
  tabs.openDocument(project.fakeDocuments.first.id);
  return (project, tabs, model);
}

void main() {
  group('HierarchyTree', () {
    test('rows follow the roots and the expansion; filters keep the ancestors dimmed', () {
      final (_, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      final tree = model.tree!;
      expect(tree.roots, ['cam', 'robot', 'floor', 'crate']);
      expect(tree.rows.map((r) => r.id), ['cam', 'robot', 'floor', 'crate']); // collapsed
      expect(tree.rows[1].hasChildren, isTrue);
      model.expand('robot');
      expect(tree.rows.map((r) => r.id), ['cam', 'robot', 'arm', 'head', 'floor', 'crate']);
      expect(tree.rows[2].depth, 1);
      model.setQuery('head');
      expect(tree.rows.map((r) => r.id), ['robot', 'head']);
      expect(tree.rows[0].matches, isFalse); // ancestor, dimmed
      expect(tree.rows[1].matches, isTrue);
      model.setFilter(const HierarchyFilter(componentTypes: {'fynkel.Camera'}));
      expect(tree.rows.map((r) => r.id), ['cam']);
      model.setFilter(const HierarchyFilter(prefab: PrefabFilter.instances));
      expect(tree.rows.map((r) => r.id), ['crate']);
      expect(tree['crate']!.isPrefabInstance, isTrue);
      expect(tree['crate']!.prefabAsset, 'prefab-crate');
      expect(tree['crate']!.overrideCount, 1);
      model.clearFilter();
      model.collapseAll();
      expect(tree.rows.length, 4);
      model.expandAll();
      expect(tree.rows.length, 6);
    });

    test('topmost, ancestors, siblings and world matrices', () {
      final (_, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      final tree = model.tree!;
      expect(tree.topmost(['arm', 'robot', 'floor']), ['robot', 'floor']);
      expect(tree.ancestorsOf('arm'), ['robot']);
      expect(tree.isAncestorOrSelf('robot', 'arm'), isTrue);
      expect(tree.isAncestorOrSelf('arm', 'robot'), isFalse);
      final (headSiblings, headIndex) = tree.siblingsOf('head');
      expect(headSiblings, ['arm', 'head']);
      expect(headIndex, 1);
      final (floorSiblings, floorIndex) = tree.siblingsOf('floor');
      expect(floorSiblings, ['cam', 'robot', 'floor', 'crate']);
      expect(floorIndex, 2);
      final w = tree.worldMatrix('arm');
      final t = Trs.fromMatrix(w);
      expect(t.position, [12, 0, 0]); // robot at 10 scaled 2 → arm's local 1 becomes 2
      expect(t.scale, [2, 2, 2]);
    });
  });

  test('item 9e: a locked Hierarchy keeps its document while another tab is focused; its close lets go', () {
    final (project, tabs, model) = setUpModel();
    addTearDown(() {
      model.dispose();
      tabs.dispose();
    });
    final first = tabs.focused!;
    final other = project.addDocument(
      FakeDocument(path: '/fake/game/assets/b.scene.json', id: 'b', entities: {'x': FakeDocument.entity('X')}),
    );
    var notified = 0;
    model.addListener(() => notified++);
    model.setLocked(true);
    expect(model.locked, isTrue);
    expect(notified, 1, reason: 'the padlock redraws');
    final second = tabs.openDocument(other.id)!;
    expect(tabs.focused, second);
    expect(model.tab, first, reason: 'locked to the first document');
    model.setLocked(false);
    expect(model.tab, second, reason: 'unlocked, it follows the focused tab again');
    model.setLocked(true);
    tabs.closeDiscarding(second);
    expect(model.locked, isFalse);
    expect(model.tab, first);

    // Nothing to lock without a document.
    final empty = HierarchyModel(
      tabs: DocumentTabs(project: FakeProject(dir: '/fake/empty')),
      project: project,
    );
    addTearDown(() {
      empty.tabs.dispose();
      empty.dispose();
    });
    empty.setLocked(true);
    expect(empty.locked, isFalse);
  });

  group('operations, one transaction each', () {
    test('rename in place', () {
      final (project, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      final doc = project.fakeDocuments.first;
      model.select('robot');
      model.beginRename();
      expect(model.renaming, 'robot');
      expect(model.rename('robot', '  Bot '), isTrue);
      expect(model.renaming, isNull);
      expect(model.tree!['robot']!.name, 'Bot');
      expect(doc.history, 1);
      expect(model.rename('robot', 'Bot'), isFalse); // unchanged: not a transaction
      expect(model.rename('robot', '   '), isFalse);
      expect(doc.history, 1);
    });

    test('duplicate copies the topmost selection with its subtree and selects the copies', () {
      final (project, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      final doc = project.fakeDocuments.first;
      model.select('robot');
      model.select('arm', toggle: true); // inside robot: comes along, not copied twice
      model.select('floor', toggle: true);
      expect(model.duplicate(), isTrue);
      expect(doc.history, 1);
      final tree = model.tree!;
      expect(tree.roots.length, 6);
      final copies = model.selection.toList();
      expect(copies.length, 2);
      // Unity's names, each right after its original.
      expect(tree[copies[0]]!.name, 'Robot (1)');
      expect(tree[copies[0]]!.children.length, 2);
      expect(tree[copies[1]]!.name, 'Floor (1)');
      expect(tree.roots.indexOf(copies[0]), tree.roots.indexOf('robot') + 1);
      expect(tree.roots.indexOf(copies[1]), tree.roots.indexOf('floor') + 1);
      expect(tree.builds, 1);
    });

    test('delete removes the subtrees and leaves nothing selected', () {
      final (project, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      model.expand('robot');
      model.select('robot');
      model.select('crate', toggle: true);
      expect(model.delete(), isTrue);
      expect(project.fakeDocuments.first.history, 1);
      final tree = model.tree!;
      expect(tree.roots, ['cam', 'floor']);
      expect(tree['arm'], isNull);
      expect(model.selection, isEmpty);
    });

    test('create empty under the selection, create with a native component and its defaults', () {
      final (project, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      final doc = project.fakeDocuments.first;
      model.select('robot');
      expect(model.createEmpty(), isTrue);
      final id = model.selection.single;
      expect(model.tree![id]!.parent, 'robot');
      expect(model.tree![id]!.name, 'GameObject');
      expect(model.tree!.expanded, contains('robot'));
      expect(model.renaming, id); // ready to type a name
      expect(doc.entitiesView[id]!['components'], hasLength(1));

      model.clearSelection();
      expect(model.createWith('fynkel.Camera'), isTrue);
      final cam = model.selection.single;
      expect(model.tree![cam]!.parent, isNull);
      expect(model.tree![cam]!.name, 'Camera');
      final comps = (doc.entitiesView[cam]!['components'] as Map).values.toList();
      expect(comps.map((c) => c['type']), ['fynkel.Transform', 'fynkel.Camera']);
      expect(comps[1]['data'], project.defaults['fynkel.Camera']);
      expect(doc.history, 2);

      expect(model.createWith('fynkel.Skin'), isFalse); // no defaults: not authorable
      expect(model.tab!.lastError, contains('fynkel.Skin'));
      expect(doc.history, 2);
    });

    test('reparent with keep world transform rewrites the Transform in the same transaction', () {
      final (project, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      final doc = project.fakeDocuments.first;
      // Crate (world 3,0,0) under Robot (10,0,0 scale 2): local becomes (-3.5,0,0), scale .5.
      expect(model.reparent(['crate'], const DropTarget(parent: 'robot', index: 0)), isTrue);
      expect(doc.history, 1);
      expect(doc.calls.where((c) => c == 'apply').length, 1);
      final tree = model.tree!;
      expect(tree['robot']!.children, ['crate', 'arm', 'head']);
      expect(tree['crate']!.parent, 'robot');
      final t = tree['crate']!.transform!;
      expect(t.position, [-3.5, 0, 0]);
      expect(t.scale, [0.5, 0.5, 0.5]);
      final world = Trs.fromMatrix(tree.worldMatrix('crate'));
      expect(world.position, [3, 0, 0]);
      expect(world.scale, [1, 1, 1]);
      expect(model.selection.toList(), ['crate']);

      // Off: the local transform is kept as is.
      model.toggleKeepWorldTransform();
      expect(model.reparent(['crate'], const DropTarget(parent: null, index: 0)), isTrue);
      expect(tree['crate']!.parent, isNull);
      expect(tree.roots, ['crate', 'cam', 'robot', 'floor']); // index 0 among the roots, as Unity's
      expect(tree['crate']!.transform!.position, [-3.5, 0, 0]);
      expect(doc.history, 2);
    });

    test('keep world transform survives a rotated parent (the affine inverse is a real inverse)', () {
      final (project, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      final doc = project.fakeDocuments.first;
      // Robot turned 90° about Y (quaternion [0, sin 45°, 0, cos 45°]).
      const s = 0.7071067811865476;
      expect(
        tabs.focused!.transaction([
          Command.setField('robot', 't', 'rotation', [0, s, 0, s]),
        ])?.ok,
        isTrue,
      );
      expect(model.reparent(['crate'], const DropTarget(parent: 'robot', index: 0)), isTrue);
      final tree = model.tree!;
      final world = Trs.fromMatrix(tree.worldMatrix('crate'));
      for (var i = 0; i < 3; i++) {
        expect(world.position[i], closeTo([3, 0, 0][i], 1e-6), reason: 'world position stays at (3, 0, 0)');
        expect(world.scale[i], closeTo(1, 1e-6));
      }
      // Local: the crate sits 7 m from the robot along world −X, which in the
      // robot's frame (turned 90° about Y: local +Z → world +X) is −Z, halved
      // by the robot's scale 2.
      final local = tree['crate']!.transform!;
      expect(local.position[0].abs(), lessThan(1e-6));
      expect(local.position[2], closeTo(-3.5, 1e-6));
      expect(doc.history, 2);
    });

    test('a drop onto itself or its descendant is refused; an index counts the vacated slot', () {
      // A prefab lists its roots, so their order is a drop target too.
      final (project, tabs, model) = setUpModel(doc: scene(kind: 'prefab'));
      addTearDown(tabs.dispose);
      expect(model.rootOrderIsEditable, isTrue);
      expect(model.canDrop(['robot'], const DropTarget(parent: 'arm', index: 0)), isFalse);
      expect(model.canDrop(['robot'], const DropTarget(parent: 'robot', index: 0)), isFalse);
      expect(model.canDrop(['arm', 'robot'], const DropTarget(parent: 'floor', index: 0)), isTrue);
      // Move cam (index 0) to after floor (index 2 among roots → the 'below floor' zone is index 3).
      expect(model.reparent(['cam'], const DropTarget(parent: null, index: 3)), isTrue);
      expect(model.tree!.roots, ['robot', 'floor', 'cam', 'crate']);
      expect(project.fakeDocuments.first.history, 1);
    });

    test('keyboard stepping, arrows and select all work over the visible rows', () {
      final (_, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      model.step(1);
      expect(model.selection.toList(), ['cam']);
      model.step(1);
      expect(model.selection.toList(), ['robot']);
      model.arrow(true); // expand
      expect(model.tree!.expanded, contains('robot'));
      model.arrow(true); // first child
      expect(model.selection.toList(), ['arm']);
      model.step(1, extend: true);
      expect(model.selection.toList(), ['arm', 'head']);
      model.arrow(false); // to parent
      expect(model.selection.toList(), ['robot']);
      model.arrow(false); // collapse
      expect(model.tree!.expanded, isNot(contains('robot')));
      model.selectAll();
      expect(model.selection.length, 4);
      expect(model.revealAndSelect('head'), isTrue);
      expect(model.tree!.expanded, contains('robot'));
      expect(model.selection.toList(), ['head']);
      expect(model.revealAndSelect('nobody'), isFalse);
    });
  });

  group('10 000 entities', () {
    test('build once, select and rename without rebuilding; rows stay virtual', () {
      final doc = FakeDocument.large(path: '/fake/game/assets/scenes/big.scene.json', count: 10000, depth: 4);
      final (_, tabs, model) = setUpModel(doc: doc);
      addTearDown(tabs.dispose);
      final sw = Stopwatch()..start();
      final tree = model.tree!;
      expect(tree.entityCount, 10000);
      expect(tree.roots.length, 2500);
      expect(tree.builds, 1);
      final buildMs = sw.elapsedMilliseconds;
      expect(tree.rows.length, 2500); // collapsed roots
      model.expandAll();
      expect(tree.rows.length, 10000);
      final node = tree['e5001']!;
      sw.reset();
      model.select('e5001');
      model.select('e5010', range: true);
      expect(model.selection.length, 10);
      final a = model.rename('e5001', 'Picked');
      expect(a, isTrue);
      final editMs = sw.elapsedMilliseconds;
      expect(tree.builds, 1); // never rebuilt
      expect(tree.patches, greaterThanOrEqualTo(1));
      expect(identical(tree['e5001'], node), isTrue);
      expect(tree['e5001']!.name, 'Picked');
      expect(tree['e5000']!.version, 1); // untouched nodes did not change
      // Generous bounds: a 10k build is well under a second on the fake; the edit is milliseconds.
      expect(buildMs, lessThan(5000));
      expect(editMs, lessThan(2000));
      model.setQuery('Picked');
      expect(tree.rows.where((r) => r.matches).length, 1);
      // The search also matches ids, so "Root 24" finds Root 24, 240–249, 2400–2499 and every root whose id has "24".
      model.setQuery('Root 24');
      final expected = doc.entitiesView.entries
          .where((e) => (e.value['name'] as String).startsWith('Root') && '${e.value['name']} ${e.key}'.contains('24'))
          .length;
      expect(expected, greaterThan(111));
      expect(tree.rows.where((r) => r.matches).length, expected);
    });

    testWidgets('the panel builds only the visible rows', (tester) async {
      final doc = FakeDocument.large(path: '/fake/game/assets/scenes/big.scene.json', count: 10000, depth: 4);
      final (_, tabs, model) = setUpModel(doc: doc);
      addTearDown(tabs.dispose);
      model.expandAll();
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SizedBox(
              height: 480,
              child: HierarchyPanel(model: model, onOpenDocument: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      final built = find.byWidgetPredicate(
        (w) => w.key is ValueKey<String> && (w.key as ValueKey<String>).value.startsWith('row.'),
      );
      expect(built.evaluate().length, lessThan(60));
      expect(built.evaluate().length, greaterThan(15));
      expect(find.byKey(const Key('hierarchy.status')), findsNothing, reason: 'no footer without a search');
      await tester.drag(find.byKey(const Key('hierarchy.list')), const Offset(0, -20000));
      await tester.pumpAndSettle();
      expect(built.evaluate().length, lessThan(60));
    });
  });

  group('panel', () {
    Future<(FakeProject, DocumentTabs, HierarchyModel)> pumpPanel(WidgetTester tester) async {
      final (project, tabs, model) = setUpModel();
      addTearDown(tabs.dispose);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SizedBox(
              height: 600,
              child: HierarchyPanel(model: model, onOpenDocument: () {}),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      return (project, tabs, model);
    }

    testWidgets('rows, expander, badges, search and the status line', (tester) async {
      final (_, _, model) = await pumpPanel(tester);
      expect(find.byKey(const Key('hierarchy.name.robot')), findsOneWidget);
      expect(find.byKey(const Key('hierarchy.name.arm')), findsNothing); // collapsed
      await tester.tap(find.byKey(const Key('hierarchy.expander.robot')));
      await tester.pumpAndSettle();
      expect(find.byKey(const Key('hierarchy.name.arm')), findsOneWidget);
      expect(find.byKey(const Key('hierarchy.prefab.crate')), findsOneWidget);
      expect(find.byKey(const Key('hierarchy.status')), findsNothing, reason: 'no footer without a search');
      await tester.enterText(find.byKey(const Key('hierarchy.search')), 'arm');
      await tester.pumpAndSettle();
      expect(model.filter.query, 'arm');
      expect(find.byKey(const Key('hierarchy.name.arm')), findsOneWidget);
      expect(find.byKey(const Key('hierarchy.name.robot')), findsOneWidget); // ancestor
      expect(find.byKey(const Key('hierarchy.name.floor')), findsNothing);
      expect(find.textContaining('1 match'), findsOneWidget);
    });

    testWidgets(
      'click selects, ⌘-click toggles, double-click does not rename, F2 renames in place, ↩ commits, ⎋ cancels',
      (tester) async {
        final (project, _, model) = await pumpPanel(tester);
        await tester.tap(find.byKey(const Key('hierarchy.name.cam')));
        await tester.pumpAndSettle();
        expect(model.selection.toList(), ['cam']);
        await tester.sendKeyDownEvent(LogicalKeyboardKey.metaLeft);
        await tester.tap(find.byKey(const Key('hierarchy.name.floor')));
        await tester.sendKeyUpEvent(LogicalKeyboardKey.metaLeft);
        await tester.pumpAndSettle();
        expect(model.selection.toList(), ['cam', 'floor']);

        await tester.tap(find.byKey(const Key('hierarchy.name.robot')));
        await tester.pump(const Duration(milliseconds: 50));
        await tester.tap(find.byKey(const Key('hierarchy.name.robot')));
        await tester.pumpAndSettle();
        expect(model.renaming, isNull, reason: 'a double-click frames the object, as in Unity');
        await tester.sendKeyEvent(LogicalKeyboardKey.f2);
        await tester.pumpAndSettle();
        expect(model.renaming, 'robot');
        expect(find.byKey(const Key('hierarchy.rename.robot')), findsOneWidget);
        await tester.enterText(find.byKey(const Key('hierarchy.rename.robot')), 'Bot');
        await tester.testTextInput.receiveAction(TextInputAction.done);
        await tester.pumpAndSettle();
        expect(model.renaming, isNull);
        expect(model.tree!['robot']!.name, 'Bot');
        expect(project.fakeDocuments.first.history, 1);

        // ↩ on the selection starts a rename; ⎋ cancels without a transaction.
        await tester.sendKeyEvent(LogicalKeyboardKey.enter);
        await tester.pumpAndSettle();
        expect(model.renaming, 'robot');
        await tester.sendKeyEvent(LogicalKeyboardKey.escape);
        await tester.pumpAndSettle();
        expect(model.renaming, isNull);
        expect(project.fakeDocuments.first.history, 1);
      },
    );

    testWidgets('⌫ deletes the selection; the arrow keys move it', (tester) async {
      final (project, _, model) = await pumpPanel(tester);
      await tester.tap(find.byKey(const Key('hierarchy.name.floor')));
      await tester.pumpAndSettle();
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowUp);
      await tester.pumpAndSettle();
      expect(model.selection.toList(), ['robot']);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
      await tester.sendKeyEvent(LogicalKeyboardKey.arrowRight);
      await tester.pumpAndSettle();
      expect(model.selection.toList(), ['arm']);
      await tester.sendKeyEvent(LogicalKeyboardKey.backspace);
      await tester.pumpAndSettle();
      expect(model.tree!['arm'], isNull);
      expect(project.fakeDocuments.first.history, 1);
      expect(model.selection, isEmpty);
    });

    testWidgets('drag-and-drop reparents onto a row and orders above/below', (tester) async {
      final (project, _, model) = await pumpPanel(tester);
      // Drag Floor onto Robot's middle → child of Robot, appended.
      final floor = tester.getCenter(find.byKey(const Key('hierarchy.name.floor')));
      final robot = tester.getCenter(find.byKey(const Key('hierarchy.name.robot')));
      final gesture = await tester.startGesture(floor);
      await tester.pump(const Duration(milliseconds: 50));
      await gesture.moveTo(robot);
      await tester.pump(const Duration(milliseconds: 50));
      await gesture.up();
      await tester.pumpAndSettle();
      expect(model.tree!['floor']!.parent, 'robot');
      expect(model.tree!['robot']!.children, ['arm', 'head', 'floor']);
      expect(project.fakeDocuments.first.history, 1);
      expect(model.tree!.expanded, contains('robot'));

      // Drag Crate to the top edge of Arm → before Arm among Robot's children.
      final crate = tester.getCenter(find.byKey(const Key('hierarchy.name.crate')));
      final armTop = tester.getTopLeft(find.byKey(const Key('hierarchy.name.arm'))) + const Offset(20, 1);
      final g2 = await tester.startGesture(crate);
      await tester.pump(const Duration(milliseconds: 50));
      await g2.moveTo(armTop);
      await tester.pump(const Duration(milliseconds: 50));
      await g2.up();
      await tester.pumpAndSettle();
      expect(model.tree!['robot']!.children, ['crate', 'arm', 'head', 'floor']);
      expect(project.fakeDocuments.first.history, 2);

      // A scene's roots are ordered too (as Unity's): the top edge of Camera is "before Camera".
      final head = tester.getCenter(find.byKey(const Key('hierarchy.name.head')));
      final camTop = tester.getTopLeft(find.byKey(const Key('hierarchy.name.cam'))) + const Offset(20, 1);
      final g3 = await tester.startGesture(head);
      await tester.pump(const Duration(milliseconds: 50));
      await g3.moveTo(camTop);
      await tester.pump(const Duration(milliseconds: 50));
      await g3.up();
      await tester.pumpAndSettle();
      expect(model.tree!['head']!.parent, isNull);
      expect(model.tree!.roots, ['head', 'cam', 'robot']);
      expect(project.fakeDocuments.first.history, 3);
    });

    testWidgets('the empty state offers Open Document', (tester) async {
      final project = FakeProject(dir: '/fake/game');
      final tabs = DocumentTabs(project: project);
      addTearDown(tabs.dispose);
      final model = HierarchyModel(tabs: tabs, project: project);
      var opens = 0;
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: HierarchyPanel(model: model, onOpenDocument: () => opens++),
          ),
        ),
      );
      await tester.tap(find.byKey(const Key('hierarchy.openDocument')));
      expect(opens, 1);
    });
  });
}
