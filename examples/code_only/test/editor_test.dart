// The game's editor part (ADR-0028), without an editor: what it registers,
// and what its command says.
import 'package:code_only/editor/editor.dart';
import 'package:code_only/generated/assets.g.dart';
import 'package:code_only/generated/behaviours.g.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/authoring.dart' show ProjectWriter;
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

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

void main() {
  test('the editor part lists the coin kinds from the Tools menu', () {
    final editor = _Items();
    registerEditor(editor);
    expect(editor.items.keys, [
      'Tools/Code Only/List Coin Kinds|codeOnly.listCoinKinds',
      'Tools/Code Only/Select Coins|codeOnly.selectCoins',
      'Tools/Code Only/Add Coin|codeOnly.addCoin',
    ]);
    final log = _Log();
    editor.items.values.first(log);
    expect(log.lines, ['A coin is worth 1, the big coin 5 (1.6× the size).']);
    expect(editor.windows, ['Tools/Code Only/Level Tools|codeOnly.levelTools']);
    expect(editor.entries, ['Hierarchy|Make Big Coin|game.Collectible']);
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
