// Editor extensions (ADR-0028 step 3): a package's custom Inspector draws
// a script's card and a data type's page in place of the default one; its
// writes go the default Inspector's way (prechecked, one undo step), and
// a throwing one falls back to the default fields with a line and one
// Console error.
import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/fynkel.dart' show Behaviour, BehaviourRegistry, BehaviourType, DataAsset, DataType;
import 'package:fynkel_authoring/fynkel_authoring.dart' show Severity;
import 'package:fynkel_editor/src/app.dart';
import 'package:fynkel_editor/src/extensions/editor_extensions.dart';
import 'package:fynkel_editor/src/play/editor_game.dart';
import 'package:fynkel_editor/src/prefs/preferences.dart';
import 'package:fynkel_editor/src/project/editor_project.dart';
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

import 'parity/data_asset_parity_test.dart' show sword, weapons;
import 'parity/harness.dart';
import 'support/fakes.dart';

class Collectible extends Behaviour {}

class Weapon extends DataAsset {}

class Stray extends Behaviour {}

void registerGame(BehaviourRegistry r) {
  r.register(BehaviourType(id: 'game.Collectible', create: Collectible.new));
  r.registerData(DataType(id: 'game.Weapon', create: Weapon.new));
}

EditorGame game(String dir) =>
    EditorGame(name: 'game', projectDir: dir, build: (_) => const SizedBox(), registerBehaviours: registerGame);

var _throws = false;

void _coins(EditorExtensions editor) {
  editor.inspector<Collectible>((context, inspector) {
    if (_throws) throw StateError('no coin');
    final value = inspector.value('value');
    return Column(
      children: [
        Text('worth $value on ${inspector.targets}', key: const Key('coin.worth')),
        TextButton(
          key: const Key('coin.double'),
          onPressed: () => inspector.set('value', (value as num) * 2),
          child: const Text('Double'),
        ),
        TextButton(key: const Key('coin.zero'), onPressed: () => inspector.set('value', 0), child: const Text('Zero')),
        inspector.field('label'),
      ],
    );
  });
  editor.inspector<Stray>((context, inspector) => const SizedBox());
  editor.inspector<Weapon>(
    (context, inspector) => Column(
      children: [
        Text('damage ${inspector.value('damage') ?? 1}', key: const Key('weapon.damage')),
        TextButton(
          key: const Key('weapon.sharpen'),
          onPressed: () => inspector.set('damage', 9),
          child: const Text('+'),
        ),
        inspector.defaultInspector(except: const ['damage']),
      ],
    ),
  );
}

Map<String, Object?> collectibleSchema() => {
  'title': 'game.Collectible',
  'type': 'object',
  'properties': {
    'value': {
      'type': 'integer',
      'minimum': 1,
      'default': 1,
      'x-fynkel': {'type': 'integer', 'min': 1},
    },
    'label': {
      'type': 'string',
      'default': '',
      'x-fynkel': {'type': 'string'},
    },
  },
  'additionalProperties': false,
};

Map<String, Object?> collectible(int value) => {
  'type': 'game.Collectible',
  'version': 1,
  'data': {'value': value, 'label': 'gold'},
};

FakeDocument _scene() => FakeDocument(
  path: '/fake/game/assets/scenes/main.scene.json',
  relativePath: 'scenes/main.scene.json',
  id: 'main',
  name: 'Main',
  entities: {
    'a': FakeDocument.entity('Coin A', components: {'c': collectible(1)}),
    'b': FakeDocument.entity('Coin B', components: {'c': collectible(3)}),
  },
);

void main() {
  setUp(() => _throws = false);

  Future<(EditorShellState, FakeProject)> openShell(WidgetTester tester) async {
    final project = FakeProject(dir: '/fake/game')..addDocument(_scene());
    project.schemas = [
      TypeSchema(
        type: 'game.Collectible',
        isBehaviour: true,
        authored: true,
        allowed: true,
        schema: collectibleSchema(),
      ),
    ];
    await pumpLarge(
      tester,
      shellApp(
        prefs: Preferences.inMemory(),
        projects: {'/fake/game': project},
        initialProject: '/fake/game',
        game: game('/fake/game'),
        extensions: const [EditorPart('game', _coins)],
      ),
    );
    return (tester.state<EditorShellState>(find.byType(EditorShell)), project);
  }

  List<String> errors(EditorShellState shell) => [
    for (final d in shell.extensions.log)
      if (d.severity == Severity.error) d.message,
  ];

  int valueOf(FakeProject project, String entity) =>
      ((project.fakeDocuments.single.entitiesView[entity]!['components'] as Map)['c'] as Map)['data']['value'] as int;

  testWidgets('a script\'s custom Inspector draws its card; a write is prechecked and one undo step', (tester) async {
    final (shell, project) = await openShell(tester);
    expect(errors(shell).single, contains('inspector<Stray> left out: Stray is not a script or data type'));

    final tab = shell.openDocument('main')!;
    tab.setSelection(['a']);
    await tester.pumpAndSettle();
    expect(find.text('worth 1 on 1'), findsOneWidget);
    expect(find.byKey(const ValueKey('field.game.Collectible#0.label')), findsOneWidget, reason: 'field() draws it');
    expect(find.byKey(const ValueKey('field.game.Collectible#0.value')), findsNothing);

    await tester.tap(find.byKey(const Key('coin.double')));
    await tester.pumpAndSettle();
    expect(valueOf(project, 'a'), 2);
    expect(find.text('worth 2 on 1'), findsOneWidget);

    // Below the minimum: refused, nothing written.
    await tester.tap(find.byKey(const Key('coin.zero')));
    await tester.pumpAndSettle();
    expect(valueOf(project, 'a'), 2);

    expect(tab.undo(), isTrue);
    await tester.pumpAndSettle();
    expect(valueOf(project, 'a'), 1);

    // Two coins: the first's value shown, the write on both, one step.
    tab.setSelection(['a', 'b']);
    await tester.pumpAndSettle();
    expect(find.text('worth 1 on 2'), findsOneWidget);
    await tester.tap(find.byKey(const Key('coin.double')));
    await tester.pumpAndSettle();
    expect((valueOf(project, 'a'), valueOf(project, 'b')), (2, 2));
    expect(tab.undo(), isTrue);
    expect((valueOf(project, 'a'), valueOf(project, 'b')), (1, 3));
  });

  testWidgets('a throwing custom Inspector: the default fields, a line saying why, one Console error', (tester) async {
    final (shell, _) = await openShell(tester);
    _throws = true;
    final tab = shell.openDocument('main')!;
    tab.setSelection(['a']);
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('inspector.custom.error')), findsOneWidget);
    expect(find.byKey(const ValueKey('field.game.Collectible#0.value')), findsOneWidget);
    tab.setSelection(['b']);
    await tester.pumpAndSettle();
    expect(errors(shell).where((e) => e.contains('Inspector failed')), hasLength(1));
    expect(
      errors(shell).last,
      contains('the game.Collectible Inspector failed, so the default one is shown: Bad state: no coin'),
    );
  });

  testWidgets('a data type\'s custom Inspector draws its page; defaultInspector(except:) the rest', (tester) async {
    final e = await Editor.open(
      tester,
      prepare: weapons,
      game: game('/elsewhere'),
      extensions: const [EditorPart('game', _coins)],
    );
    e.shell.browser!.select(sword);
    await tester.pumpAndSettle();
    expect(find.text('damage 1'), findsOneWidget);
    expect(find.byKey(const ValueKey('field.data.$sword.damage')), findsNothing);
    expect(find.byKey(const ValueKey('field.data.$sword.upgrade')), findsOneWidget);

    await tester.tap(find.byKey(const Key('weapon.sharpen')));
    await tester.pumpAndSettle();
    final file = File('${e.dir.path}/assets/data/sword.data.json');
    expect((jsonDecode(file.readAsStringSync()) as Map)['data'], {'damage': 9});
    expect(find.text('damage 9'), findsOneWidget);
  });
}
