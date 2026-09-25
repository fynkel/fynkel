/// The project's assets by path at runtime (ADR-0026 §3): `world.assets`,
/// Unity's `Resources.Load("prefabs/Burst")`. The path is the file's under
/// `assets/`, with `/` separators, read from the cooked cache's
/// `index.json` (on a device, the copy the app bundles), so it is what the
/// last cook saw. Code that names an asset it knows while writing prefers
/// the generated `Assets.…` constants (`lib/generated/assets.g.dart`),
/// which the compiler checks; this is for paths only known while running
/// (a level list in a file, a name typed in a console).
library;

import 'dart:convert' show jsonDecode;
import 'dart:io' show File, FileSystemException;

import 'asset_ref.dart';

/// One asset of the cook's index.
final class CookedAsset {
  const CookedAsset({required this.id, required this.path, required this.kind});

  final String id;

  /// Under `assets/`, `/`-separated: `prefabs/burst.prefab.json`.
  final String path;

  /// The importer or document kind: `document/prefab`, `fynkel.gltf`, `fynkel.audio`.
  final String kind;

  AssetRef get ref => AssetRef(id);

  @override
  String toString() => '$path ($kind, $id)';
}

final class AssetIndex {
  /// Reads `index.json` in the directory [cacheDir] returns at each call
  /// (a world's: the cache of its last load).
  AssetIndex(this._cacheDir);

  final String? Function() _cacheDir;

  String? _readDir;
  DateTime? _readStamp;
  List<CookedAsset> _entries = const [];
  Map<String, CookedAsset> _byPath = const {};
  Map<String, CookedAsset> _byId = const {};

  /// Every asset of the last cook, in the index's order.
  List<CookedAsset> get all {
    _refresh();
    return _entries;
  }

  /// The asset at [path] under `assets/` — `scenes/level1.scene.json`, or
  /// without its extension, `scenes/level1`, when only one file has that
  /// name — as an [AssetRef] naming its sub-asset [sub] (`mesh/Cube`,
  /// `prefab`) if given. Null when no asset has the path; a path without an
  /// extension that two files share throws [StateError] naming them.
  AssetRef? find(String path, {String? sub}) {
    _refresh();
    final key = _normalize(path);
    var entry = _byPath[key];
    if (entry == null) {
      final matches = [for (final e in _entries) if (_withoutExtension(e.path) == key) e];
      if (matches.length > 1) {
        throw StateError('assets.find($path): ${matches.map((e) => e.path).join(', ')} all match; add the extension');
      }
      entry = matches.firstOrNull;
    }
    if (entry == null) return null;
    return AssetRef(entry.id, sub: sub);
  }

  /// The path under `assets/` of the asset [ref] names (its sub-asset
  /// aside), or null when the last cook has no such asset.
  String? pathOf(AssetRef ref) {
    _refresh();
    return _byId[ref.asset]?.path;
  }

  static String _normalize(String path) {
    var p = path.replaceAll(r'\', '/');
    if (p.startsWith('./')) p = p.substring(2);
    if (p.startsWith('assets/')) p = p.substring('assets/'.length);
    return p;
  }

  /// `scenes/level1.scene.json` → `scenes/level1`: the name up to its first
  /// dot, as the Project browser shows it.
  static String _withoutExtension(String path) {
    final slash = path.lastIndexOf('/');
    final dot = path.indexOf('.', slash + 1);
    return dot <= slash + 1 ? path : path.substring(0, dot);
  }

  /// Reads the index again when the cache moved or a cook rewrote it.
  void _refresh() {
    final dir = _cacheDir();
    if (dir == null) {
      throw StateError('world.assets: no cache directory yet — loadProject or loadScene first');
    }
    final file = File('$dir/index.json');
    DateTime? stamp;
    try {
      stamp = file.lastModifiedSync();
    } on FileSystemException {
      stamp = null;
    }
    if (dir == _readDir && stamp == _readStamp) return;
    final entries = <CookedAsset>[];
    if (stamp != null) {
      final json = jsonDecode(file.readAsStringSync());
      for (final a in (json is Map ? json['assets'] : null) as List? ?? const []) {
        if (a is! Map) continue;
        final id = a['id'], path = a['path'];
        if (id is! String || path is! String) continue;
        entries.add(CookedAsset(id: id, path: path, kind: a['kind']?.toString() ?? ''));
      }
    }
    _readDir = dir;
    _readStamp = stamp;
    _entries = List.unmodifiable(entries);
    _byPath = {for (final e in entries) e.path: e};
    _byId = {for (final e in entries) e.id: e};
  }
}
