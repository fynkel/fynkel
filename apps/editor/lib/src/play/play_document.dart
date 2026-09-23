// Live Play mode (ADR-0021 P7, docs/design/live-play-mode.md): the running
// game's world as an `EditorDocument`, so the Hierarchy and the Inspector
// show it with the panels, rows, cards and field widgets they use for a
// scene. The world is read, never written through here yet (L3), and no
// document is touched: Stop drops it all.
//
// Entities are named by their authoring ids (what a loaded scene spawned,
// a cooked prefab instance's children included) or `runtime:<handle>`
// (what the game made). Components keep the ids the open scene gives them
// when it has the entity — so the selection and Apply to Scene (L4) line
// up — and get `<entity>/<type>` ids otherwise.
//
// Refresh is a poll ([refresh], about ten times a second while Play runs):
// the entity table comes back only when it changed (the world hashes it)
// and is then compared row by row, and the entities someone
// watches (the Inspector's selection) are re-read whole. A changed row is
// reported as an `applied` edit with those ids, which the tab patches its
// tree from like any transaction; a watched entity whose values alone
// moved only bumps [PlayDocument.values], so the Hierarchy is left alone.
import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:fynkel/fynkel.dart' as fynkel;
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/editor_document.dart';
import 'live_preview.dart';

/// A script component of a running entity, as the document stores one.
final class LiveBehaviour {
  const LiveBehaviour({required this.type, required this.data, this.version = 1, this.authoredComponentId});

  final String type;
  final int version;
  final Map<String, Object?> data;

  /// The scene component it was loaded from, if one was.
  final String? authoredComponentId;
}

/// What a [PlayDocument] reads: the running world (and its script
/// components). [WorldLiveBackend] is the real one; widget tests use a fake.
abstract class LiveBackend {
  List<LiveEntity> entities();

  /// [entities], or null when nothing in the table changed since the last
  /// read (what a refresh asks, many times a second).
  List<LiveEntity>? changedEntities();
  LiveEntityState entity(String id);

  /// The script components on the entity with runtime [handle].
  List<LiveBehaviour> behaviours(int handle);

  /// The world for the Scene view to draw and pick in (L5); null when
  /// there is none.
  PreviewBackend? openPreview();
}

/// A package:fynkel world the game is running.
final class WorldLiveBackend implements LiveBackend {
  WorldLiveBackend(this.world) : live = LiveWorld(world.native.handleAddress);

  final fynkel.World world;
  final LiveWorld live;

  @override
  List<LiveEntity> entities() => live.entities();

  @override
  List<LiveEntity>? changedEntities() => live.changedEntities();

  @override
  LiveEntityState entity(String id) => live.entity(id);

  @override
  List<LiveBehaviour> behaviours(int handle) => [
    for (final b in world.behaviours)
      if (!b.isDestroyed && !b.gameObject.isPending && b.gameObject.entity == handle)
        if (b.behaviourType case final type?)
          LiveBehaviour(
            type: type.id,
            version: type.version,
            data: {for (final f in type.fields) f.name: f.get(b)},
            authoredComponentId: b.authoredComponentId,
          ),
  ];

  @override
  PreviewBackend openPreview() => LivePreview(live, alive: () => !world.native.isDisposed);
}

final class PlayDocument extends EditorDocument {
  PlayDocument({required this.backend, this.scene, Duration interval = const Duration(milliseconds: 100)}) {
    _read(backend.entities());
    _poll = Timer.periodic(interval, (_) => refresh());
  }

  final LiveBackend backend;

  /// The scene being played, when it is open: its component ids are reused
  /// for the entities it has.
  final EditorDocument? scene;

  /// The ids to re-read whole on every refresh (the Inspector's selection).
  Iterable<String> Function()? watched;

  Timer? _poll;
  bool _closed = false;
  DocumentEdit? _lastEdit;
  int _revision = 0;

  /// Row signature and parent per id, in the world's order.
  Map<String, String> _rows = {};
  Map<String, LiveEntity> _byId = {};
  Map<String, List<String>> _children = {};
  List<String> _roots = [];

  /// The last whole read of an entity, as the document stores it, by id.
  final Map<String, Map<String, Object?>> _full = {};
  final Map<String, String> _fullJson = {};

  /// Bumped when a watched entity's values changed and no row did (the
  /// Inspector listens; nothing else needs to).
  final values = ValueNotifier<int>(0);

  /// The watched entities whose values the last [values] bump was for.
  List<String> lastMoved = const [];

  /// Refreshes run and how long the last one took (the design's budget is
  /// 0.5 ms for the reference game's Playground).
  int refreshes = 0;
  Duration lastRefresh = Duration.zero;

  // ── EditorDocument ───────────────────────────────────────────────────

  @override
  String get path => scene?.path ?? '';
  @override
  String? get relativePath => scene?.relativePath;
  @override
  String get kind => 'scene';
  @override
  String get id => 'play:${scene?.id ?? ''}';
  @override
  String get name => scene?.name ?? 'Playing';
  @override
  String get revision => 'play-$_revision';
  @override
  bool get dirty => false;
  @override
  bool get canUndo => false;
  @override
  bool get canRedo => false;
  @override
  int get history => 0;
  @override
  String? get readOnly => 'The running game is shown as it is; editing it is not there yet.';
  @override
  String? get sub => null;
  @override
  bool get isClosed => _closed;
  @override
  DocumentEdit? get lastEdit => _lastEdit;

  @override
  Map<String, Object?> root() => {
    'kind': 'scene',
    'id': id,
    'name': name,
    'entities': {for (final id in _byId.keys) id: ?_document(id)},
    'roots': List<String>.of(_roots),
  };

  @override
  EntityPatch entities(Iterable<String> ids) => EntityPatch(
    entities: {for (final id in ids) id: _byId.containsKey(id) ? _document(id) : null},
    roots: List<String>.of(_roots),
    state: _state(),
  );

  @override
  List<String> roots() => List<String>.of(_roots);

  DocumentState _state() => DocumentState.fromJson({
    'path': path,
    'relativePath': relativePath,
    'kind': kind,
    'id': id,
    'name': name,
    'revision': revision,
    'readOnly': readOnly,
  });

  @override
  Applied apply(List<Map<String, Object?>> commands, {String? baseRevision}) => Applied.fromJson({
    'ok': false,
    'diagnostics': [
      {'code': 'FYN-E0007', 'severity': 'error', 'message': readOnly},
    ],
  });

  @override
  Replayed undo() => throw _refused('undo');
  @override
  Replayed redo() => throw _refused('redo');
  @override
  String save({String? path}) => throw _refused('save');
  @override
  void reload() => refresh();
  @override
  List<Diagnostic> validate() => const [];
  @override
  Map<String, Object?> inspect() => root();
  @override
  PrefabExpansion instance(String anchor) => throw _refused('instance');
  /// The running world for the Scene view (L5): opened once, closed with
  /// the document (the world itself is the game's).
  @override
  PreviewBackend openPreview() {
    final open = _preview;
    if (open != null && !open.isClosed) return open;
    if (_closed) throw _refused('openPreview');
    return _preview = backend.openPreview() ?? (throw _refused('openPreview'));
  }

  PreviewBackend? _preview;
  @override
  PreviewBackend? get preview => _preview;

  AuthoringException _refused(String what) => AuthoringException('play $what', AuthoringStatus.invalidArgument, [
    Diagnostic(code: 'FYN-T0003', severity: Severity.error, message: '$what: not on the running game'),
  ]);

  @override
  void close() {
    _poll?.cancel();
    _poll = null;
    _closed = true;
    _preview?.close();
  }

  @override
  void dispose() {
    close();
    values.dispose();
    super.dispose();
  }

  // ── Reading ──────────────────────────────────────────────────────────

  /// Reads the world again and tells the listeners what changed. Nothing
  /// is reported when nothing did.
  void refresh() {
    if (_closed) return;
    final watch = Stopwatch()..start();
    final before = _rows;
    final List<String> affected;
    final moved = <String>[];
    try {
      final rows = backend.changedEntities();
      affected = rows == null ? [] : _read(rows);
      for (final id in watched?.call() ?? const <String>[]) {
        if (affected.contains(id) || !_byId.containsKey(id)) continue;
        final was = _fullJson[id];
        _full.remove(id);
        _document(id);
        if (was != null && _fullJson[id] != was) moved.add(id);
      }
    } on AuthoringException {
      // The world went (Stop is on its way) or refused: nothing to show.
      return;
    }
    refreshes++;
    lastRefresh = watch.elapsed;
    if (affected.isEmpty) {
      if (moved.isNotEmpty) {
        lastMoved = moved;
        values.value++;
      }
      return;
    }
    // A row changed: the tab re-reads those, and the Inspector everything
    // it shows (the moved entities with it).
    for (final id in affected) {
      if (!_rows.containsKey(id) || before[id] != _rows[id]) _full.remove(id);
    }
    _revision++;
    _lastEdit = DocumentEdit(DocumentChangeKind.applied, affectedIds: affected);
    notifyListeners();
  }

  /// Takes the table in; answers the ids whose row appeared, went or changed.
  List<String> _read(List<LiveEntity> rows) {
    final byId = <String, LiveEntity>{};
    final children = <String, List<String>>{};
    final roots = <String>[];
    for (final r in rows) {
      byId[r.id] = r;
      final parent = r.parent;
      if (parent == null) {
        roots.add(r.id);
      } else {
        (children[parent] ??= []).add(r.id);
      }
    }
    final next = {
      for (final r in rows)
        r.id: jsonEncode([r.name, r.parent, children[r.id] ?? const [], r.types, r.layer]),
    };
    final affected = <String>[
      for (final e in next.entries)
        if (_rows[e.key] != e.value) e.key,
      for (final id in _rows.keys)
        if (!next.containsKey(id)) id,
    ];
    for (final id in affected) {
      if (!next.containsKey(id)) {
        _full.remove(id);
        _fullJson.remove(id);
      }
    }
    _rows = next;
    _byId = byId;
    _children = children;
    _roots = roots;
    return affected;
  }

  /// The open scene's component ids for [id] by type, read once per scene
  /// revision (the scene may be edited in its tab while the game runs).
  Map<String, List<String>> _sceneIds(String id) {
    final s = scene;
    if (s == null || id.startsWith('runtime:') || s.isClosed) return const {};
    if (s.revision != _sceneRevision) {
      _sceneRevision = s.revision;
      _sceneIdCache.clear();
    }
    return _sceneIdCache[id] ??= () {
      final byType = <String, List<String>>{};
      final components = s.entities([id]).entities[id]?['components'];
      if (components is Map) {
        for (final c in components.entries) {
          final type = (c.value as Map?)?['type'];
          if (type is String) (byType[type] ??= []).add(c.key as String);
        }
      }
      return byType;
    }();
  }

  String? _sceneRevision;
  final Map<String, Map<String, List<String>>> _sceneIdCache = {};

  /// [id] as the scene document would store it, read on first use.
  Map<String, Object?>? _document(String id) {
    final cached = _full[id];
    if (cached != null) return cached;
    final row = _byId[id];
    if (row == null) return null;
    final LiveEntityState state;
    try {
      state = backend.entity(id);
    } on AuthoringException {
      return null;
    }
    final ids = _SceneComponentIds(id, _sceneIds(id));
    final components = <String, Object?>{};
    for (final c in state.components) {
      components[ids.forType(c.type)] = {'type': c.type, 'version': 1, 'data': c.data};
    }
    for (final b in backend.behaviours(row.handle)) {
      components[b.authoredComponentId ?? ids.forType(b.type)] = {'type': b.type, 'version': b.version, 'data': b.data};
    }
    final doc = <String, Object?>{
      'name': state.name,
      'parent': ?row.parent,
      'children': List<String>.of(_children[id] ?? const []),
      'components': components,
    };
    _full[id] = doc;
    _fullJson[id] = jsonEncode(doc);
    return doc;
  }
}

/// The open scene's component ids for one entity, by type, in order; an
/// id of its own for a component the scene does not have.
final class _SceneComponentIds {
  _SceneComponentIds(this.entity, this._byType);

  final String entity;
  final Map<String, List<String>> _byType;
  final Map<String, int> _used = {};

  String forType(String type) {
    final n = _used[type] = (_used[type] ?? 0) + 1;
    final ids = _byType[type];
    if (ids != null && n <= ids.length) return ids[n - 1];
    return n == 1 ? '$entity/$type' : '$entity/$type#$n';
  }
}
