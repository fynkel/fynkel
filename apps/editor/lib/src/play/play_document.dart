import 'package:fynkel/fynkel.dart' show EntityRef;
// Live Play mode (ADR-0021 P7, docs/design/live-play-mode.md): the running
// game's world as an `EditorDocument`, so the Hierarchy and the Inspector
// show it with the panels, rows, cards and field widgets they use for a
// scene. Edits go to the running world at once (L3) — a field, a
// component added or removed, a rename, the active checkbox, a new
// parent, a delete, a new or duplicated object — and no document is touched: Stop drops it all,
// unless Apply to Scene kept it (L4, `apply_to_scene.dart`).
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
import '../inspector/prefab_actions.dart' show newUuid;
import 'live_preview.dart';

/// A script component of a running entity, as the document stores one.
final class LiveBehaviour {
  const LiveBehaviour({
    required this.type,
    required this.data,
    this.version = 1,
    this.authoredComponentId,
    this.instance,
  });

  final String type;
  final int version;
  final Map<String, Object?> data;

  /// The scene component it was loaded from, if one was.
  final String? authoredComponentId;

  /// The behaviour itself, for the writes ([LiveBackend.setBehaviourField],
  /// [LiveBackend.removeBehaviour]).
  final Object? instance;
}

/// What a [PlayDocument] reads and writes: the running world (and its
/// script components). [WorldLiveBackend] is the real one; widget tests use
/// a fake. The writes answer the world's refusals (empty when it applied)
/// and throw an [AuthoringException] when the call itself failed.
abstract class LiveBackend {
  List<LiveEntity> entities();

  /// [entities], or null when nothing in the table changed since the last
  /// read (what a refresh asks, many times a second).
  List<LiveEntity>? changedEntities();
  LiveEntityState entity(String id);

  /// The script components on the entity with runtime [handle].
  List<LiveBehaviour> behaviours(EntityRef handle);

  /// The world for the Scene view to draw and pick in (L5); null when
  /// there is none.
  PreviewBackend? openPreview();

  /// A native write: `set`, `remove`, `rename`, `setActive`, `parent` or `destroy` with
  /// [LiveWorld]'s arguments.
  List<String> write(String kind, Map<String, Object?> args) => throw _notWritable(kind);

  List<String> setBehaviourField(Object instance, String field, Object? value) => throw _notWritable('setField');

  List<String> addBehaviour(EntityRef handle, String type, {int? version, Map<String, Object?> data = const {}}) =>
      throw _notWritable('addComponent');

  List<String> removeBehaviour(Object instance) => throw _notWritable('removeComponent');

  /// A material or volume profile ([kind]) edited in the Inspector during
  /// Play: the running game draws asset [id] with [fields] from its next
  /// frame ([LiveWorld.setMaterial]). False when this world takes none.
  bool setAsset(String kind, String id, Map<String, Object?> fields) => false;

  /// Makes an entity at once ([LiveWorld.spawn]): answers its id and the
  /// components the world refused.
  ({String id, List<String> refused}) spawn({
    required String name,
    String? parent,
    int? index,
    List<Map<String, Object?>> components = const [],
    String? authoringId,
  }) => throw _notWritable('createObject');

  static AuthoringException _notWritable(String what) => AuthoringException(
    'live $what',
    AuthoringStatus.invalidArgument,
    [Diagnostic(code: 'FYN-T0003', severity: Severity.error, message: '$what: this world is not written')],
  );
}

/// A package:fynkel world the game is running.
final class WorldLiveBackend extends LiveBackend {
  WorldLiveBackend(this.world) : live = LiveWorld(world.native.handleAddress) {
    // A script's object field reads back under the ids the table shows.
    world.liveIdOf = (handle) => _ids[handle];
  }

  final fynkel.World world;
  final LiveWorld live;

  @override
  List<LiveEntity> entities() => _keep(live.entities());

  @override
  bool setAsset(String kind, String id, Map<String, Object?> fields) {
    switch (kind) {
      case 'material':
        live.setMaterial(id, fields);
      case 'volume':
        live.setVolume(id, fields);
      default:
        return false;
    }
    return true;
  }

  @override
  List<LiveEntity>? changedEntities() {
    final rows = live.changedEntities();
    return rows == null ? null : _keep(rows);
  }

  /// The live ids by handle, from the last table read: what a script's
  /// object field shows (`{entity: id}`) for the object it holds.
  final Map<EntityRef, String> _ids = {};

  List<LiveEntity> _keep(List<LiveEntity> rows) {
    _ids
      ..clear()
      ..addEntries(rows.map((r) => MapEntry(r.handle, r.id)));
    return rows;
  }

  /// An object field's `{entity, component?}` as the object in the world
  /// (null when it is not there).
  Object? _resolveReference(Object? value) {
    if (value is! Map) return null;
    final entity = value['entity'];
    if (entity is! String) return null;
    final EntityRef handle;
    try {
      handle = live.entity(entity).handle;
    } on AuthoringException {
      return null;
    }
    final go = world.byEntity(handle) ?? world.attach(handle);
    final component = value['component'];
    if (component is! String) return go;
    for (final b in go.behaviours) {
      if (!b.isDestroyed && b.authoredComponentId == component) return b;
    }
    return null;
  }

  @override
  LiveEntityState entity(String id) => live.entity(id);

  /// Removed behaviours go at the world's next step; until then (a paused
  /// game) they are not shown.
  final Set<fynkel.Behaviour> _removed = Set.identity();

  @override
  List<LiveBehaviour> behaviours(EntityRef handle) => [
    for (final b in world.behaviours)
      if (!b.isDestroyed && !_removed.contains(b) && !b.gameObject.isPending && b.gameObject.entity == handle)
        if (b.behaviourType case final type?)
          LiveBehaviour(
            type: type.id,
            version: type.version,
            data: {for (final f in type.fields) f.name: f.get(b)},
            authoredComponentId: b.authoredComponentId,
            instance: b,
          ),
  ];

  @override
  PreviewBackend openPreview() => LivePreview(live, alive: () => !world.native.isDisposed);

  @override
  List<String> write(String kind, Map<String, Object?> args) {
    final id = args['id'] as String;
    return switch (kind) {
      'set' => live.set(id, args['type'] as String, (args['data'] as Map).cast()),
      'remove' => live.remove(id, args['type'] as String),
      'rename' => _rename(id, args['name'] as String),
      'setActive' => live.setActive(id, args['active'] as bool),
      'parent' => live.reparent(id, args['parent'] as String?, index: args['index'] as int?),
      'destroy' => live.destroy(id),
      _ => throw ArgumentError.value(kind, 'kind'),
    };
  }

  /// A rename reaches the scripts' handle too (`GameObject.name`).
  List<String> _rename(String id, String name) {
    final refused = live.rename(id, name);
    if (refused.isEmpty) world.byEntity(live.entity(id).handle)?.name = name;
    return refused;
  }

  @override
  ({String id, List<String> refused}) spawn({
    required String name,
    String? parent,
    int? index,
    List<Map<String, Object?>> components = const [],
    String? authoringId,
  }) => live.spawn(name: name, parent: parent, index: index, components: components, authoringId: authoringId);

  @override
  List<String> setBehaviourField(Object instance, String field, Object? value) {
    final b = instance as fynkel.Behaviour;
    final f = b.behaviourType?.fields.where((f) => f.name == field).firstOrNull;
    if (f == null) return ['${b.behaviourType?.id ?? b.runtimeType} has no field $field'];
    try {
      if (f.isReference) {
        final target = _resolveReference(value);
        if (value != null && target == null) return ['$field: that object is not in the running game'];
        f.set(b, target);
        return const [];
      }
      if (f.holdsData) {
        // The shared instance(s) of the named data asset(s), as a load sets them.
        final data = world.resolveData(value, f.dataType!, where: '${b.behaviourType!.id}.$field');
        f.set(b, data ?? (value is List ? const <Object?>[] : null));
        return const [];
      }
      f.set(b, value);
    } on Object catch (e) {
      return ['$field: $e'];
    }
    return const [];
  }

  @override
  List<String> addBehaviour(EntityRef handle, String type, {int? version, Map<String, Object?> data = const {}}) {
    // An entity no script has touched yet gets its handle now.
    final go = world.byEntity(handle) ?? world.attach(handle);
    try {
      go.addBehaviourFromData(type, fields: data, version: version);
    } on Object catch (e) {
      return ['$type: $e'];
    }
    return const [];
  }

  @override
  List<String> removeBehaviour(Object instance) {
    final b = instance as fynkel.Behaviour;
    b.destroy();
    _removed.add(b);
    return const [];
  }
}

final class PlayDocument extends EditorDocument {
  PlayDocument({
    required this.backend,
    this.scene,
    this.written,
    Duration interval = const Duration(milliseconds: 100),
  }) {
    _read(backend.entities());
    _poll = Timer.periodic(interval, (_) => refresh());
  }

  final LiveBackend backend;

  /// The scene being played, when it is open: its component ids are reused
  /// for the entities it has.
  final EditorDocument? scene;

  /// The ids to re-read whole on every refresh (the Inspector's selection).
  Iterable<String> Function()? watched;

  /// Called after every write to the world (a paused Game view draws it
  /// again: `FynkelPlayControl.redraw`).
  final void Function()? written;

  /// The objects made here during Play (created or duplicated), and the
  /// ones deleted here — what Apply to Scene may keep (the game's own
  /// spawns and destroys are not in these).
  final Set<String> createdHere = {};
  final Set<String> deletedHere = {};

  /// The components removed here during Play, by entity (their ids as
  /// the scene has them).
  final Map<String, Set<String>> removedHere = {};

  /// Whether [id] is in the running world now.
  bool has(String id) => _byId.containsKey(id);

  /// Whether [id] is a root the game keeps across scene loads
  /// (`dontDestroyOnLoad`, ADR-0037): the Hierarchy lists these last,
  /// under a DontDestroyOnLoad header.
  bool isPersistent(String id) => _byId[id]?.persistent ?? false;

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

  /// What each component id of that read is in the world, by entity id.
  final Map<String, Map<String, _ComponentRef>> _refs = {};
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
  String? get readOnly => null;
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

  /// Applies [commands] to the running world, in order, each at once
  /// (L3). The first refusal stops the rest; what went before it stays —
  /// the world has no undo — and is shown.
  @override
  Applied apply(List<Map<String, Object?>> commands, {String? baseRevision}) {
    if (_closed) return _rejection('The game stopped.');
    final touched = <String>{};
    final mutations = <Map<String, Object?>>[];
    String? refused;
    try {
      for (final c in commands) {
        final problems = _applyOne(c, touched, mutations);
        if (problems.isNotEmpty) {
          refused = problems.join('; ');
          break;
        }
      }
    } on AuthoringException catch (e) {
      refused = e.diagnostics.isEmpty ? '$e' : e.diagnostics.map((d) => d.message).join('; ');
    } on FormatException catch (e) {
      refused = e.message;
    } on RangeError catch (e) {
      refused = '$e';
    }
    if (touched.isNotEmpty) _afterWrite(touched);
    if (refused != null) return _rejection(refused);
    return Applied.fromJson({'ok': true, 'revision': revision, 'mutations': mutations});
  }

  Applied _rejection(String message) => Applied.fromJson({
    'ok': false,
    'revision': revision,
    'diagnostics': [
      {'code': 'FYN-E0007', 'severity': 'error', 'message': message},
    ],
  });

  /// One command on the world; answers its refusals. [touched] gets the
  /// entities it wrote (before the write: a refused one may have half
  /// happened); [mutations] the objects a create or a duplicate made, as a
  /// document's transaction reports them.
  List<String> _applyOne(Map<String, Object?> c, Set<String> touched, List<Map<String, Object?>> mutations) {
    final command = c['command'];
    String entityOf(String key) {
      final id = c[key];
      if (id is! String || !_byId.containsKey(id)) throw _gone(id);
      return id;
    }

    switch (command) {
      case 'setField' || 'unsetField' || 'assignAsset':
        final id = entityOf('entity');
        final ref = _component(id, c['component']);
        final field = c['field'] as String;
        final value = switch (command) {
          'assignAsset' => c['asset'],
          'unsetField' => ref.defaultOf(field),
          _ => c['value'],
        };
        final data = _written(ref.data, field, value, unset: command == 'unsetField' && ref.instance == null);
        touched.add(id);
        final instance = ref.instance;
        if (instance == null) return backend.write('set', {'id': id, 'type': ref.type, 'data': data});
        final name = _segments(field).first;
        return backend.setBehaviourField(instance, name, data[name]);
      case 'addComponent':
        final id = entityOf('entity');
        final spec = (c['component'] as Map).cast<String, Object?>();
        final type = spec['type'] as String;
        final data = (spec['data'] as Map?)?.cast<String, Object?>() ?? const {};
        touched.add(id);
        if (_isNative(type)) return backend.write('set', {'id': id, 'type': type, 'data': data});
        return backend.addBehaviour(_byId[id]!.handle, type, version: spec['version'] as int?, data: data);
      case 'removeComponent':
        final id = entityOf('entity');
        final ref = _component(id, c['component']);
        touched.add(id);
        final instance = ref.instance;
        final refused = instance == null
            ? backend.write('remove', {'id': id, 'type': ref.type})
            : backend.removeBehaviour(instance);
        if (refused.isEmpty) (removedHere[id] ??= {}).add(c['component'] as String);
        return refused;
      case 'renameObject':
        final id = entityOf('id');
        touched.add(id);
        return backend.write('rename', {'id': id, 'name': c['name'] as String? ?? ''});
      case 'setObjectActive':
        final id = entityOf('id');
        touched.add(id);
        return backend.write('setActive', {'id': id, 'active': c['active'] == true});
      case 'reparentObject':
        final id = entityOf('id');
        final parent = c['parent'] as String?;
        if (parent != null && !_byId.containsKey(parent)) throw _gone(parent);
        touched.addAll([id, ?_byId[id]!.parent, ?parent]);
        return backend.write('parent', {'id': id, 'parent': parent, 'index': ?c['index']});
      case 'deleteObject':
        final id = entityOf('id');
        touched.addAll([id, ?_byId[id]!.parent]);
        final refused = backend.write('destroy', {'id': id});
        if (refused.isEmpty && !id.startsWith('runtime:')) deletedHere.add(id);
        return refused;
      case 'createObject':
        final parent = c['parent'] as String?;
        if (parent != null && !_byId.containsKey(parent)) throw _gone(parent);
        final r = _spawn(
          name: c['name'] as String? ?? '',
          parent: parent,
          index: c['index'] as int?,
          id: c['id'] as String? ?? newUuid(),
          components: [for (final x in c['components'] as List? ?? const []) (x as Map).cast<String, Object?>()],
        );
        touched.addAll([r.id, ?parent]);
        mutations.add({
          'command': 'createObject',
          'ok': true,
          'affectedIds': [r.id],
        });
        return r.refused;
      case 'duplicateObject':
        // The subtree copied with fresh ids, the copy right after the
        // original (the document's rule), scripts included.
        final id = entityOf('id');
        final row = _byId[id]!;
        final siblings = row.parent == null ? _roots : (_children[row.parent] ?? const <String>[]);
        final made = <String>[];
        final refused = <String>[];
        void copy(String source, String? parent, {String? newId, String? name, int? index}) {
          _full.remove(source); // its values now, not as last shown
          final doc = _document(source);
          if (doc == null) return;
          final r = _spawn(
            name: name ?? doc['name'] as String? ?? '',
            parent: parent,
            index: index,
            id: newId ?? newUuid(),
            components: [
              for (final v in (doc['components'] as Map? ?? const {}).values) (v as Map).cast<String, Object?>(),
            ],
          );
          made.add(r.id);
          refused.addAll(r.refused);
          for (final child in List.of(_children[source] ?? const <String>[])) {
            copy(child, r.id);
          }
        }

        copy(
          id,
          row.parent,
          newId: c['newId'] as String?,
          name: c['name'] as String?,
          index: c['index'] as int? ?? siblings.indexOf(id) + 1,
        );
        touched.addAll([...made, ?row.parent]);
        mutations.add({'command': 'duplicateObject', 'ok': true, 'affectedIds': made});
        return refused;
      default:
        return ['$command is not there while the game runs'];
    }
  }

  /// Makes one object: its native components with it, its scripts added
  /// right after.
  ({String id, List<String> refused}) _spawn({
    required String name,
    required String? parent,
    required int? index,
    required String id,
    required List<Map<String, Object?>> components,
  }) {
    final r = backend.spawn(
      name: name,
      parent: parent,
      index: index,
      authoringId: id,
      components: [
        for (final c in components)
          if (_isNative(c['type'] as String? ?? ''))
            {'type': c['type'], 'data': c['data'] ?? const <String, Object?>{}},
      ],
    );
    createdHere.add(r.id);
    final refused = [...r.refused];
    final scripts = components.where((c) => !_isNative(c['type'] as String? ?? '')).toList();
    if (scripts.isNotEmpty) {
      final handle = backend.entity(r.id).handle;
      for (final s in scripts) {
        refused.addAll(
          backend.addBehaviour(
            handle,
            s['type'] as String,
            version: s['version'] as int?,
            data: (s['data'] as Map?)?.cast<String, Object?>() ?? const {},
          ),
        );
      }
    }
    return (id: r.id, refused: refused);
  }

  /// Whether [type] is one of the world's own components (the rest are
  /// scripts).
  bool _isNative(String type) => type.startsWith('fynkel.') || (backend is WorldLiveBackend && (backend as WorldLiveBackend).world.registeredComponentTypes.any((t)=>t.id==type));

  AuthoringException _gone(Object? id) => AuthoringException('play', AuthoringStatus.invalidArgument, [
    Diagnostic(code: 'FYN-E0007', severity: Severity.error, message: '$id is not in the running game'),
  ]);

  /// [id]'s component [component] as last read.
  _ComponentRef _component(String id, Object? component) {
    _document(id);
    final ref = _refs[id]?[component];
    if (ref == null) {
      throw AuthoringException('play', AuthoringStatus.invalidArgument, [
        Diagnostic(code: 'FYN-E0007', severity: Severity.error, message: '$component is not on $id'),
      ]);
    }
    return ref;
  }

  /// The world after a write: its table read again, the written entities
  /// re-read, one `applied` edit for the tab and the Inspector.
  void _afterWrite(Set<String> touched) {
    final List<String> changed;
    try {
      changed = _read(backend.entities());
    } on AuthoringException {
      return;
    }
    final affected = {...touched, ...changed};
    for (final id in affected) {
      _full.remove(id);
      _fullJson.remove(id);
    }
    _revision++;
    _lastEdit = DocumentEdit(DocumentChangeKind.applied, affectedIds: affected.toList());
    notifyListeners();
    written?.call();
  }

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
    final preview = backend.openPreview() ?? (throw _refused('openPreview'));
    if (preview is LivePreview) preview.written = written;
    return _preview = preview;
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
        r.id: jsonEncode([r.name, r.parent, children[r.id] ?? const [], r.types, r.layer, r.activeSelf, r.persistent]),
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
        _refs.remove(id);
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
    final refs = <String, _ComponentRef>{};
    for (final c in state.components) {
      final cid = ids.forType(c.type);
      components[cid] = {'type': c.type, 'version': state.componentVersions[c.type] ?? 1, 'data': c.data};
      refs[cid] = _ComponentRef(c.type, c.data);
    }
    for (final b in backend.behaviours(row.handle)) {
      final cid = b.authoredComponentId ?? ids.forType(b.type);
      components[cid] = {'type': b.type, 'version': b.version, 'data': b.data};
      refs[cid] = _ComponentRef(b.type, b.data, instance: b.instance);
    }
    _refs[id] = refs;
    final doc = <String, Object?>{
      'name': state.name,
      if (!state.activeSelf) 'active': false,
      'parent': ?row.parent,
      'children': List<String>.of(_children[id] ?? const []),
      'components': components,
    };
    _full[id] = doc;
    _fullJson[id] = jsonEncode(doc);
    return doc;
  }
}

/// A component of a running entity as last read: a native one (no
/// [instance]) or a script.
final class _ComponentRef {
  const _ComponentRef(this.type, this.data, {this.instance});

  final String type;
  final Map<String, Object?> data;
  final Object? instance;

  /// A script field's default (what `unsetField` writes back).
  Object? defaultOf(String field) {
    final b = instance;
    if (b is! fynkel.Behaviour) return null;
    final name = _segments(field).first;
    return b.behaviourType?.fields.where((f) => f.name == name).firstOrNull?.defaultValue;
  }
}

/// A field name or a JSON pointer (`/position/1`) as its segments.
List<String> _segments(String field) => field.startsWith('/')
    ? [for (final s in field.substring(1).split('/')) s.replaceAll('~1', '/').replaceAll('~0', '~')]
    : [field];

/// A copy of [data] with [value] at [field] (removed with [unset]).
Map<String, Object?> _written(Map<String, Object?> data, String field, Object? value, {bool unset = false}) {
  final copy = (jsonDecode(jsonEncode(data)) as Map).cast<String, Object?>();
  final path = _segments(field);
  Object? at = copy;
  for (final s in path.take(path.length - 1)) {
    at = switch (at) {
      Map() => at[s],
      List() => at[int.parse(s)],
      _ => throw FormatException('$field: no $s in ${jsonEncode(data)}'),
    };
  }
  final last = path.last;
  switch (at) {
    case Map():
      if (unset) {
        at.remove(last);
      } else {
        at[last] = value;
      }
    case List():
      at[int.parse(last)] = value;
    default:
      throw FormatException('$field: no $last in ${jsonEncode(data)}');
  }
  return copy;
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
