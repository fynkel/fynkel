import 'package:fynkel/gameplay.dart' show AssetRef, DataAsset;

/// The project's assets (Unity's `AssetDatabase`), as they are when asked.
abstract interface class EditorAssets {
  /// Every asset.
  List<EditorAsset> get all;

  /// The assets of [kind]: `scene`, `prefab`, `material`, `data`,
  /// `model`, `texture`, `audio`, …
  List<EditorAsset> ofKind(String kind);

  /// The data assets of the data type [typeId] (`game.CoinKind`).
  List<EditorAsset> ofDataType(String typeId);

  /// The asset at [path] under `assets/` (with or without its extension:
  /// `data/coin`), or else the first named [path].
  EditorAsset? find(String path);

  /// The assets selected in the Project panel.
  List<EditorAsset> get selected;

  /// Creates a data asset at [path] under `assets/` (`.data.json` added
  /// when missing) holding [value] (Unity's `AssetDatabase.CreateAsset`):
  /// ADR-0027's `writeData`, through the editor, as the Project panel's
  /// Create does. [value] then knows its asset, so an object added
  /// afterwards names it. Null when it could not (the Console says why:
  /// the file exists, the type is not registered).
  EditorAsset? createData(String path, DataAsset value);

  /// Shows [asset] in the Project panel, outlined, without selecting it
  /// (Unity's `EditorGUIUtility.PingObject`).
  void ping(EditorAsset asset);

  /// Opens [asset]: a scene or prefab in its tab, anything else in the
  /// Inspector (Unity's `AssetDatabase.OpenAsset`).
  void open(EditorAsset asset);
}

/// One asset of the project.
final class EditorAsset {
  const EditorAsset({required this.id, required this.path, required this.name, required this.kind, this.dataType});

  final String id;

  /// Under the project's `assets/`: `data/coin.data.json`.
  final String path;
  final String name;

  /// `scene`, `prefab`, `material`, `data`, `model`, `texture`, `audio`, …
  final String kind;

  /// A data asset's type id (`game.CoinKind`).
  final String? dataType;

  /// What a field that names it holds.
  AssetRef get ref => AssetRef(id);

  @override
  String toString() => 'EditorAsset($path)';
}
