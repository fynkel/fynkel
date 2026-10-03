import 'package:fynkel/gameplay.dart' show AssetRef;

/// A scene or prefab open in the editor, as it was when read.
abstract interface class EditorScene {
  /// Its asset id.
  String get id;

  /// Its file, under the project's `assets/` (`scenes/level.scene.json`).
  String get path;

  /// Its name, as the tab shows it.
  String get name;

  /// A prefab (the editor's Prefab mode) rather than a scene.
  bool get isPrefab;

  /// The running game's world, during Play.
  bool get isPlaying;

  /// Every object, in the Hierarchy's order (depth first).
  List<EditorObject> get objects;

  /// The top objects, in order.
  List<EditorObject> get roots;

  /// The object with this [id], if it is there.
  EditorObject? byId(String id);

  /// The first object named [name] in the Hierarchy's order, or at this
  /// path of names (`Level/Coins/Coin 1`, from a top object down).
  EditorObject? find(String name);
}

/// One object (Unity's `GameObject`) of an [EditorScene], as it was when
/// read.
final class EditorObject {
  const EditorObject({
    required this.id,
    required this.name,
    required this.active,
    required this.parentId,
    required this.childIds,
    required this.components,
    this.prefab,
    this.fromPrefab = false,
  });

  /// Its id in the document (stable: the same object keeps it).
  final String id;
  final String name;

  /// Its own active flag (Unity's `activeSelf`).
  final bool active;

  /// Its parent's id; null for a top object.
  final String? parentId;

  /// Its children's ids, in order.
  final List<String> childIds;

  /// Its components, in the Inspector's order, their data with a prefab's
  /// overrides applied.
  final List<EditorComponent> components;

  /// The prefab it is an instance of (the instance's root), if any.
  final AssetRef? prefab;

  /// Inside a prefab instance: it comes from the prefab, so a change to
  /// it is an override on the instance.
  final bool fromPrefab;

  /// Its first component of [type] (`fynkel.Transform`, `game.Collectible`).
  EditorComponent? component(String type) => components.where((c) => c.type == type).firstOrNull;

  /// Whether it has a component of [type].
  bool has(String type) => component(type) != null;

  @override
  String toString() => 'EditorObject($name, $id)';
}

/// A component of an [EditorObject]: its type and its data as the
/// document holds it (a vector is a list, an asset `{asset, sub?}`).
final class EditorComponent {
  const EditorComponent({required this.id, required this.type, required this.data});

  final String id;

  /// `fynkel.Transform`, `game.Collectible`.
  final String type;
  final Map<String, Object?> data;

  /// The field [name]'s value, as the document holds it.
  Object? operator [](String name) => data[name];

  @override
  String toString() => 'EditorComponent($type)';
}
