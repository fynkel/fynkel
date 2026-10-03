// The game's editor part (ADR-0028), without an editor: what it registers,
// and what its command says.
import 'package:code_only/editor/editor.dart';
import 'package:code_only/game/coin_kind.dart';
import 'package:code_only/game/level.dart' show burst;
import 'package:code_only/generated/assets.g.dart';
import 'package:code_only/generated/behaviours.g.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/authoring.dart' show ProjectWriter;
import 'package:fynkel/gameplay.dart' show Vec3;
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

typedef InspectorBuilder = Widget Function(BuildContext context, InspectorContext inspector);
typedef InspectorDrawer = Widget Function(BuildContext context, FieldContext field);

class _Items implements EditorExtensions {
  final items = <String, void Function(EditorContext)>{};

  @override
  String get package => 'code_only';

  @override
  void menuItem(
    String path, {
    required String id,
    required void Function(EditorContext context) run,
    bool Function(EditorContext context)? enabled,
    SingleActivator? shortcut,
    String? description,
  }) => items['$path|$id'] = run;

  final inspectors = <Type>[];
  InspectorBuilder? coinKind;

  @override
  void inspector<T>(InspectorBuilder build) {
    inspectors.add(T);
    coinKind = build;
  }

  final drawers = <String>[];
  final named = <String, InspectorDrawer>{};

  @override
  void drawer<T>(InspectorDrawer build) => drawers.add('$T');

  @override
  void namedDrawer(String name, InspectorDrawer build) {
    drawers.add(name);
    named[name] = build;
  }

  final windows = <String>[];
  final entries = <String>[];

  @override
  void window(
    String id, {
    required String title,
    String? menu,
    IconData? icon,
    required Widget Function(BuildContext context, EditorContext editor) build,
  }) => windows.add('$menu|$id');

  @override
  void hierarchyMenuItem(
    String label, {
    required String id,
    String? withComponent,
    required void Function(EditorContext context, List<EditorObject> objects) run,
    bool Function(EditorContext context, List<EditorObject> objects)? enabled,
  }) => entries.add('Hierarchy|$label|$withComponent');

  @override
  void projectMenuItem(
    String label, {
    required String id,
    String? kind,
    String? dataType,
    required void Function(EditorContext context, List<EditorAsset> assets) run,
    bool Function(EditorContext context, List<EditorAsset> assets)? enabled,
  }) => entries.add('Project|$label');

  @override
  void componentMenuItem(
    String type,
    String label, {
    required String id,
    required void Function(EditorContext context, List<EditorObject> objects) run,
    bool Function(EditorContext context, List<EditorObject> objects)? enabled,
  }) => entries.add('$type|$label');

  @override
  void preview({
    String? kind,
    String? dataType,
    bool Function(EditorAsset asset)? accepts,
    required Widget Function(BuildContext context, PreviewContext preview) build,
    Widget Function(BuildContext context, PreviewContext preview)? toolbar,
    String? Function(PreviewContext preview)? info,
  }) => entries.add('Preview|$kind|$dataType');
}

class _Log implements EditorContext {
  final lines = <String>[];

  @override
  String get package => 'code_only';

  @override
  void log(String message) => lines.add(message);

  @override
  void logWarning(String message) => lines.add('warning: $message');

  @override
  void logError(String message) => lines.add('error: $message');

  // The scene, the selection, edits and assets: not used here.
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

/// A data asset's fields as the editor's Inspector would give them.
class _CoinKindFields implements InspectorContext {
  _CoinKindFields(this.data);

  final Map<String, Object?> data;

  @override
  Object? value(String field) => data[field];

  @override
  Widget defaultInspector({Iterable<String> except = const []}) => const Text('default fields');

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

void main() {
  test('the editor part lists the coin kinds from the Tools menu', () {
    final editor = _Items();
    registerEditor(editor);
    expect(editor.items.keys, [
      'Tools/Code Only/List Coin Kinds|codeOnly.listCoinKinds',
      'Tools/Code Only/Select Coins|codeOnly.selectCoins',
      'Tools/Code Only/Add Coin|codeOnly.addCoin',
      'Tools/Code Only/Collect All Coins|codeOnly.collectAll',
    ]);
    final log = _Log();
    editor.items.values.first(log);
    expect(log.lines, ['A coin is worth 1, the big coin 5 (1.6× the size).']);
    expect(editor.windows, ['Tools/Code Only/Level Tools|codeOnly.levelTools']);
    expect(editor.entries, ['Hierarchy|Make Big Coin|game.Collectible']);
    expect(editor.inspectors, [CoinKind]);
    expect(editor.drawers, ['jumpHeight', 'Sparks']);
  });

  test("the jumpHeight drawer's line: v² / 2g", () {
    expect(JumpHeight.describe(5.5), 'Reaches 1.54 m');
    expect(JumpHeight.describe(null), contains('differ'));
  });

  testWidgets("the coin kind's Inspector: the coin in its colour at its size, then the default fields", (tester) async {
    final editor = _Items();
    registerEditor(editor);
    final big = _CoinKindFields({
      'value': 5,
      'color': [1.0, 0.42, 0.12],
      'size': 1.6,
    });
    await tester.pumpWidget(MaterialApp(home: Scaffold(body: Builder(builder: (c) => editor.coinKind!(c, big)))));
    expect(find.text('Worth 5'), findsOneWidget);
    expect(find.text('default fields'), findsOneWidget);
    final preview = tester.widget<CoinPreview>(find.byType(CoinPreview));
    expect(preview.diameter, closeTo(CoinPreview.base * 1.6, 1e-9));
    expect(preview.swatch.r, closeTo(1, 1e-6));
    expect(tester.getSize(find.byKey(const Key('coinPreview.coin'))).width, closeTo(CoinPreview.base * 1.6, 1e-6));
  });

  testWidgets("the Sparks drawer's burst: as many as the record says, and how long they last", (tester) async {
    Future<String> line(Object? count, Object? seconds) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(body: SparksPreview(count: count, size: 0.24, seconds: seconds)),
        ),
      );
      return tester.widget<Text>(find.byKey(const Key('sparksPreview.text'))).data!;
    }

    expect(await line(12, 1.6), '12 sparks, gone after 1.6 s');
    expect(await line(1, 2), '1 spark, gone after 2 s');
    expect(await line(0, 1.2), 'No sparks, gone after 1.2 s');
    expect(await line(null, null), '6 sparks, gone after 1.2 s', reason: "the record's defaults");
    expect(await line(99, 1), contains('24 sparks'), reason: 'at most 24');
    expect(await line(8, 1.67109375), '8 sparks, gone after 1.67 s', reason: 'a dragged value, rounded');
    expect(await line(8, 1.5), '8 sparks, gone after 1.5 s');
  });

  test("a coin's burst: its kind's sparks, their size and how long they last", () {
    final big = CoinKinds().big.sparks;
    final spec = burst(Assets.materials.gold, Vec3.zero, big);
    expect(spec.children, hasLength(12));
    expect(spec.children.first.transform.scale, const Vec3(0.24, 0.24, 0.24));
    expect(burst(Assets.materials.gold, Vec3.zero).children, hasLength(6), reason: 'a Sparks() by default');
    expect(burst(Assets.materials.gold, Vec3.zero, Sparks(count: 0)).children, isEmpty);
  });

  test('the Level Tools ring: coin prefabs evenly around the centre', () {
    final ring = LevelToolsRing.of(4, 2);
    expect(ring.name, 'Coin Ring');
    expect([for (final c in ring.children) c.name], ['Coin 1', 'Coin 2', 'Coin 3', 'Coin 4']);
    expect(ring.children.every((c) => c.prefab == Assets.prefabs.coin), isTrue);
    final p = ring.children[1].transform.position;
    expect((p.x.abs() < 1e-9, p.y, p.z), (true, 0.5, 2.0));

    // What the editor pastes: the ring and one instance of the coin prefab
    // per coin, under it.
    final built = ProjectWriter('.', registerBehaviours: registerBehaviours).entitiesOf([ring]);
    expect(built.roots, hasLength(1));
    final entities = built.entities.values.cast<Map<String, Object?>>().toList();
    expect(entities, hasLength(5));
    expect([for (final e in entities) if (e['prefab'] != null) e['name']], ['Coin 1', 'Coin 2', 'Coin 3', 'Coin 4']);
  });
}
