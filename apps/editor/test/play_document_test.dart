// Live Play mode L2 (ADR-0021 P7): the running game as a document — the
// scene's ids and component ids, what the game made under `runtime:` ids,
// only what changed reported on a refresh — and the Hierarchy and the
// Inspector on it while Play runs, with the selection carried both ways.

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' as fynkel;
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/documents/document_tabs.dart';
import 'package:fynkel_editor/src/documents/editor_document.dart';
import 'package:fynkel_editor/src/hierarchy/hierarchy_model.dart';
import 'package:fynkel_editor/src/inspector/inspector_model.dart';
import 'package:fynkel_editor/src/inspector/field_widgets.dart';
import 'package:fynkel_editor/src/inspector/inspector_panel.dart';
import 'package:fynkel_editor/src/play/editor_game.dart';
import 'package:fynkel_editor/src/play/live_preview.dart';
import 'package:fynkel_editor/src/play/live_play.dart';
import 'package:fynkel_editor/src/play/play_document.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart' show FynkelLaunch, FynkelPlayControl, FynkelPlayScope;

import 'inspector_test.dart' show nativeSchemas;
import 'support/fakes.dart';

/// A running world in memory: rows, their native components, behaviours.
final class FakeLive extends LiveBackend {
  final rows = <LiveEntity>[];
  final native = <String, List<({String type, Map<String, Object?> data})>>{};
  final scripts = <int, List<LiveBehaviour>>{};
  int entityReads = 0;

  void add(String id, String name, {String? parent, int? handle, List<String> types = const ['fynkel.Transform']}) {
    rows.add(
      LiveEntity(
        id: id,
        handle: handle ?? rows.length + 1,
        name: name,
        parent: parent,
        spawned: id.startsWith('runtime:'),
        layer: 0,
        childCount: 0,
        types: types,
      ),
    );
    native[id] = [
      for (final t in types)
        (
          type: t,
          data: t == 'fynkel.Transform'
              ? {
                  'position': [0, 0, 0],
                  'rotation': [0, 0, 0, 1],
                  'scale': [1, 1, 1],
                }
              : <String, Object?>{},
        ),
    ];
  }

  void rename(String id, String name) {
    final i = rows.indexWhere((r) => r.id == id);
    final r = rows[i];
    rows[i] = LiveEntity(
      id: r.id,
      handle: r.handle,
      name: name,
      parent: r.parent,
      spawned: r.spawned,
      layer: r.layer,
      childCount: r.childCount,
      types: r.types,
    );
  }

  @override
  List<LiveEntity> entities() => List.of(rows);

  @override
  List<LiveEntity>? changedEntities() => entities();

  @override
  PreviewBackend? openPreview() => null;

  @override
  LiveEntityState entity(String id) {
    entityReads++;
    final r = rows.firstWhere((r) => r.id == id);
    return LiveEntityState(id: id, handle: r.handle, name: r.name, parent: r.parent, world: const {}, components: native[id]!);
  }

  @override
  List<LiveBehaviour> behaviours(int handle) => scripts[handle] ?? const [];

  /// Every write, as `kind id` (L3).
  final writes = <String>[];

  /// What the next write answers (the world's refusals).
  List<String> refuse = const [];

  @override
  List<String> write(String kind, Map<String, Object?> args) {
    final id = args['id'] as String;
    writes.add('$kind $id');
    if (refuse.isNotEmpty) return refuse;
    final i = rows.indexWhere((r) => r.id == id);
    final r = rows[i];
    LiveEntity row({String? name, String? parent, List<String>? types}) => LiveEntity(
      id: r.id,
      handle: r.handle,
      name: name ?? r.name,
      parent: args.containsKey('parent') ? parent : r.parent,
      spawned: r.spawned,
      layer: r.layer,
      childCount: r.childCount,
      types: types ?? r.types,
    );
    switch (kind) {
      case 'set':
        final type = args['type'] as String;
        final data = (args['data'] as Map).cast<String, Object?>();
        final list = native[id]!..removeWhere((c) => c.type == type);
        list.add((type: type, data: data));
        rows[i] = row(types: [for (final c in list) c.type]);
      case 'remove':
        final list = native[id]!..removeWhere((c) => c.type == args['type']);
        rows[i] = row(types: [for (final c in list) c.type]);
      case 'rename':
        rename(id, args['name'] as String);
      case 'parent':
        rows[i] = row(parent: args['parent'] as String?);
      case 'destroy':
        rows.removeWhere((e) => e.id == id || e.parent == id);
    }
    return const [];
  }

  @override
  List<String> setBehaviourField(Object instance, String field, Object? value) {
    writes.add('setBehaviourField $field');
    (instance as Map<String, Object?>)[field] = value;
    return const [];
  }

  @override
  List<String> addBehaviour(int handle, String type, {int? version, Map<String, Object?> data = const {}}) {
    writes.add('addBehaviour $handle $type');
    final fields = {...data};
    (scripts[handle] ??= []).add(LiveBehaviour(type: type, data: fields, instance: fields));
    return const [];
  }

  @override
  List<String> removeBehaviour(Object instance) {
    writes.add('removeBehaviour');
    for (final list in scripts.values) {
      list.removeWhere((b) => identical(b.instance, instance));
    }
    return const [];
  }
}

FakeDocument scene() => FakeDocument(
  path: '/fake/game/assets/scenes/main.scene.json',
  id: 'main',
  name: 'Main',
  entities: {
    'robot': FakeDocument.entity(
      'Robot',
      components: {
        'light': {'type': 'fynkel.PointLight', 'version': 1, 'data': <String, Object?>{}},
      },
    ),
    'floor': FakeDocument.entity('Floor'),
  },
  roots: ['robot', 'floor'],
);

FakeLive running() {
  final spinner = <String, Object?>{'speed': 2.5};
  return FakeLive()
    ..add('robot', 'Robot', handle: 7, types: ['fynkel.Transform', 'fynkel.PointLight'])
    ..add('floor', 'Floor')
    ..add('runtime:99', 'Bullet', handle: 99)
    ..scripts[7] = [
      LiveBehaviour(type: 'game.Spinner', data: spinner, authoredComponentId: 'spin', instance: spinner),
    ];
}

/// A game that shows a (real, empty) world the way `FynkelGameView` does:
/// registered with the editor's play scope.
class _WorldGame extends StatefulWidget {
  const _WorldGame(this.launch);

  final FynkelLaunch launch;

  @override
  State<_WorldGame> createState() => _WorldGameState();
}

class _WorldGameState extends State<_WorldGame> {
  final world = fynkel.World.create();
  FynkelPlayControl? _play;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _play = FynkelPlayScope.maybeOf(context)?..attach(world);
  }

  @override
  void dispose() {
    _play?.detach(world);
    world.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => const SizedBox.expand();
}

void main() {
  test('the running world reads as the scene does: its ids, its component ids, the game\'s own apart', () {
    final live = running();
    final doc = PlayDocument(backend: live, scene: scene(), interval: const Duration(days: 1));
    addTearDown(doc.dispose);
    expect(doc.roots(), ['robot', 'floor', 'runtime:99']);
    expect(doc.readOnly, isNull, reason: 'the running game is edited (L3)');
    expect(doc.dirty, isFalse);

    final robot = doc.entities(['robot']).entities['robot']!;
    final components = robot['components'] as Map<String, Object?>;
    expect(components.keys, ['t', 'light', 'spin'], reason: "the scene's ids, and the behaviour's authored one");
    expect((components['spin'] as Map)['data'], {'speed': 2.5});
    final bullet = doc.entities(['runtime:99']).entities['runtime:99']!;
    expect((bullet['components'] as Map).keys, ['runtime:99/fynkel.Transform']);
    expect(doc.entities(['gone']).entities['gone'], isNull);
  });

  test('edits go to the running world at once: fields, components, names, parents, deletes (L3)', () {
    final live = running();
    var written = 0;
    final doc = PlayDocument(backend: live, scene: scene(), written: () => written++, interval: const Duration(days: 1));
    addTearDown(doc.dispose);
    Map<String, Object?> components(String id) => doc.entities([id]).entities[id]!['components'] as Map<String, Object?>;
    Object? data(String id, String component) => (components(id)[component] as Map)['data'];

    final revision = doc.revision;
    var a = doc.apply([Command.setField('robot', 't', '/position/1', 3)]);
    expect(a.ok, isTrue);
    expect(live.writes, ['set robot']);
    expect((data('robot', 't') as Map)['position'], [0, 3, 0], reason: 'a pointer writes into the value');
    expect((data('robot', 't') as Map)['scale'], [1, 1, 1], reason: 'the rest is kept');
    expect(doc.revision, isNot(revision));
    expect(doc.lastEdit!.affectedIds, contains('robot'));
    expect(written, 1, reason: 'a paused Game view draws the write');

    expect(doc.apply([Command.setField('robot', 'spin', 'speed', 9)]).ok, isTrue);
    expect(live.writes.last, 'setBehaviourField speed');
    expect(data('robot', 'spin'), {'speed': 9});

    doc.apply([
      Command.addComponent('floor', {'type': 'fynkel.PointLight', 'version': 1, 'data': {'range': 4}}),
      Command.addComponent('floor', {'type': 'game.Spinner', 'version': 2, 'data': {'speed': 1}}),
    ]);
    expect(live.writes.sublist(live.writes.length - 2), ['set floor', 'addBehaviour 2 game.Spinner']);
    expect(components('floor').keys, containsAll(['floor/fynkel.PointLight', 'floor/game.Spinner']));
    doc.apply([Command.removeComponent('robot', 'light'), Command.removeComponent('robot', 'spin')]);
    expect(live.writes.sublist(live.writes.length - 2), ['remove robot', 'removeBehaviour']);
    expect(components('robot').keys, ['t']);

    doc.apply([Command.renameObject('robot', 'Hero')]);
    expect(doc.entities(['robot']).entities['robot']!['name'], 'Hero');
    doc.apply([Command.reparentObject('runtime:99', parent: 'robot', index: 0)]);
    expect(live.writes.last, 'parent runtime:99');
    expect(doc.roots(), ['robot', 'floor']);
    expect(doc.entities(['robot']).entities['robot']!['children'], ['runtime:99']);
    expect(doc.lastEdit!.affectedIds, containsAll(['runtime:99', 'robot']));
    doc.apply([Command.deleteObject('robot')]);
    expect(doc.roots(), ['floor']);
    expect(doc.entities(['runtime:99']).entities['runtime:99'], isNull, reason: 'its subtree went with it');

    final writes = live.writes.length;
    a = doc.apply([Command.createObject(name: 'New')]);
    expect(a.ok, isFalse, reason: 'not while the game runs (yet)');
    expect(doc.apply([Command.renameObject('robot', 'x')]).ok, isFalse, reason: 'gone');
    expect(doc.apply([Command.setField('floor', 'nope', 'x', 1)]).ok, isFalse);
    expect(live.writes.length, writes);
    live.refuse = ['no'];
    a = doc.apply([Command.renameObject('floor', 'x'), Command.renameObject('floor', 'y')]);
    expect(a.ok, isFalse);
    expect(a.diagnostics.single.message, 'no');
    expect(live.writes.length, writes + 1, reason: 'the first refusal stops the rest');
  });

  test('a real running world takes a field, and a gizmo drag moves it and puts it back (L3)', () {
    final world = fynkel.World.create();
    addTearDown(world.dispose);
    world.spawn(name: 'Box');
    world.step();
    final backend = WorldLiveBackend(world);
    var written = 0;
    final doc = PlayDocument(backend: backend, written: () => written++, interval: const Duration(days: 1));
    addTearDown(doc.dispose);
    final id = doc.roots().single;
    final transform = (doc.entities([id]).entities[id]!['components'] as Map).keys.single as String;
    expect(doc.apply([Command.setField(id, transform, '/position/0', 5)]).ok, isTrue);
    List<Object?> position() => backend.live.entity(id).component('fynkel.Transform')!['position']! as List<Object?>;
    expect(position().first, 5);
    expect(written, 1);

    final preview = doc.openPreview() as LivePreview;
    preview.setTransforms([
      PreviewTransform(id: id, position: const [1, 2, 3], rotation: const [0, 0, 0, 1], scale: const [1, 1, 1]),
    ]);
    expect(position(), [1, 2, 3]);
    preview.restoreTransforms([id]);
    expect(position(), [5, 0, 0], reason: 'the drag puts it back; its transaction writes the end');
    expect(written, 3);
    expect(doc.apply([Command.renameObject(id, 'Crate')]).ok, isTrue);
    expect(backend.live.entity(id).name, 'Crate');
  });

  test('a refresh reports only what changed; a watched entity\'s moving values go to `values` alone', () {
    final live = running();
    final doc = PlayDocument(backend: live, scene: scene(), interval: const Duration(days: 1));
    addTearDown(doc.dispose);
    var told = 0;
    doc.addListener(() => told++);
    doc.watched = () => ['floor'];

    doc.refresh();
    expect(told, 0, reason: 'nothing moved');

    live.rename('robot', 'Hero');
    live.add('runtime:100', 'Coin', handle: 100);
    doc.refresh();
    expect(told, 1);
    expect(doc.lastEdit!.affectedIds, unorderedEquals(['robot', 'runtime:100']));
    expect(doc.entities(['robot']).entities['robot']!['name'], 'Hero');

    final edit = doc.lastEdit;
    live.native['floor']![0].data['position'] = [0, 3, 0];
    doc.refresh();
    expect(told, 1, reason: 'no row changed: the tab (and the Hierarchy) hear nothing');
    expect(doc.lastEdit, same(edit));
    expect(doc.values.value, 1);
    expect(doc.lastMoved, ['floor']);
    expect(((doc.entities(['floor']).entities['floor']!['components'] as Map).values.first as Map)['data'], {
      'position': [0, 3, 0],
      'rotation': [0, 0, 0, 1],
      'scale': [1, 1, 1],
    });

    live.rows.removeWhere((r) => r.id == 'runtime:99');
    doc.refresh();
    expect(doc.lastEdit!.affectedIds, ['runtime:99']);
    expect(doc.roots(), isNot(contains('runtime:99')));
    expect(doc.refreshes, 4);
  });

  testWidgets('during Play the Hierarchy and the Inspector show the running game; Stop brings the scene back', (
    tester,
  ) async {
    final project = FakeProject(dir: '/fake/game')..addDocument(scene());
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final hierarchy = HierarchyModel(tabs: tabs, project: project);
    final inspector = InspectorModel(tabs: tabs, project: project);
    final main = tabs.openDocument('main')!..setSelection(['robot']);

    final live = running();
    final play = LivePlay(tabs: tabs, backend: (_) => live);
    final world = fynkel.World.create();
    addTearDown(world.dispose);

    play.follow(world);
    final shown = tabs.live!;
    expect(hierarchy.tab, same(shown));
    expect(inspector.tab, same(shown));
    expect(hierarchy.canEdit, isTrue, reason: 'the running game is edited (L3)');
    expect(tabs.focused, same(main), reason: 'the scene tab is still the focused tab');
    expect(shown.selection, {'robot'}, reason: 'the selection came along');
    expect(hierarchy.tree!.contains('runtime:99'), isTrue);
    expect(inspector.single?.id, 'robot');

    // The game renames the robot: the Hierarchy row and the Inspector follow.
    live.rename('robot', 'Hero');
    play.document!.refresh();
    expect(hierarchy.tree!['robot']!.name, 'Hero');
    expect(inspector.single?.name, 'Hero');

    shown.setSelection(['floor', 'runtime:99']);
    play.follow(null);
    expect(tabs.live, isNull);
    expect(hierarchy.tab, same(main));
    expect(main.selection, {'floor'}, reason: "back to the scene, without what the game made");
    expect(main.tree['robot']!.name, 'Robot', reason: 'nothing of the run reached the document');
    play.dispose();
  });

  testWidgets('a scene the game loads after its world shows up still gets the selection', (tester) async {
    final project = FakeProject(dir: '/fake/game')..addDocument(scene());
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    tabs.openDocument('main')!.setSelection(['robot']);
    // The world starts on another scene (a menu).
    final live = FakeLive()..add('runtime:1', 'Menu camera');
    final play = LivePlay(tabs: tabs, backend: (_) => live);
    final world = fynkel.World.create();
    addTearDown(world.dispose);

    play.follow(world);
    expect(tabs.live!.selection, isEmpty);
    live.rows.clear();
    live
      ..add('robot', 'Robot', handle: 7)
      ..add('floor', 'Floor');
    play.document!.refresh();
    expect(tabs.live!.selection, {'robot'}, reason: 'carried once the robot is there');

    // Once carried (or once the user picked something), it is not forced again.
    tabs.live!.setSelection(['floor']);
    live.rename('robot', 'Hero');
    play.document!.refresh();
    expect(tabs.live!.selection, {'floor'});
    play.dispose();
  });

  testWidgets('a moving value rebuilds its field alone: the other cards and fields keep their widgets', (tester) async {
    final project = FakeProject(dir: '/fake/game')..addDocument(scene());
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final inspector = InspectorModel(tabs: tabs, project: project);
    addTearDown(inspector.dispose);
    final hierarchy = HierarchyModel(tabs: tabs, project: project);
    addTearDown(hierarchy.dispose);
    tabs.openDocument('main')!.setSelection(['robot']);
    final live = running();
    final play = LivePlay(tabs: tabs, backend: (_) => live);
    final world = fynkel.World.create();
    addTearDown(world.dispose);
    play.follow(world);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: SizedBox(width: 420, height: 1200, child: InspectorPanel(model: inspector))),
      ),
    );
    final generation = inspector.generation;
    final slots = List.of(inspector.slots);
    Widget card(String key) => tester.widget(find.byKey(ValueKey('card.$key')));
    Widget field(String key) => tester.widget(find.byKey(ValueKey('field.$key')));
    final light = card('fynkel.PointLight#0');
    final rotation = field('fynkel.Transform#0.rotation');
    final position = field('fynkel.Transform#0.position');
    var hierarchyTold = 0;
    hierarchy.addListener(() => hierarchyTold++);

    live.native['robot']![0].data['position'] = [0, 2, 0];
    play.document!.refresh();
    await tester.pump();
    expect(hierarchyTold, 0);
    expect(inspector.generation, generation, reason: 'only values moved');
    expect(inspector.slots[1], same(slots[1]), reason: 'the light did not change');
    expect(inspector.slots[0], isNot(same(slots[0])));
    expect(card('fynkel.PointLight#0'), same(light));
    expect(field('fynkel.Transform#0.rotation'), same(rotation));
    expect(field('fynkel.Transform#0.position'), isNot(same(position)));
    expect(inspector.slots[0].value('position'), [0, 2, 0]);
    play.dispose();
  });

  testWidgets('a moving value reaches its text box without a rebuild, never under the user\'s typing', (tester) async {
    final project = FakeProject(dir: '/fake/game')..addDocument(scene());
    project.schemas = nativeSchemas();
    final tabs = DocumentTabs(project: project);
    addTearDown(tabs.dispose);
    final inspector = InspectorModel(tabs: tabs, project: project);
    addTearDown(inspector.dispose);
    tabs.openDocument('main')!.setSelection(['robot']);
    final live = running();
    final play = LivePlay(tabs: tabs, backend: (_) => live);
    final world = fynkel.World.create();
    addTearDown(world.dispose);
    play.follow(world);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: SizedBox(width: 420, height: 1200, child: InspectorPanel(model: inspector))),
      ),
    );
    Finder box(String field, String axis) => find.descendant(
      of: find.descendant(of: find.byKey(ValueKey('field.fynkel.Transform#0.$field')), matching: find.byKey(ValueKey(axis))),
      matching: find.byType(EditableText),
    );
    String text(String field, String axis) => tester.widget<EditableText>(box(field, axis)).controller.text;
    final position = tester.widget(find.byKey(const ValueKey('field.fynkel.Transform#0.position')));
    var told = 0;
    inspector.addListener(() => told++);
    final transform = live.native['robot']![0].data;

    transform['position'] = [1.5, 2, 0];
    transform['rotation'] = [0, 0.7071067811865476, 0, 0.7071067811865476];
    play.document!.refresh();
    await tester.pump();
    expect(told, 0, reason: 'only followed values moved: the panel is not told');
    expect(tester.widget(find.byKey(const ValueKey('field.fynkel.Transform#0.position'))), same(position));
    expect(text('position', 'vec.0'), '1.5');
    expect(text('position', 'vec.1'), '2');
    expect(text('rotation', 'euler.1'), '90');

    // A value no field follows (a new name) rebuilds as before.
    live.rename('robot', 'Hero');
    play.document!.refresh();
    await tester.pump();
    expect(told, 1);
    expect(text('position', 'vec.0'), '1.5');
    play.dispose();
  });

  testWidgets('a followed number never changes under the user\'s typing, and comes back on Escape', (tester) async {
    final value = ValueNotifier<num?>(2);
    addTearDown(value.dispose);
    final commits = <Object>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SizedBox(width: 300, child: NumberField(value: 2, live: value, label: 'Y', onCommit: commits.add)),
        ),
      ),
    );
    final box = find.byType(EditableText);
    String text() => tester.widget<EditableText>(box).controller.text;
    value.value = 3.25;
    await tester.pump();
    expect(text(), '3.25');

    await tester.tap(box);
    await tester.pump();
    await tester.enterText(box, '4');
    value.value = 7;
    await tester.pump();
    expect(text(), '4', reason: 'the edit wins');
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(text(), '7', reason: 'cancelled: the running value comes back');
    expect(commits, isEmpty);

    await tester.tap(box);
    await tester.pump();
    expect(text(), '7', reason: 'an edit starts from the newest value');
    await tester.enterText(box, '5');
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pump();
    expect(commits, [5]);
  });

  testWidgets('Play tints the editor and the Hierarchy says it shows the running game; Stop ends both', (tester) async {
    final prefs = Preferences.inMemory();
    final project = FakeProject(dir: '/fake/game')..addDocument(scene());
    await pumpLarge(
      tester,
      shellApp(
        prefs: prefs,
        projects: {'/fake/game': project},
        initialProject: '/fake/game',
        runner: RecordingRunner(),
        game: EditorGame(name: 'fake_game', projectDir: '/fake/game', build: _WorldGame.new),
      ),
    );
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    shell.openDocument('main');
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('shell.playTint')), findsNothing);

    shell.registry.execute('play.start');
    await tester.pumpAndSettle();
    expect(shell.tabs!.live, isNotNull, reason: 'the game registered its world');
    expect(find.byKey(const Key('shell.playTint')), findsOneWidget);
    expect(find.byKey(const Key('hierarchy.playing')), findsOneWidget);

    prefs.setPlayModeTint(false);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('shell.playTint')), findsNothing, reason: 'off in Preferences');
    expect(find.byKey(const Key('hierarchy.playing')), findsOneWidget);

    await shell.stopPlaying();
    await tester.pumpAndSettle();
    expect(shell.tabs!.live, isNull);
    expect(find.byKey(const Key('hierarchy.playing')), findsNothing);
  });
}
