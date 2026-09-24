// The Project browser's state (item 5): the asset database as folders or
// as one flat list, searched, filtered by kind and sorted; a selection;
// the asset operations — every one a session operation with its own undo
// (`EditorProject.operate`), the browser only chooses paths and names.
// Folders are not assets: creating, renaming and deleting one is file
// work here, and a folder rename moves each asset under it through the
// session so every id survives. The Scripts root (roadmap Phase 2 item 1)
// lists `lib/` beside the assets (`scripts.dart`).
import 'dart:async';
import 'dart:io';
import 'dart:ui' show Color;

import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart' show ImageProvider;
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../inspector/field_widgets.dart' show AssetDragPayload;
import '../project/editor_project.dart';
import 'scripts.dart';
import 'thumbnails.dart';

enum BrowserView { folders, list }

enum AssetSort { name, kind, path, modified }

/// The kinds the filter menu offers, in order; a source is one of the last
/// four by its importer.
const browserKinds = [
  'scene',
  'prefab',
  'material',
  'rig',
  'blend',
  'volume',
  'physics_material',
  'texture',
  'model',
  'sound',
  'other',
];

/// The browser kind of an entry: a document's kind, a source's by importer.
String browserKindOf(AssetEntry e) {
  if (!e.isSource) return e.document ?? 'other';
  return switch (e.importer) {
    'fynkel.image' => 'texture',
    'fynkel.gltf' || 'fynkel.fbx' => 'model',
    'fynkel.audio' => 'sound',
    _ => 'other',
  };
}

/// A file size for people: `812 B`, `14.2 KB`, `3.10 MB`.
String formatBytes(int n) {
  if (n < 1024) return '$n B';
  if (n < 1024 * 1024) return '${(n / 1024).toStringAsFixed(1)} KB';
  return '${(n / (1024 * 1024)).toStringAsFixed(2)} MB';
}

/// A document kind's file suffix: `.scene.json`, and `.physicsmaterial.json`
/// for `physics_material` (ADR-0025).
String documentSuffix(String kind) => kind == 'physics_material' ? '.physicsmaterial.json' : '.$kind.json';

/// A document kind's name for people: `Volume Profile`, `Physics Material`.
String documentKindTitle(String kind) => switch (kind) {
  'volume' => 'Volume Profile',
  'physics_material' => 'Physics Material',
  _ => kind.isEmpty ? kind : '${kind[0].toUpperCase()}${kind.substring(1)}',
};

/// The `assetKindIcon` kind of a browser kind.
String iconKindOf(String browserKind) => switch (browserKind) {
  'model' => 'mesh',
  'blend' => 'clip',
  _ => browserKind,
};

/// What a row shows before its name.
sealed class AssetPreview {
  const AssetPreview();
}

/// A decoded image file (PNG, JPEG, WebP, GIF, BMP — what Flutter decodes).
final class TexturePreview extends AssetPreview {
  const TexturePreview(this.file);
  final String file;
}

/// A material's base colour, from its factors.
final class SwatchPreview extends AssetPreview {
  const SwatchPreview(this.color);
  final Color color;
}

/// An icon per other kind (mesh thumbnails wait for item 6's world).
final class IconPreview extends AssetPreview {
  const IconPreview(this.kind);

  /// An `assetKindIcon` kind.
  final String kind;
}

/// A delete the session refused because the assets are referenced: the
/// panel asks "delete anyway?" and answers with `delete(force: true)`.
final class DeleteRefusal {
  const DeleteRefusal({required this.ids, required this.message, required this.dependents});

  final List<String> ids;
  final String message;

  /// One per referencing field: `document` (absolute path), `pointer`, `id`.
  final List<Diagnostic> dependents;
}

final class ProjectBrowserModel extends ChangeNotifier {
  ProjectBrowserModel({required this.project}) {
    project.addListener(_onProject);
    _files = project.fileChanges.listen(_onFiles);
  }

  final EditorProject project;

  BrowserView view = BrowserView.folders;

  /// The folder shown in the folders view, relative to the asset root
  /// (`''` is the root).
  String folder = '';
  String query = '';

  /// Browser kinds shown; empty shows every kind.
  final Set<String> kinds = {};
  AssetSort sort = AssetSort.name;
  bool descending = false;

  /// Selected asset ids and the lead (the one the Inspector shows).
  final Set<String> selection = {};
  String? lead;

  /// The selected part of [lead] (a model's `mesh/Cube`), shown in the
  /// Inspector; null when the asset itself is.
  String? leadPart;

  /// Sources whose parts the list shows under them (Unity's ▸ on a model).
  final Set<String> expandedSources = {};

  /// A source with parts to show: more than the one it drags whole.
  bool canExpand(AssetEntry e) => e.isSource && e.subAssets.length > 1;

  void toggleExpanded(String id) {
    if (!expandedSources.remove(id)) expandedSources.add(id);
    if (!expandedSources.contains(id) && lead == id) leadPart = null;
    notifyListeners();
  }

  /// The parts listed under [e] when it is expanded, in key order.
  List<String> partsOf(AssetEntry e) =>
      expandedSources.contains(e.id) && canExpand(e) ? (e.subAssets.keys.toList()..sort()) : const [];

  /// A part clicked: its asset selected, the part the Inspector shows.
  void selectPart(String id, String sub) {
    select(id);
    leadPart = sub;
    notifyListeners();
  }

  /// The asset (id) or folder (path) whose name is being edited inline.
  String? renamingAsset;
  String? renamingFolder;

  /// The last refused operation's reason, until the next one succeeds.
  String? lastError;
  DeleteRefusal? pendingDelete;

  /// The assets a Delete asks about before anything moves (Unity's
  /// "Delete selected asset?"); the panel shows the question.
  List<String>? pendingConfirm;
  int searchFocusRequests = 0;
  bool _disposed = false;

  // ── The database, cached per revision ───────────────────────────────

  int _seen = -1;
  List<AssetEntry> _entries = const [];
  Map<String, AssetEntry> _byId = const {};
  List<String> _folders = const [];
  final _details = <String, AssetDetail?>{};
  final _modified = <String, DateTime?>{};
  final _sizes = <String, int?>{};
  Map<String, int>? _folderCounts;
  AssetHistory? _history;

  void _sync() {
    final rev = project.assetsRevision;
    if (rev == _seen) return;
    _seen = rev;
    _entries = project.assets();
    _byId = {for (final e in _entries) e.id: e};
    _folders = project.folders();
    _details.clear();
    _modified.clear();
    _sizes.clear();
    _folderCounts = null;
    _history = null;
    // An operation may have been registerBehaviour: the metadata moved.
    _scripts = null;
    _behaviourIds = null;
    // What vanished leaves the selection.
    selection.removeWhere((id) => !_byId.containsKey(id));
    if (lead != null && !_byId.containsKey(lead)) lead = selection.lastOrNull;
    if (renamingAsset != null && !_byId.containsKey(renamingAsset)) renamingAsset = null;
    if (folder.isNotEmpty && !_folders.contains(folder)) folder = _parentOf(folder);
  }

  void _onProject() {
    if (_disposed) return;
    final before = _seen;
    _sync();
    if (_seen != before) notifyListeners();
  }

  /// Re-reads even when the revision did not move (a folder made outside).
  void refresh() {
    _seen = -1;
    _sync();
    notifyListeners();
  }

  List<AssetEntry> get entries {
    _sync();
    return _entries;
  }

  List<String> get folders {
    _sync();
    return _folders;
  }

  AssetEntry? entry(String id) {
    _sync();
    return _byId[id];
  }

  AssetDetail? detail(String id) {
    _sync();
    return _details.putIfAbsent(id, () => project.assetDetail(id));
  }

  AssetHistory get history {
    _sync();
    return _history ??= project.assetHistory();
  }

  DateTime? modified(AssetEntry e) {
    _sync();
    return _modified.putIfAbsent(e.id, () {
      try {
        return File(project.assetFile(e.path)).lastModifiedSync();
      } on FileSystemException {
        return null;
      }
    });
  }

  /// The file's size in bytes (the Size column); null when it cannot be read.
  int? size(AssetEntry e) {
    _sync();
    return _sizes.putIfAbsent(e.id, () {
      try {
        return File(project.assetFile(e.path)).lengthSync();
      } on FileSystemException {
        return null;
      }
    });
  }

  /// How many assets are inside [rel], at any depth (a folder row's Contents).
  int folderCount(String rel) {
    _sync();
    final counts = _folderCounts ??= () {
      final out = <String, int>{};
      for (final e in _entries) {
        var at = e.path.lastIndexOf('/');
        while (at > 0) {
          final f = e.path.substring(0, at);
          out.update(f, (n) => n + 1, ifAbsent: () => 1);
          at = f.lastIndexOf('/');
        }
      }
      return out;
    }();
    return rel.isEmpty ? _entries.length : counts[rel] ?? 0;
  }

  // ── What the list shows ─────────────────────────────────────────────

  bool get filtering => query.trim().isNotEmpty || kinds.isNotEmpty;

  bool _matches(AssetEntry e) {
    if (kinds.isNotEmpty && !kinds.contains(browserKindOf(e))) return false;
    final q = query.trim().toLowerCase();
    if (q.isEmpty) return true;
    if (e.path.toLowerCase().contains(q) || e.name.toLowerCase().contains(q) || e.id.startsWith(q)) return true;
    return e.subAssets.keys.any((s) => s.toLowerCase().contains(q));
  }

  static String _parentOf(String rel) {
    final i = rel.lastIndexOf('/');
    return i < 0 ? '' : rel.substring(0, i);
  }

  static String _folderOf(AssetEntry e) => _parentOf(e.path);

  /// The sub-folders of [folder] (folders view without a filter; the list
  /// view and a search show assets only).
  List<String> get visibleFolders {
    _sync();
    if (view == BrowserView.list || filtering) return const [];
    return [
      for (final f in _folders)
        if (_parentOf(f) == folder) f,
    ];
  }

  /// The assets shown: the folder's own in the folders view, everything
  /// under it while searching, every asset in the list view — filtered and
  /// sorted.
  List<AssetEntry> get visibleAssets {
    _sync();
    final out = <AssetEntry>[];
    for (final e in _entries) {
      if (!_matches(e)) continue;
      if (view == BrowserView.folders) {
        if (filtering) {
          if (folder.isNotEmpty && !(e.path.startsWith('$folder/'))) continue;
        } else if (_folderOf(e) != folder) {
          continue;
        }
      }
      out.add(e);
    }
    out.sort(_compare);
    return out;
  }

  int _compare(AssetEntry a, AssetEntry b) {
    int byName() => p.basename(a.path).toLowerCase().compareTo(p.basename(b.path).toLowerCase());
    var r = switch (sort) {
      AssetSort.name => byName(),
      AssetSort.kind => browserKindOf(a).compareTo(browserKindOf(b)),
      AssetSort.path => a.path.toLowerCase().compareTo(b.path.toLowerCase()),
      AssetSort.modified => (modified(a) ?? DateTime(0)).compareTo(modified(b) ?? DateTime(0)),
    };
    if (r == 0) r = byName();
    if (r == 0) r = a.path.compareTo(b.path);
    return descending ? -r : r;
  }

  /// The number of assets per browser kind, for the filter menu.
  Map<String, int> get countsByKind {
    _sync();
    final out = <String, int>{};
    for (final e in _entries) {
      out.update(browserKindOf(e), (n) => n + 1, ifAbsent: () => 1);
    }
    return out;
  }

  // ── View state ──────────────────────────────────────────────────────

  void setView(BrowserView v) {
    if (view == v) return;
    view = v;
    notifyListeners();
  }

  void toggleView() => setView(view == BrowserView.folders ? BrowserView.list : BrowserView.folders);

  void setFolder(String rel) {
    _sync();
    if (rel.isNotEmpty && !_folders.contains(rel)) return;
    inScripts = false;
    folder = rel;
    if (view != BrowserView.folders) view = BrowserView.folders;
    notifyListeners();
  }

  void goUp() => setFolder(_parentOf(folder));

  void setQuery(String q) {
    if (query == q) return;
    query = q;
    notifyListeners();
  }

  void toggleKind(String kind) {
    if (!kinds.remove(kind)) kinds.add(kind);
    notifyListeners();
  }

  void clearFilters() {
    query = '';
    kinds.clear();
    notifyListeners();
  }

  void setSort(AssetSort s, {bool? descending}) {
    if (sort == s && descending == null) {
      this.descending = !this.descending;
    } else {
      sort = s;
      this.descending = descending ?? false;
    }
    notifyListeners();
  }

  void requestSearchFocus() {
    searchFocusRequests++;
    notifyListeners();
  }

  // ── Selection ───────────────────────────────────────────────────────

  bool isSelected(String id) => selection.contains(id);

  void select(String id, {bool toggle = false, bool range = false}) {
    _sync();
    pinged = null;
    leadPart = null;
    if (!_byId.containsKey(id)) return;
    if (range && lead != null) {
      final rows = visibleAssets.map((e) => e.id).toList();
      final a = rows.indexOf(lead!);
      final b = rows.indexOf(id);
      if (a >= 0 && b >= 0) {
        selection.addAll(rows.sublist(a < b ? a : b, (a < b ? b : a) + 1));
        notifyListeners();
        return;
      }
    }
    if (toggle) {
      if (!selection.remove(id)) selection.add(id);
      lead = selection.contains(id) ? id : selection.lastOrNull;
    } else {
      selection
        ..clear()
        ..add(id);
      lead = id;
    }
    notifyListeners();
  }

  void selectAll() {
    selection
      ..clear()
      ..addAll(visibleAssets.map((e) => e.id));
    lead ??= selection.lastOrNull;
    notifyListeners();
  }

  void clearSelection() {
    pinged = null;
    leadPart = null;
    if (selection.isEmpty && lead == null) return;
    selection.clear();
    lead = null;
    notifyListeners();
  }

  // ── Hierarchy → Project: a new prefab (Unity's drag) ─────────────────

  /// Why the Hierarchy's dragged objects cannot become a prefab (null:
  /// they can); set by the shell to the Hierarchy's rule.
  String? Function(List<String> ids)? cannotMakePrefab;

  /// Makes the prefab in [folder] and answers its id (null: refused or
  /// cancelled — an instance asks first); set by the shell.
  Future<String?> Function(List<String> ids, String folder)? makePrefab;

  bool canMakePrefab(List<String> ids) =>
      canEdit && makePrefab != null && cannotMakePrefab != null && cannotMakePrefab!(ids) == null;

  /// A Hierarchy drag dropped on [folder]: the prefab is made there and
  /// selected.
  Future<bool> dropObjects(List<String> ids, String folder) async {
    if (!canMakePrefab(ids)) return false;
    final id = await makePrefab!(ids, folder);
    if (id == null) return false;
    reveal(id);
    return true;
  }

  /// Selects [id] and shows its folder in the folders view.
  void reveal(String id) {
    final e = entry(id);
    if (e == null) return;
    inScripts = false;
    if (view == BrowserView.folders && !filtering) folder = _folderOf(e);
    select(id);
    scrollRequests++;
  }

  /// Unity's ping: [id] shown in its folder and outlined, the selection
  /// (and so the Inspector) left alone; the next click clears it.
  void ping(String id) {
    final e = entry(id);
    if (e == null) return;
    inScripts = false;
    if (view == BrowserView.folders && !filtering) folder = _folderOf(e);
    pinged = id;
    scrollRequests++;
    notifyListeners();
  }

  /// The asset [ping] outlined, until the next selection change.
  String? pinged;

  /// Bumped when the panel should scroll [pinged] or the lead into view.
  int scrollRequests = 0;

  // ── The padlock (item 9e) ────────────────────────────────────────────

  /// A locked browser stays where it is: "Show in Project", a Game row or
  /// an asset slot's arrow reveals in another copy — the shell opens one
  /// when every copy is locked. The user's own clicks still move it.
  bool locked = false;

  // ── The zoom slider (Unity's, at the status bar's right) ─────────────

  /// Below this the slider's left end shows the list.
  static const gridFrom = 0.05;

  /// 0: the list; above [gridFrom], a grid of tiles growing with it.
  double zoom = 0;

  bool get grid => zoom >= gridFrom;

  void setZoom(double v) {
    v = v.clamp(0.0, 1.0);
    if (v < gridFrom) v = 0;
    if (v == zoom) return;
    zoom = v;
    notifyListeners();
  }

  /// A tile's picture, a side, in logical pixels.
  double get tileSize => 48 + zoom * 80;

  /// Rendered pictures of models, prefabs, meshes and materials; null when
  /// there is no CLI to draw them (tests).
  ThumbnailCache? thumbnails;

  /// The rendered picture of [e] (its part [sub]) when it has one: a
  /// model, a prefab, a material, a model's mesh or material.
  ImageProvider? thumbnail(AssetEntry e, {String? sub}) {
    final cache = thumbnails;
    if (cache == null) return null;
    final kind = sub == null ? browserKindOf(e) : AssetEntry.subKind(sub);
    if (!const {'model', 'prefab', 'material', 'mesh'}.contains(kind)) return null;
    return cache.image(e.id, sub, project.assetFile(e.path));
  }

  void setLocked(bool on) {
    if (on == locked) return;
    locked = on;
    notifyListeners();
  }

  /// What a restart brings back: the folder, the view and the padlock.
  Map<String, Object?> get remembered => {
    'folder': folder,
    'view': view.name,
    if (inScripts) 'scripts': scriptFolder,
    if (locked) 'locked': true,
    if (zoom > 0) 'zoom': zoom,
  };

  /// Restores [remembered]; a folder that is gone falls back to the root.
  void recall(Object? json) {
    if (json is! Map) return;
    _sync();
    final f = json['folder'];
    if (f is String && (f.isEmpty || _folders.contains(f))) folder = f;
    view = BrowserView.values.asNameMap()[json['view']] ?? view;
    final s = json['scripts'];
    if (s is String && (s.isEmpty || scripts.folders.contains(s))) {
      inScripts = true;
      scriptFolder = s;
    }
    locked = json['locked'] == true;
    final z = json['zoom'];
    zoom = z is num && z >= gridFrom ? z.toDouble().clamp(0.0, 1.0) : 0;
    notifyListeners();
  }

  /// Moves the lead by [delta] rows (⇧ extends).
  void step(int delta, {bool extend = false}) {
    final rows = visibleAssets;
    if (rows.isEmpty) return;
    var i = lead == null ? -1 : rows.indexWhere((e) => e.id == lead);
    i = (i + delta).clamp(0, rows.length - 1);
    if (extend) {
      extendTo(rows[i].id);
    } else {
      select(rows[i].id);
    }
  }

  /// [id] added to the selection as its lead (⇧ with an arrow key).
  void extendTo(String id) {
    selection.add(id);
    lead = id;
    leadPart = null;
    pinged = null;
    notifyListeners();
  }

  /// The selected entries, in the list's order.
  List<AssetEntry> get selectedEntries {
    final out = [
      for (final e in visibleAssets)
        if (selection.contains(e.id)) e,
    ];
    final shown = {for (final e in out) e.id};
    for (final id in selection) {
      if (!shown.contains(id)) {
        final e = entry(id);
        if (e != null) out.add(e);
      }
    }
    return out;
  }

  AssetEntry? get leadEntry => lead == null ? null : entry(lead!);

  // ── Previews and drags ──────────────────────────────────────────────

  /// [file] (absolute) relative to the asset root, the way rows show paths;
  /// a file outside it relative to the project.
  String assetPath(String file) => p.isWithin(project.assetsDir, file)
      ? p.relative(file, from: project.assetsDir).replaceAll(r'\', '/')
      : project.relative(file);

  static const _decodable = {'.png', '.jpg', '.jpeg', '.webp', '.gif', '.bmp'};

  AssetPreview preview(AssetEntry e) {
    final kind = browserKindOf(e);
    if (kind == 'texture' && _decodable.contains(p.extension(e.path).toLowerCase())) {
      return TexturePreview(project.assetFile(e.path));
    }
    if (kind == 'material') {
      final color = swatchColor(detail(e.id));
      if (color != null) return SwatchPreview(color);
    }
    return IconPreview(iconKindOf(kind));
  }

  /// A material's `base_color` factor as a colour, or null.
  static Color? swatchColor(AssetDetail? d) {
    final m = d?.material;
    if (m == null) return null;
    final fields = m['fields'];
    final c = fields is Map ? fields['base_color'] : null;
    if (c is! List || c.length < 3) {
      // The PBR shader's base colour defaults to white; a custom shader's swatch is unknown.
      return m['shader'] == 'fynkel.pbr@1' ? const Color.from(alpha: 1, red: 1, green: 1, blue: 1) : null;
    }
    double ch(Object? v) => v is num ? v.toDouble().clamp(0.0, 1.0) : 1.0;
    return Color.from(alpha: c.length > 3 ? ch(c[3]) : 1.0, red: ch(c[0]), green: ch(c[1]), blue: ch(c[2]));
  }

  /// What a row drags: a document by its kind, a whole audio source as a
  /// `sound`, otherwise the source's first sub-asset of each kind is not
  /// guessed — the Inspector's asset page drags a source's parts one by one.
  AssetDragPayload? dragPayload(AssetEntry e, {String? sub}) {
    final name = p.basename(e.path);
    if (sub != null) {
      if (!e.subAssets.containsKey(sub)) return null;
      return AssetDragPayload(id: e.id, sub: sub, kind: AssetEntry.subKind(sub), name: '$name#$sub');
    }
    if (!e.isSource) return AssetDragPayload(id: e.id, kind: e.document ?? 'other', name: name);
    if (e.wholeKind != null) return AssetDragPayload(id: e.id, kind: e.wholeKind!, name: name);
    if (e.subAssets.containsKey('prefab')) {
      return AssetDragPayload(id: e.id, sub: 'prefab', kind: 'prefab', name: '$name#prefab');
    }
    final subs = e.subAssets.keys.toList();
    if (subs.length == 1) return dragPayload(e, sub: subs.single);
    return null;
  }

  // ── Operations ──────────────────────────────────────────────────────

  static String _messageOf(AuthoringException e) {
    final lines = [for (final d in e.diagnostics) d.message];
    return lines.isEmpty ? e.toString() : lines.join('\n');
  }

  OperationResult? _guard(OperationResult Function() f) {
    try {
      final r = f();
      lastError = null;
      pendingDelete = null;
      _sync();
      notifyListeners();
      return r;
    } on AuthoringException catch (e) {
      lastError = _messageOf(e);
      notifyListeners();
      return null;
    }
  }

  void clearError() {
    if (lastError == null) return;
    lastError = null;
    notifyListeners();
  }

  bool get canEdit => !project.isReadOnly;

  /// A unique `New <Kind>[ N]` file in [inFolder] for a document kind.
  String _freshPath(String inFolder, String base, String suffix) {
    _sync();
    final taken = {for (final e in _entries) e.path.toLowerCase()};
    for (var n = 1; ; n++) {
      final stem = n == 1 ? base : '$base $n';
      final rel = inFolder.isEmpty ? '$stem$suffix' : '$inFolder/$stem$suffix';
      if (!taken.contains(rel.toLowerCase()) && !File(project.assetFile(rel)).existsSync()) return rel;
    }
  }

  /// A new scene, prefab, material, volume profile or physics material in
  /// [inFolder] (the shown folder by default), selected and ready to rename.
  OperationResult? newDocument(String kind, {String? inFolder}) {
    final target = inFolder ?? (view == BrowserView.folders ? folder : '');
    // Unity's names: New Scene, New Material, New Volume Profile, New Physics Material.
    final rel = _freshPath(target, 'New ${documentKindTitle(kind)}', documentSuffix(kind));
    final r = _guard(() => project.newDocument(kind, rel));
    if (r == null) return null;
    final id = r.id;
    if (view == BrowserView.folders && !filtering) folder = target;
    if (id != null && entry(id) != null) {
      select(id);
      renamingAsset = id;
      notifyListeners();
    }
    return r;
  }

  /// Unity's Create ▸ Prefab Variant on the prefab [baseId]:
  /// `<Base> Variant` beside it, its root an instance of the base, selected and
  /// ready to rename.
  OperationResult? newPrefabVariant(String baseId) {
    final base = entry(baseId);
    if (base == null || base.document != 'prefab') return null;
    final slash = base.path.lastIndexOf('/');
    final inFolder = slash < 0 ? '' : base.path.substring(0, slash);
    final file = slash < 0 ? base.path : base.path.substring(slash + 1);
    final stem = file.endsWith('.prefab.json') ? file.substring(0, file.length - '.prefab.json'.length) : file;
    final rel = _freshPath(inFolder, '$stem Variant', '.prefab.json');
    final r = _guard(() => project.newDocument('prefab', rel, variantOf: {'asset': baseId}));
    if (r == null) return null;
    final id = r.id;
    if (id != null && entry(id) != null) {
      select(id);
      renamingAsset = id;
      notifyListeners();
    }
    return r;
  }

  /// Files dragged in from Finder, copied into [inFolder] (the shown
  /// folder by default) and selected, as Unity imports them.
  OperationResult? importFiles(List<String> paths, {String? inFolder}) {
    final target = inFolder ?? (view == BrowserView.folders ? folder : '');
    final r = _guard(() => project.importFiles(paths, folder: target));
    if (r == null) return null;
    final ids = [
      for (final i in r.json['imported'] as List? ?? const [])
        if (i is Map && i['id'] is String) i['id'] as String,
    ];
    _sync();
    selection
      ..clear()
      ..addAll(ids.where((id) => entry(id) != null));
    lead = selection.lastOrNull;
    leadPart = null;
    notifyListeners();
    return r;
  }

  /// An empty folder under [inFolder], shown and ready to rename.
  bool newFolder({String? inFolder}) {
    final target = inFolder ?? folder;
    _sync();
    String rel;
    for (var n = 1; ; n++) {
      final name = n == 1 ? 'New Folder' : 'New Folder $n';
      rel = target.isEmpty ? name : '$target/$name';
      if (!_folders.contains(rel) && !Directory(project.assetFile(rel)).existsSync()) break;
    }
    final err = project.createFolder(rel);
    if (err != null) {
      lastError = err;
      notifyListeners();
      return false;
    }
    lastError = null;
    refresh();
    if (view == BrowserView.folders && !filtering) folder = target;
    renamingFolder = rel;
    notifyListeners();
    return true;
  }

  void beginRename([String? id]) {
    final target = id ?? lead;
    if (target == null || !canEdit || entry(target) == null) return;
    renamingAsset = target;
    renamingFolder = null;
    notifyListeners();
  }

  void beginRenameFolder(String rel) {
    if (!canEdit || !folders.contains(rel)) return;
    renamingFolder = rel;
    renamingAsset = null;
    notifyListeners();
  }

  void cancelRename() {
    if (renamingAsset == null && renamingFolder == null) return;
    renamingAsset = null;
    renamingFolder = null;
    notifyListeners();
  }

  /// The text the inline rename starts with: a document's stem (the
  /// `.scene.json` suffix stays), a source's whole file name (the
  /// extension names the importer).
  static String editableName(AssetEntry e) {
    final file = p.basename(e.path);
    if (e.isSource) return file;
    final suffix = '.${e.document}.json';
    return file.endsWith(suffix) ? file.substring(0, file.length - suffix.length) : file;
  }

  /// The target path for [e] renamed to [typed] in its folder: the
  /// document suffix or the source extension is put back when left off.
  static String renamedPath(AssetEntry e, String typed) {
    var name = typed.trim().replaceAll('/', '');
    if (e.isSource) {
      final ext = p.extension(e.path);
      if (p.extension(name).toLowerCase() != ext.toLowerCase()) name = '$name$ext';
    } else {
      final suffix = '.${e.document}.json';
      if (!name.endsWith(suffix)) name = '$name$suffix';
    }
    final dir = _folderOf(e);
    return dir.isEmpty ? name : '$dir/$name';
  }

  /// Renames the asset [id] to [typed] (see [renamedPath]); an unchanged
  /// or empty name is not an operation.
  bool rename(String id, String typed) {
    renamingAsset = null;
    final e = entry(id);
    if (e == null || typed.trim().isEmpty) {
      notifyListeners();
      return false;
    }
    final to = renamedPath(e, typed);
    if (to == e.path) {
      notifyListeners();
      return false;
    }
    return _guard(() => project.renameAsset(id, to)) != null;
  }

  /// Moves [ids] into [toFolder] (`''` is the root), one session operation
  /// each; stops at the first refusal.
  int move(Iterable<String> ids, String toFolder) {
    var moved = 0;
    for (final id in ids.toList()) {
      final e = entry(id);
      if (e == null || _folderOf(e) == toFolder) continue;
      final file = p.basename(e.path);
      final to = toFolder.isEmpty ? file : '$toFolder/$file';
      if (_guard(() => project.renameAsset(id, to)) == null) break;
      moved++;
    }
    return moved;
  }

  /// Whether [ids] may be dropped into [toFolder]: at least one would move.
  bool canMove(Iterable<String> ids, String toFolder) =>
      canEdit && ids.any((id) => entry(id) != null && _folderOf(entry(id)!) != toFolder);

  /// Renames the folder [rel] to [typed]: every asset under it moves
  /// through the session (each its own undo step), then the empty
  /// directory goes. An empty folder is only a directory rename.
  bool renameFolder(String rel, String typed) {
    renamingFolder = null;
    final name = typed.trim().replaceAll('/', '');
    if (name.isEmpty || name == p.basename(rel)) {
      notifyListeners();
      return false;
    }
    final parent = _parentOf(rel);
    final to = parent.isEmpty ? name : '$parent/$name';
    _sync();
    if (_folders.contains(to) || Directory(project.assetFile(to)).existsSync()) {
      lastError = 'A folder named $name is already there.';
      notifyListeners();
      return false;
    }
    final under = [
      for (final e in _entries)
        if (e.path.startsWith('$rel/')) e,
    ];
    if (under.isEmpty) {
      try {
        Directory(project.assetFile(rel)).renameSync(project.assetFile(to));
      } on FileSystemException catch (e) {
        lastError = 'Cannot rename $rel: ${e.osError?.message ?? e.message}';
        notifyListeners();
        return false;
      }
    } else {
      for (final e in under) {
        final target = '$to/${e.path.substring(rel.length + 1)}';
        if (_guard(() => project.renameAsset(e.id, target)) == null) return false;
      }
      _removeEmptyTree(project.assetFile(rel));
    }
    if (folder == rel || folder.startsWith('$rel/')) folder = to + folder.substring(rel.length);
    lastError = null;
    refresh();
    return true;
  }

  /// Deletes the folder [rel] when nothing is under it; a folder with
  /// assets is refused — move or delete them first.
  bool deleteFolder(String rel) {
    _sync();
    final under = _entries.where((e) => e.path.startsWith('$rel/')).length;
    if (under > 0) {
      lastError = '$rel holds $under asset${under == 1 ? '' : 's'} — move or delete them first.';
      notifyListeners();
      return false;
    }
    if (!_removeEmptyTree(project.assetFile(rel))) {
      lastError = '$rel is not empty (it holds files the database does not index).';
      notifyListeners();
      return false;
    }
    if (folder == rel || folder.startsWith('$rel/')) folder = _parentOf(rel);
    lastError = null;
    refresh();
    return true;
  }

  /// Removes a directory tree that holds only directories; false when a
  /// file is in the way (nothing is removed then).
  static bool _removeEmptyTree(String dir) {
    final d = Directory(dir);
    if (!d.existsSync()) return true;
    bool onlyDirs(Directory x) => x.listSync(followLinks: false).every((c) => c is Directory && onlyDirs(c));
    try {
      if (!onlyDirs(d)) return false;
      d.deleteSync(recursive: true);
      return true;
    } on FileSystemException {
      return false;
    }
  }

  /// Deletes [ids] (the selection by default). A refusal because of
  /// references parks a [pendingDelete] for the panel's "delete anyway";
  /// [force] deletes with the dependents as warnings.
  bool delete({Iterable<String>? ids, bool force = false}) {
    final targets = (ids ?? selection).toList();
    if (targets.isEmpty) return false;
    pendingDelete = null;
    if (!force) {
      // Ask before touching anything: the whole delete waits for the answer
      // when any target is still referenced (the session would refuse it).
      final all = <Diagnostic>[];
      var referenced = 0;
      for (final id in targets) {
        final dependents = project.dependents(id);
        if (dependents.isEmpty) continue;
        referenced++;
        all.addAll([
          for (final r in dependents)
            Diagnostic(
              code: 'FYN-A0004',
              severity: Severity.error,
              message: '${r['path']} references ${entry(id)?.path ?? id}',
              document: project.assetFile(r['path'] as String? ?? ''),
              pointer: r['pointer'] as String?,
              id: r['document'] as String?,
            ),
        ]);
      }
      if (all.isNotEmpty) {
        final what = referenced == 1
            ? '${targets.length == 1 ? 'the asset' : 'one of them'} is still referenced'
            : '$referenced of them are still referenced';
        pendingDelete = DeleteRefusal(ids: targets, message: 'Not deleted: $what.', dependents: all);
        lastError = null;
        notifyListeners();
        return false;
      }
    }
    for (final id in targets) {
      try {
        project.deleteAsset(id, force: force);
      } on AuthoringException catch (e) {
        final d = e.diagnostics.firstOrNull;
        if (!force && d != null && d.code == 'FYN-T0003' && d.related.isNotEmpty) {
          // The session knows a reference the dependents query did not show: ask with its list.
          pendingDelete = DeleteRefusal(ids: targets, message: d.message, dependents: d.related);
          lastError = null;
          notifyListeners();
          return false;
        }
        lastError = _messageOf(e);
        _sync();
        notifyListeners();
        return false;
      }
    }
    lastError = null;
    _sync();
    notifyListeners();
    return true;
  }

  /// Delete as a key or the menu does it: asks first ([pendingConfirm]);
  /// a referenced asset goes straight to [delete], whose refusal is the
  /// question.
  void requestDelete() {
    final targets = selection.toList();
    if (targets.isEmpty || !canEdit) return;
    if (targets.any((id) => project.dependents(id).isNotEmpty)) {
      delete(ids: targets);
      return;
    }
    pendingConfirm = targets;
    notifyListeners();
  }

  /// The answer to [pendingConfirm].
  bool answerDelete(bool confirmed) {
    final ids = pendingConfirm;
    pendingConfirm = null;
    if (confirmed && ids != null) return delete(ids: ids);
    notifyListeners();
    return false;
  }

  /// Duplicates the selected assets beside themselves (`coin 1`, …) and
  /// selects the copies — one asset-history step each.
  bool duplicate() {
    final targets = [
      for (final id in selection)
        if (entry(id) != null) id,
    ];
    if (targets.isEmpty || !canEdit) return false;
    final copies = <String>[];
    for (final id in targets) {
      final r = _guard(() => project.duplicateAsset(id));
      final copy = r?.entry?['id'] as String?;
      if (copy == null) break;
      copies.add(copy);
    }
    if (copies.isEmpty) return false;
    _sync();
    selection
      ..clear()
      ..addAll(copies);
    lead = copies.last;
    notifyListeners();
    return true;
  }

  void cancelDelete() {
    if (pendingDelete == null) return;
    pendingDelete = null;
    notifyListeners();
  }

  OperationResult? setImportSettings(String id, Map<String, Object?> settings) =>
      _guard(() => project.setImportSettings(id, settings));

  /// Reimports [id]; the importer's refusal comes back as a result with
  /// `ok` false (its diagnostics say why), never as [lastError].
  OperationResult? reimport(String id) => _guard(() => project.reimport(id));

  /// Set as Startup Scene: the scene `fynkel run` starts with.
  OperationResult? setStartupScene(String id) => _guard(() => project.setStartupScene(id));

  /// Extract Materials: [id]'s embedded materials as documents, remapped.
  OperationResult? extractMaterials(String id) => _guard(() => project.extractMaterials(id));

  OperationResult? undo() => history.canUndo ? _guard(project.undoAsset) : null;
  OperationResult? redo() => history.canRedo ? _guard(project.redoAsset) : null;

  // ── Scripts (roadmap Phase 2 item 1) ────────────────────────────────

  /// `lib/`: the game's Dart source, the Scripts root.
  String get scriptsDir => p.join(project.dir, 'lib');

  /// Whether the project has a `lib/` to show (a Dart package's).
  bool get hasScripts => Directory(scriptsDir).existsSync();

  /// The browser shows `lib/` rather than the assets.
  bool inScripts = false;

  /// The scripts folder shown, relative to `lib/` (`''` is `lib/` itself).
  String scriptFolder = '';

  /// The selected script, relative to `lib/` (one at a time: scripts only
  /// open, they do not move or rename here).
  String? scriptLead;

  /// What New Behaviour is doing while `fynkel codegen` runs, for the
  /// status bar; null when idle.
  String? scriptsBusy;

  StreamSubscription<List<String>>? _files;
  ScriptTree? _scripts;
  Map<String, List<String>>? _behaviourIds;

  ScriptTree get scripts => _scripts ??= ScriptTree.read(scriptsDir);

  /// The behaviour ids each script declares, from the committed metadata.
  Map<String, List<String>> get behaviourIds => _behaviourIds ??= behaviourScripts(project.behaviourMetadata());

  /// The absolute path of a `lib/`-relative script.
  String scriptFile(String rel) => p.join(scriptsDir, p.joinAll(rel.split('/')));

  void _onFiles(List<String> changed) {
    if (_disposed || !changed.any((c) => p.isWithin(scriptsDir, c))) return;
    _scripts = null;
    _behaviourIds = null;
    if (scriptLead != null && !scripts.files.contains(scriptLead)) scriptLead = null;
    if (scriptFolder.isNotEmpty && !scripts.folders.contains(scriptFolder)) scriptFolder = '';
    notifyListeners();
  }

  bool get _searchingScripts => query.trim().isNotEmpty;

  /// The sub-folders of [scriptFolder] (none while searching or in the list view).
  List<String> get visibleScriptFolders {
    if (view == BrowserView.list || _searchingScripts) return const [];
    return [
      for (final f in scripts.folders)
        if (_parentOf(f) == scriptFolder) f,
    ];
  }

  /// The scripts shown: the folder's own, everything under it while
  /// searching, every one in the list view. The kind filter is the assets'.
  List<String> get visibleScripts {
    final q = query.trim().toLowerCase();
    final out = [
      for (final f in scripts.files)
        if (view == BrowserView.list ||
            (_searchingScripts ? scriptFolder.isEmpty || f.startsWith('$scriptFolder/') : _parentOf(f) == scriptFolder))
          if (q.isEmpty ||
              f.toLowerCase().contains(q) ||
              (behaviourIds[f] ?? const []).any((id) => id.toLowerCase().contains(q)))
            f,
    ];
    int byName(String a, String b) => p.basename(a).toLowerCase().compareTo(p.basename(b).toLowerCase());
    out.sort(switch (sort) {
      AssetSort.path => (a, b) => a.toLowerCase().compareTo(b.toLowerCase()),
      AssetSort.kind => (a, b) {
        final r = scriptKind(a).compareTo(scriptKind(b));
        return r != 0 ? r : byName(a, b);
      },
      _ => byName,
    });
    return descending ? out.reversed.toList() : out;
  }

  /// What a script row drags (Unity's script drop): the behaviour the
  /// file declares — of several, the one named after the file, as Unity
  /// asks — as a `script` whose id is the type id. Null for a file without one.
  AssetDragPayload? scriptDragPayload(String rel) {
    final ids = behaviourIds[rel] ?? const <String>[];
    String className(String id) => id.substring(id.lastIndexOf('.') + 1);
    final id = ids.length == 1
        ? ids.single
        : ids.where((id) => snakeCase(className(id)) == p.basenameWithoutExtension(rel)).firstOrNull;
    return id == null ? null : AssetDragPayload(id: id, kind: 'script', name: className(id));
  }

  /// A script row's kind: `behaviour`, `generated` or `script`.
  String scriptKind(String rel) => behaviourIds.containsKey(rel)
      ? 'behaviour'
      : isGeneratedScript(rel)
      ? 'generated'
      : 'script';

  /// Shows the Scripts root at [rel] (under `lib/`).
  void showScripts([String rel = '']) {
    if (rel.isNotEmpty && !scripts.folders.contains(rel)) return;
    inScripts = true;
    scriptFolder = rel;
    if (view != BrowserView.folders) view = BrowserView.folders;
    notifyListeners();
  }

  void selectScript(String? rel) {
    if (rel == scriptLead) return;
    scriptLead = rel;
    notifyListeners();
  }

  /// Moves the script selection by [delta] rows.
  void stepScript(int delta) {
    final rows = visibleScripts;
    if (rows.isEmpty) return;
    var i = scriptLead == null ? -1 : rows.indexOf(scriptLead!);
    i = (i + delta).clamp(0, rows.length - 1);
    selectScript(rows[i]);
  }

  /// Up one scripts folder.
  void scriptsUp() => showScripts(_parentOf(scriptFolder));

  /// Where New Behaviour writes: the shown scripts folder, else `lib/game`
  /// when the project has one (the template's layout), else `lib/`.
  String get newBehaviourFolder => inScripts ? scriptFolder : (scripts.folders.contains('game') ? 'game' : '');

  /// Whether New Behaviour can run: an editable Dart package with `lib/`.
  bool get canNewBehaviour => canEdit && hasScripts && File(p.join(project.dir, 'pubspec.yaml')).existsSync();

  /// Create → Behaviour, first half: [className]'s file from the template
  /// in [inFolder] (the [newBehaviourFolder] by default), shown and
  /// selected. Null, with [lastError] saying why, when refused.
  NewBehaviour? writeNewBehaviour(String className, {String? inFolder}) {
    final metadata = project.behaviourMetadata();
    final taken = {
      for (final b in metadata?['behaviours'] as List? ?? const [])
        if (b is Map && b['id'] is String) b['id'] as String,
    };
    final NewBehaviour b;
    try {
      b = writeBehaviour(
        libDir: scriptsDir,
        folder: inFolder ?? newBehaviourFolder,
        className: className,
        namespace: behaviourNamespace(metadata),
        taken: taken,
      );
    } on FormatException catch (e) {
      lastError = e.message;
      notifyListeners();
      return null;
    }
    lastError = null;
    _scripts = null;
    inScripts = true;
    scriptFolder = _parentOf(b.rel);
    scriptLead = b.rel;
    view = BrowserView.folders;
    notifyListeners();
    return b;
  }

  /// Create → Behaviour, second half: [generate] (`fynkel codegen`) writes
  /// the metadata and schema, then the session registers the type
  /// (`registerBehaviour`, undoable) so the Inspector's Add Component lists
  /// it at once. Returns the problem, or null.
  Future<String?> generateBehaviour(NewBehaviour b, Future<String?> Function() generate) async {
    scriptsBusy = 'Generating ${b.id} (fynkel codegen)…';
    notifyListeners();
    final problem = await generate();
    if (_disposed) return problem;
    scriptsBusy = null;
    if (problem != null) {
      lastError = 'lib/${b.rel} was written, but fynkel codegen failed: $problem';
      notifyListeners();
      return lastError;
    }
    final r = _guard(() => project.registerBehaviour(b.id, description: '${b.className} (lib/${b.rel}).'));
    if (r == null) return lastError;
    if (r.json['inMetadata'] != true) {
      lastError = 'fynkel codegen did not list ${b.id}: check lib/${b.rel} for errors.';
      notifyListeners();
      return lastError;
    }
    return null;
  }

  @override
  void dispose() {
    _disposed = true;
    unawaited(_files?.cancel());
    project.removeListener(_onProject);
    super.dispose();
  }
}
