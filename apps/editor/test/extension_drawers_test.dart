// Editor extensions (ADR-0028 step 4): property drawers. A record class's
// drawer draws every field holding one — alone and as a list's items — and
// a named one every field `@Expose(drawer:)` names; their writes go the
// default Inspector's way (prechecked, one undo step), and a throwing one
// falls back to the default editor with a line and one Console error.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart' show Severity;
import 'package:fynkel/fynkel.dart' show BehaviourRegistry, DataAsset, DataType;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor/src/play/editor_game.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import 'parity/harness.dart';
import 'support/fakes.dart';

class Wave {}

class CoinKind extends DataAsset {}

class Sparks {}

/// code_only's coin kind: a custom Inspector over the default fields, and
/// a drawer for its Sparks record — the drawer inside the Inspector's
/// defaultInspector, on a data asset's page.
void _coins(EditorExtensions editor) {
  editor.inspector<CoinKind>(
    (context, inspector) => Column(children: [const Text('coin kind page'), inspector.defaultInspector()]),
  );
  editor.drawer<Sparks>(
    (context, field) => field.row(
      TextButton(
        key: const Key('sparks.fewer'),
        onPressed: () => field.setField('count', 3),
        child: Text('${field.valueOf('count')} sparks', key: const Key('sparks.text')),
      ),
    ),
  );
}

const _bigCoin = '6d1f0a52-0000-4000-8000-00000000c014';

/// code_only's real CoinKind schema and its big coin.
void _coinKinds(Directory dir) {
  File('../../examples/code_only/schemas/data/game.CoinKind.schema.json')
      .copySync('${dir.path}/schemas/data/game.CoinKind.schema.json');
  File('${dir.path}/assets/data/big_coin.data.json').writeAsStringSync(
    const JsonEncoder.withIndent('  ').convert({
      r'$schema': 'https://fynkel.dev/schemas/data.schema.json',
      'formatVersion': 1,
      'id': _bigCoin,
      'name': 'Big Coin',
      'type': 'game.CoinKind',
      'version': 1,
      'data': {
        'value': 5,
        'sparks': {'count': 12, 'seconds': 1.6, 'size': 0.24},
      },
    }),
  );
}

var _throws = false;

void _drawers(EditorExtensions editor) {
  editor.drawer<Wave>((context, field) {
    if (_throws) throw StateError('no wave');
    final count = field.valueOf('count') as num? ?? 0;
    return Column(
      children: [
        field.row(
          Wrap(
            children: [
              Text('${field.label}: $count after ${field.valueOf('delay')} s', key: Key('wave.${field.name}')),
              TextButton(
                key: Key('wave.more.${field.name}'),
                onPressed: () => field.setField('count', count + 1),
                child: const Text('+1'),
              ),
              TextButton(
                key: Key('wave.none.${field.name}'),
                onPressed: () => field.setField('count', 0),
                child: const Text('0'),
              ),
            ],
          ),
        ),
        field.field('delay'),
      ],
    );
  });
  editor.namedDrawer('minMax', (context, field) {
    final v = field.value as List;
    return field.row(
      TextButton(
        key: const Key('minMax.widen'),
        onPressed: () => field.set([v[0], (v[1] as num) * 2]),
        child: Text('from ${v[0]} to ${v[1]}', key: const Key('minMax.text')),
      ),
    );
  });
  editor.namedDrawer('minMax', (context, field) => const SizedBox());
}

/// The generator's own `game.Spawner` (`boss`, a `Wave`; `waves`, a list of
/// them), with a `range` that names the `minMax` drawer.
Map<String, Object?> spawnerSchema() {
  final s = jsonDecode(
    File('../../packages/fynkel_generator/example/spinner_game/schemas/behaviours/game.Spawner.schema.json')
        .readAsStringSync(),
  ) as Map<String, Object?>;
  (s['properties'] as Map)['range'] = {
    'type': 'array',
    'items': {'type': 'number'},
    'minItems': 2,
    'maxItems': 2,
    'x-fynkel': {'drawer': 'minMax'},
  };
  return s;
}

FakeDocument _scene() => FakeDocument(
  path: '/fake/game/assets/scenes/main.scene.json',
  relativePath: 'scenes/main.scene.json',
  id: 'main',
  name: 'Main',
  entities: {
    'gate': FakeDocument.entity(
      'Gate',
      components: {
        's': {
          'type': 'game.Spawner',
          'version': 1,
          'data': {
            'boss': {'count': 1, 'delay': 4.0},
            'waves': [
              {'count': 3, 'delay': 2.0},
            ],
            'range': [1, 5],
          },
        },
      },
    ),
  },
);

void main() {
  setUp(() => _throws = false);

  Future<(EditorShellState, FakeProject)> openShell(WidgetTester tester) async {
    final project = FakeProject(dir: '/fake/game')..addDocument(_scene());
    project.schemas = [
      TypeSchema(type: 'game.Spawner', isBehaviour: true, authored: true, allowed: true, schema: spawnerSchema()),
    ];
    await pumpLarge(
      tester,
      shellApp(
        prefs: Preferences.inMemory(),
        projects: {'/fake/game': project},
        initialProject: '/fake/game',
        extensions: const [EditorPart('game', _drawers)],
      ),
    );
    final shell = tester.state<EditorShellState>(find.byType(EditorShell));
    shell.openDocument('main')!.setSelection(['gate']);
    await tester.pumpAndSettle();
    return (shell, project);
  }

  List<String> errors(EditorShellState shell) => [
    for (final d in shell.extensions.log)
      if (d.severity == Severity.error) d.message,
  ];

  Map<String, Object?> data(FakeProject project) =>
      ((project.fakeDocuments.single.entitiesView['gate']!['components'] as Map)['s'] as Map)['data']
          as Map<String, Object?>;

  testWidgets('a record\'s drawer draws it alone and as a list\'s items; its writes are prechecked undo steps', (
    tester,
  ) async {
    final (shell, project) = await openShell(tester);
    expect(errors(shell).single, contains('drawer "minMax" left out: another extension already registered it'));

    expect(find.text('Boss: 1 after 4.0 s'), findsOneWidget);
    expect(find.text('Item 0: 3 after 2.0 s'), findsOneWidget);
    expect(find.byKey(const ValueKey('foldout.game.Spawner#0.boss')), findsNothing, reason: 'in place of the foldout');
    // field('delay'): the standard editor, at the record's path.
    expect(find.byKey(const ValueKey('field.game.Spawner#0.boss.delay')), findsOneWidget);

    await tester.tap(find.byKey(const Key('wave.more.boss')));
    await tester.pumpAndSettle();
    expect(data(project)['boss'], {'count': 2, 'delay': 4.0});
    await tester.tap(find.byKey(const Key('wave.more.0')));
    await tester.pumpAndSettle();
    expect((data(project)['waves'] as List).single, {'count': 4, 'delay': 2.0});

    // Below the record's minimum: refused, nothing written.
    await tester.tap(find.byKey(const Key('wave.none.boss')));
    await tester.pumpAndSettle();
    expect((data(project)['boss'] as Map)['count'], 2);

    final tab = shell.tabs!.focused!;
    expect(tab.undo(), isTrue);
    expect(tab.undo(), isTrue);
    expect(data(project)['boss'], {'count': 1, 'delay': 4.0});
  });

  testWidgets('a named drawer draws the field @Expose(drawer:) names, before a record\'s', (tester) async {
    final (shell, project) = await openShell(tester);
    expect(find.text('from 1 to 5'), findsOneWidget);
    await tester.tap(find.byKey(const Key('minMax.widen')));
    await tester.pumpAndSettle();
    expect(data(project)['range'], [1, 10]);
    expect(shell.extensions.drawerFor(named: 'minMax', record: 'Wave')!.name, 'minMax');
    expect(shell.extensions.drawerFor(named: 'nope', record: 'Wave')!.record, 'Wave');
  });

  testWidgets('a throwing drawer: the default editor, a line saying why, one Console error', (tester) async {
    _throws = true;
    final (shell, _) = await openShell(tester);
    expect(find.byKey(const Key('drawer.error')), findsNWidgets(2));
    expect(find.byKey(const ValueKey('foldout.game.Spawner#0.boss')), findsOneWidget);
    final failed = errors(shell).where((e) => e.contains('drawer<Wave> failed')).toList();
    expect(failed, hasLength(1));
    expect(failed.single, contains('so the field is drawn the default way: Bad state: no wave'));
  });

  testWidgets('a record\'s drawer on a data asset\'s page, inside a custom Inspector\'s default fields', (
    tester,
  ) async {
    final e = await Editor.open(
      tester,
      tag: 'sparks',
      prepare: (dir) {
        Directory('${dir.path}/schemas/data').createSync(recursive: true);
        Directory('${dir.path}/assets/data').createSync(recursive: true);
        _coinKinds(dir);
      },
      game: EditorGame(
        name: 'game',
        projectDir: '/elsewhere',
        build: (_) => const SizedBox(),
        registerBehaviours: (BehaviourRegistry r) =>
            r.registerData(DataType(id: 'game.CoinKind', create: CoinKind.new)),
      ),
      extensions: const [EditorPart('game', _coins)],
    );
    e.shell.browser!.select(_bigCoin);
    await tester.pumpAndSettle();
    expect(find.text('coin kind page'), findsOneWidget);
    expect(find.text('12 sparks'), findsOneWidget);

    await tester.tap(find.byKey(const Key('sparks.fewer')));
    await tester.pumpAndSettle();
    final data = (jsonDecode(File('${e.dir.path}/assets/data/big_coin.data.json').readAsStringSync()) as Map)['data'];
    expect(data['sparks'], {'count': 3, 'seconds': 1.6, 'size': 0.24});
    expect(find.text('3 sparks'), findsOneWidget);
  });
}
