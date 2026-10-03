// The editor API's own behaviour (foundation 6.5): its value classes, and
// the library's example extension written against the API alone. The
// editor implements the interfaces; its tests cover them.
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fynkel/gameplay.dart' show AssetRef;
import 'package:fynkel_editor_api/fynkel_editor_api.dart';

/// The example in `fynkel_editor_api.dart`'s library comment, as written.
void registerEditor(EditorExtensions editor) {
  editor.menuItem('Tools/Say Hello', id: 'game.sayHello', run: (context) => context.log('Hello'));
}

/// Records the menu items an extension registers.
final class _Extensions implements EditorExtensions {
  final items = <String, void Function(EditorContext)>{};

  @override
  String get package => 'game';

  @override
  void menuItem(
    String path, {
    required String id,
    required void Function(EditorContext context) run,
    bool Function(EditorContext context)? enabled,
    SingleActivator? shortcut,
    String? description,
  }) => items['$path|$id'] = run;

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

/// Keeps what is logged; nothing else is used.
final class _Context implements EditorContext {
  final lines = <String>[];

  @override
  void log(String message) => lines.add(message);

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

EditorComponent _component(String id, String type, [Map<String, Object?> data = const {}]) =>
    EditorComponent(id: id, type: type, data: data);

void main() {
  test('the library comment\'s example registers a menu item that logs', () {
    final editor = _Extensions();
    registerEditor(editor);
    expect(editor.items.keys, ['Tools/Say Hello|game.sayHello']);
    final context = _Context();
    editor.items.values.single(context);
    expect(context.lines, ['Hello']);
  });

  test('an object answers its first component of a type and its fields', () {
    final coin = EditorObject(
      id: 'o1',
      name: 'Coin',
      active: true,
      parentId: null,
      childIds: const [],
      components: [
        _component('c1', 'fynkel.Transform', {
          'position': [1, 2, 3],
        }),
        _component('c2', 'game.Collectible', {'value': 5}),
        _component('c3', 'game.Collectible', {'value': 9}),
      ],
    );
    expect(coin.has('game.Collectible'), isTrue);
    expect(coin.has('fynkel.Light'), isFalse);
    expect(coin.component('fynkel.Light'), isNull);
    expect(coin.component('game.Collectible')!.id, 'c2', reason: 'the first, in the Inspector\'s order');
    expect(coin.component('game.Collectible')!['value'], 5);
    expect(coin.component('fynkel.Transform')!['missing'], isNull);
    expect(coin.fromPrefab, isFalse);
    expect(coin.prefab, isNull);
    expect('$coin', 'EditorObject(Coin, o1)');
  });

  test('an asset\'s ref is what a field that names it holds', () {
    const asset = EditorAsset(
      id: '5c4e3f2a-1b0d-4c9e-8a7b-6f5e4d3c2b1a',
      path: 'data/coin.data.json',
      name: 'coin',
      kind: 'data',
      dataType: 'game.CoinKind',
    );
    expect(asset.ref, const AssetRef('5c4e3f2a-1b0d-4c9e-8a7b-6f5e4d3c2b1a'));
    expect(asset.ref.sub, isNull);
    expect('$asset', 'EditorAsset(data/coin.data.json)');
  });

  test('a preview clip is known by its asset and sub; its label is the sub\'s last part', () {
    const run = PreviewClip(asset: 'a', sub: 'anim/Run');
    expect(run.label, 'Run');
    expect(const PreviewClip(asset: 'a', sub: 'anim/Run', name: 'Sprint').label, 'Sprint');
    expect(
      run,
      const PreviewClip(asset: 'a', sub: 'anim/Run', name: 'Sprint'),
      reason: 'the name is only a label',
    );
    expect(run, isNot(const PreviewClip(asset: 'b', sub: 'anim/Run')));
    expect(run.hashCode, const PreviewClip(asset: 'a', sub: 'anim/Run', name: 'Sprint').hashCode);
  });

  test('an edit result says nothing was added unless it was', () {
    const refused = EditResult(ok: false, error: 'the scene is read-only');
    expect(refused.added, isEmpty);
    expect(const EditResult(ok: true).error, isNull);
  });
}
