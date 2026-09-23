// The open project as the shell sees it: a name, a directory, whether it is
// read-only and why, and its diagnostics. `SessionProject` is the real one
// over `AuthoringSession`; the abstract class lets widget tests run the
// shell without the native runtime.
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;
import 'package:watcher/watcher.dart';

import '../documents/editor_document.dart';

/// A document in the asset database, as the Open Document dialog lists it.
final class DocumentAsset {
  const DocumentAsset({required this.id, required this.path, required this.name, required this.kind});

  final String id;

  /// Relative to the project's assets directory.
  final String path;
  final String name;

  /// `scene`, `prefab`, `material`, `rig`, `blend`.
  final String kind;
}

/// A component type the project knows, from the `schemas` query.
final class ComponentType {
  const ComponentType({required this.type, required this.authored, required this.allowed});

  final String type;

  /// `x-fynkel.authored`: meant to be put on an entity by hand (a `Skin` is not).
  final bool authored;

  /// The project's component allow-list admits it.
  final bool allowed;
}

/// One component or behaviour schema from the `schemas` query, with what
/// the Inspector needs around it.
final class TypeSchema {
  const TypeSchema({
    required this.type,
    required this.isBehaviour,
    required this.authored,
    required this.allowed,
    this.schema,
    this.version,
    this.dart,
  });

  final String type;
  final bool isBehaviour;

  /// `x-fynkel.authored` (native); a behaviour is always authorable.
  final bool authored;

  /// The project's component allow-list admits it.
  final bool allowed;

  /// The JSON Schema, or null for a behaviour declared without a schema file.
  final Map<String, Object?>? schema;
  final int? version;

  /// A behaviour's `package:…#Class`.
  final String? dart;
}

/// One entry of the asset database (`fynkel asset list --json`).
final class AssetEntry {
  const AssetEntry({
    required this.id,
    required this.path,
    required this.name,
    required this.kind,
    this.document,
    this.importer,
    this.subAssets = const {},
  });

  final String id;

  /// Relative to the project's assets directory.
  final String path;
  final String name;

  /// `document` or `source`.
  final String kind;

  /// A document's kind (`scene`, `prefab`, `material`, `rig`, `blend`).
  final String? document;

  /// A source's importer (`fynkel.gltf`, `fynkel.fbx`).
  final String? importer;

  /// A source's sub-assets: `mesh/0`, `anim/1`, `skin/0`, `prefab`, `image/0` → uuid.
  final Map<String, String> subAssets;

  bool get isSource => kind == 'source';

  /// The kind a source is referenced as **whole** (no sub-asset): an audio
  /// source is a `sound` (`fynkel.AudioSource.clip`), an image a `texture`
  /// (a material's texture slots name the image's own id). Null for the
  /// others, which are only referenced through their sub-assets.
  String? get wholeKind => switch (isSource ? importer : null) {
    'fynkel.audio' => 'sound',
    'fynkel.image' => 'texture',
    _ => null,
  };

  /// The Inspector's asset kinds this entry (or one of its subs) offers:
  /// a document's kind; a source's `mesh`, `clip` (anim), `skin`, `prefab`,
  /// `material`, and its [wholeKind] (`sound`, `texture`).
  Set<String> get offeredKinds {
    if (!isSource) return {?document};
    final out = <String>{?wholeKind};
    for (final sub in subAssets.keys) {
      out.add(subKind(sub));
    }
    return out;
  }

  /// `mesh/1/0` → `mesh`, `anim/0` → `clip`, `image/0/linear` → `texture`.
  static String subKind(String sub) {
    final head = sub.split('/').first;
    return switch (head) {
      'anim' => 'clip',
      'image' => 'texture',
      _ => head,
    };
  }
}

abstract class EditorProject extends ChangeNotifier {
  String get dir;
  String get name;

  /// Why the project is read-only (a lock written by a newer Fynkel), or null.
  String? get readOnly;
  bool get isReadOnly => readOnly != null;

  /// Open-time diagnostics plus the last whole-project validation, de-duplicated.
  List<Diagnostic> get diagnostics;
  bool get validating;
  DateTime? get validatedAt;

  /// The validation itself failed (an `AuthoringException`), as text.
  String? get validationError;
  int get documents;
  int get sources;
  int get references;
  int get assetCount;

  /// What `fynkel migrate` would do to the documents behind their format
  /// version (null before the first validation).
  MigrationPreview? get migrations;
  int get outdatedDocuments => migrations?.outdated ?? 0;

  int get errorCount => diagnostics.where((d) => d.severity == Severity.error).length;
  int get warningCount => diagnostics.where((d) => d.severity == Severity.warning).length;

  Future<void> revalidate();
  Future<void> close();

  /// Opens a document by path (absolute, or relative to the project) or by
  /// asset id; idempotent — the same document gives the same object.
  /// Throws `AuthoringException` when it cannot be read or is not editable.
  EditorDocument openDocument(String pathOrId);

  /// The documents in the asset database, scenes first.
  List<DocumentAsset> documentAssets();

  /// The component types the project knows, in schema order.
  List<ComponentType> componentTypes();

  /// The `data` a native component starts with (its Rust `Default`); null
  /// for a behaviour or an unknown type.
  Map<String, Object?>? componentDefaults(String type);

  /// Every component and behaviour schema the project knows (the `schemas`
  /// query), components first, each in schema order.
  List<TypeSchema> typeSchemas();

  /// The committed behaviour metadata (`lib/generated/behaviours.g.json`)
  /// as read at open, or null when the project has none.
  Map<String, Object?>? behaviourMetadata();

  /// The asset database now.
  List<AssetEntry> assets();

  /// The asset root (absolute) — every [AssetEntry.path] is relative to it.
  String get assetsDir;

  /// Bumped whenever the asset database changed (a rescan after a save or a
  /// watcher event, an asset operation); the Project browser re-reads its
  /// entries when it moves.
  int get assetsRevision;

  /// The folders under the asset root, relative and `/`-separated, sorted,
  /// hidden ones skipped (`materials`, `models/characters`).
  List<String> folders() {
    final root = Directory(assetsDir);
    if (!root.existsSync()) return const [];
    final out = <String>[];
    void walk(Directory d, String prefix) {
      final List<FileSystemEntity> children;
      try {
        children = d.listSync(followLinks: false);
      } on FileSystemException {
        return;
      }
      for (final c in children) {
        if (c is! Directory) continue;
        final name = p.basename(c.path);
        if (name.startsWith('.')) continue;
        final rel = prefix.isEmpty ? name : '$prefix/$name';
        out.add(rel);
        walk(c, rel);
      }
    }

    walk(root, '');
    out.sort((a, b) => a.toLowerCase().compareTo(b.toLowerCase()));
    return out;
  }

  /// Creates an empty folder under the asset root ([rel] relative,
  /// `/`-separated). Folders are not assets: nothing to index, nothing to
  /// undo — like Save As, the editor makes the directory itself. Returns
  /// the problem, or null.
  String? createFolder(String rel) {
    try {
      Directory(p.join(assetsDir, p.joinAll(rel.split('/')))).createSync(recursive: true);
      return null;
    } on FileSystemException catch (e) {
      return 'Cannot create $rel: ${e.osError?.message ?? e.message}';
    }
  }

  /// One asset in full (the `assetDetail` query), or null when [id] is not
  /// in the database.
  AssetDetail? assetDetail(String id);

  /// Every reference to [id]: `{ document, path, pointer, asset, sub? }`,
  /// one per referencing field.
  List<Map<String, Object?>> dependents(String id);

  /// The asset operations' undo/redo state.
  AssetHistory assetHistory();

  /// An asset operation through the session (`newDocument`, `renameAsset`,
  /// `deleteAsset`, `setImportSettings`, `reimport`, `setLayersAndTags`,
  /// `undoAsset`, `redoAsset`) — one history step each, the database re-read
  /// incrementally. Throws `AuthoringException` when refused; nothing was
  /// touched then.
  OperationResult operate(String kind, [Map<String, Object?> args = const {}]);

  OperationResult newDocument(String kind, String path, {String? name, String? shader}) =>
      operate('newDocument', {'kind': kind, 'path': path, 'name': ?name, 'shader': ?shader});
  OperationResult renameAsset(String id, String path) => operate('renameAsset', {'asset': id, 'path': path});
  OperationResult deleteAsset(String id, {bool force = false}) =>
      operate('deleteAsset', {'asset': id, if (force) 'force': true});
  OperationResult setImportSettings(String id, Map<String, Object?> settings) =>
      operate('setImportSettings', {'asset': id, 'settings': settings});
  OperationResult reimport(String id) => operate('reimport', {'asset': id});
  OperationResult undoAsset() => operate('undoAsset');
  OperationResult redoAsset() => operate('redoAsset');

  /// The project's layers and tags replaced (ADR-0020); `*From[i]` is the
  /// old index new entry `i` comes from (null: new). A reorder or a
  /// removal rewrites the scenes and prefabs that use them.
  OperationResult setLayersAndTags({
    required List<String> layers,
    required List<String> tags,
    List<int?>? layersFrom,
    List<int?>? tagsFrom,
  }) => operate('setLayersAndTags', {'layers': layers, 'tags': tags, 'layersFrom': ?layersFrom, 'tagsFrom': ?tagsFrom});

  /// The absolute path of an asset-root-relative [path].
  String assetFile(String path) => p.join(assetsDir, p.joinAll(path.split('/')));

  /// Whether [id] (and [sub], when given) resolves in the asset database.
  bool assetExists(String id, {String? sub}) {
    for (final a in assets()) {
      if (a.id == id) return sub == null || sub.isEmpty || a.subAssets.containsKey(sub);
      if (a.subAssets.containsValue(id)) return sub == null || sub.isEmpty;
    }
    return false;
  }

  /// The project file's `layers` (bit `i` = layer `i`, ADR-0020), or the
  /// default `["default"]`.
  List<String> layerNames() {
    final layers = _projectList('layers');
    return layers == null || layers.isEmpty ? const ['default'] : layers;
  }

  /// The project file's `tags` (bit `i` of an object's tag set, ADR-0020),
  /// or none.
  List<String> tagNames() => _projectList('tags') ?? const [];

  List<String>? _projectList(String key) {
    try {
      final decoded = jsonDecode(File(p.join(dir, 'fynkel.project.json')).readAsStringSync());
      final list = decoded is Map ? decoded[key] : null;
      if (list is List) return [for (final l in list) l.toString()];
    } catch (_) {
      // A missing or malformed project file: the validation reports it; the fields show bit numbers.
    }
    return null;
  }

  /// The revision of a document file on disk now, or null when it cannot be read.
  String? fileRevision(String path);

  /// Absolute paths the file watcher saw change, one list per debounce
  /// window — the open documents check whether they are among them.
  Stream<List<String>> get fileChanges;

  /// A project path shown to the user: relative to the project directory.
  String relative(String path) => p.isWithin(dir, path) || p.equals(dir, path) ? p.relative(path, from: dir) : path;
}

/// The project over the authoring session, with a directory watcher that
/// rescans changed asset paths and revalidates (debounced).
final class SessionProject extends EditorProject {
  SessionProject._(this.session, this._info, {required this.dir}) {
    _events = session.events.listen((e) {
      if (e is DocumentClosed) _open.remove(e.handle)?.dispose();
      if (e is AssetsChanged) {
        _assetsRevision++;
        _info = session.project ?? _info;
        notifyListeners();
      }
    });
  }

  /// Opens [dir]; throws `AuthoringException` when it is not a project.
  /// [watch] false turns the file watcher off (tests).
  static Future<SessionProject> open(String dir, {bool watch = true, AuthoringSession? session}) async {
    final s = session ?? AuthoringSession();
    final ProjectInfo info;
    try {
      info = s.openProject(dir);
    } catch (_) {
      if (session == null) s.dispose();
      rethrow;
    }
    final project = SessionProject._(s, info, dir: info.dir.isEmpty ? dir : info.dir);
    await project.revalidate();
    if (watch) project._watch();
    return project;
  }

  final AuthoringSession session;
  ProjectInfo _info;
  ProjectReport? _report;
  bool _validating = false;
  DateTime? _validatedAt;
  String? _validationError;
  StreamSubscription<WatchEvent>? _watcher;
  StreamSubscription<AuthoringEvent>? _events;
  Timer? _debounce;
  final _pending = <String>{};
  final _fileChanges = StreamController<List<String>>.broadcast();
  final _open = <int, SessionDocument>{};
  bool _closed = false;

  @override
  Stream<List<String>> get fileChanges => _fileChanges.stream;

  @override
  EditorDocument openDocument(String pathOrId) {
    final handle = session.openDocument(pathOrId);
    return _open.putIfAbsent(handle.handle, () => SessionDocument(handle));
  }

  int _assetsRevision = 0;
  @override
  int get assetsRevision => _assetsRevision;

  @override
  String get assetsDir => _info.assetsDir.isEmpty ? p.join(dir, 'assets') : _info.assetsDir;

  @override
  AssetDetail? assetDetail(String id) {
    try {
      return session.assetDetail(id);
    } on AuthoringException {
      return null;
    }
  }

  @override
  List<Map<String, Object?>> dependents(String id) {
    try {
      return session.dependents(id);
    } on AuthoringException {
      return const [];
    }
  }

  @override
  AssetHistory assetHistory() {
    try {
      return session.assetHistory();
    } on AuthoringException {
      return const AssetHistory.empty();
    }
  }

  /// The session re-reads the touched paths and emits `AssetsChanged` (the
  /// revision moves there); the whole-project validation follows in the
  /// background so a dangling reference after a forced delete shows up.
  @override
  OperationResult operate(String kind, [Map<String, Object?> args = const {}]) {
    final r = session.operate(kind, args);
    _info = session.project ?? _info;
    unawaited(revalidate());
    return r;
  }

  @override
  List<DocumentAsset> documentAssets() {
    const order = ['scene', 'prefab', 'material', 'rig', 'blend', 'volume'];
    final out = [
      for (final a in session.assets(kind: 'document'))
        DocumentAsset(
          id: a['id'] as String? ?? '',
          path: a['path'] as String? ?? '',
          name: a['name'] as String? ?? '',
          kind: a['document'] as String? ?? '',
        ),
    ];
    int rank(String k) => order.contains(k) ? order.indexOf(k) : order.length;
    out.sort((x, y) {
      final r = rank(x.kind).compareTo(rank(y.kind));
      return r != 0 ? r : x.path.toLowerCase().compareTo(y.path.toLowerCase());
    });
    return out;
  }

  @override
  Map<String, Object?>? componentDefaults(String type) {
    try {
      return session.componentDefaults(type);
    } on AuthoringException {
      return null;
    }
  }

  @override
  String? fileRevision(String path) {
    try {
      return session.fileRevision(path).revision;
    } on AuthoringException {
      return null;
    }
  }

  @override
  List<ComponentType> componentTypes() => [
    for (final c in session.schemas()['components'] as List? ?? const [])
      if (c is Map<String, Object?>)
        ComponentType(
          type: c['type'] as String? ?? '',
          authored: c['authored'] == true,
          allowed: c['allowed'] != false,
        ),
  ];

  @override
  List<TypeSchema> typeSchemas() {
    final all = session.schemas();
    Map<String, Object?>? schemaOf(Object? v) =>
        v is Map ? {for (final e in v.entries) e.key as String: e.value} : null;
    return [
      for (final c in all['components'] as List? ?? const [])
        if (c is Map<String, Object?>)
          TypeSchema(
            type: c['type'] as String? ?? '',
            isBehaviour: false,
            authored: c['authored'] == true,
            allowed: c['allowed'] != false,
            schema: schemaOf(c['schema']),
          ),
      for (final b in all['behaviours'] as List? ?? const [])
        if (b is Map<String, Object?>)
          TypeSchema(
            type: b['type'] as String? ?? '',
            isBehaviour: true,
            authored: true,
            allowed: b['allowed'] != false,
            schema: b['hasSchema'] == false ? null : schemaOf(b['schema']),
            version: (b['version'] as num?)?.toInt(),
            dart: b['dart'] as String?,
          ),
    ];
  }

  @override
  Map<String, Object?>? behaviourMetadata() {
    try {
      return session.metadata();
    } on AuthoringException {
      return null;
    }
  }

  @override
  List<AssetEntry> assets() => [
    for (final a in session.assets())
      AssetEntry(
        id: a['id'] as String? ?? '',
        path: a['path'] as String? ?? '',
        name: a['name'] as String? ?? '',
        kind: a['kind'] as String? ?? '',
        document: a['document'] as String?,
        importer: a['importer'] as String?,
        subAssets: {
          if (a['subAssets'] is Map)
            for (final e in (a['subAssets'] as Map).entries) e.key as String: e.value.toString(),
        },
      ),
  ];

  @override
  final String dir;
  @override
  String get name => _info.name.isEmpty ? p.basename(dir) : _info.name;
  @override
  String? get readOnly => _info.readOnly;
  @override
  bool get validating => _validating;
  @override
  DateTime? get validatedAt => _validatedAt;
  @override
  String? get validationError => _validationError;
  @override
  int get documents => _report?.documents ?? 0;
  @override
  int get sources => _report?.sources ?? 0;
  @override
  int get references => _report?.references ?? 0;
  @override
  int get assetCount => _info.assetCount;
  ProjectInfo get info => _info;
  ProjectReport? get report => _report;
  MigrationPreview? _migrations;
  @override
  MigrationPreview? get migrations => _migrations;

  @override
  List<Diagnostic> get diagnostics {
    final seen = <String>{};
    final out = <Diagnostic>[];
    for (final d in [..._info.diagnostics, ...?_report?.diagnostics]) {
      if (seen.add('${d.code}|${d.document}|${d.pointer}|${d.message}')) out.add(d);
    }
    return out;
  }

  /// Runs the whole-project validation `fynkel validate` runs.
  @override
  Future<void> revalidate() async {
    if (_closed) return;
    _validating = true;
    notifyListeners();
    // Let the frame that shows "validating" paint before the synchronous call.
    await Future<void>.delayed(Duration.zero);
    if (_closed) return;
    try {
      _report = session.validateProject();
      _migrations = session.migrationPreview();
      _validationError = null;
    } on AuthoringException catch (e) {
      _validationError = e.toString();
    }
    _validating = false;
    _validatedAt = DateTime.now();
    notifyListeners();
  }

  static const _ignoredDirs = {'.fynkel', '.dart_tool', 'build', 'fynkel_cache', '.git', 'ios', 'macos', '.idea'};

  void _watch() {
    try {
      _watcher = DirectoryWatcher(dir).events.listen(_onEvent, onError: (_) {});
    } catch (_) {
      _watcher = null;
    }
  }

  void _onEvent(WatchEvent event) {
    final rel = p.split(relative(event.path));
    if (rel.any(_ignoredDirs.contains)) return;
    _pending.add(event.path);
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 400), _flush);
  }

  Future<void> _flush() async {
    if (_closed) return;
    final changed = _pending.toList();
    _pending.clear();
    _fileChanges.add(changed);
    final root = assetsDir;
    final assets = changed.where((c) => p.isWithin(root, c)).toList();
    try {
      // The rescan emits AssetsChanged, which moves the revision; a change
      // that indexes nothing (a folder appeared) still moves it here so the
      // browser's tree follows.
      if (assets.isNotEmpty) {
        _assetsRevision++;
        session.rescan(assets);
      }
      _info = session.refreshProject();
    } on AuthoringException catch (e) {
      _validationError = e.toString();
    }
    await revalidate();
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    _debounce?.cancel();
    await _watcher?.cancel();
    await _events?.cancel();
    await _fileChanges.close();
    for (final d in _open.values) {
      d.dispose();
    }
    _open.clear();
    try {
      session.closeProject();
    } catch (_) {
      // A poisoned session has nothing left to close.
    }
    session.dispose();
    notifyListeners();
  }

  @override
  void dispose() {
    if (!_closed) close();
    super.dispose();
  }
}

/// True when [dir] holds a project file (the launcher's Open check).
bool isFynkelProject(String dir) => File(p.join(dir, 'fynkel.project.json')).existsSync();
