// An in-memory document for the tabs and Hierarchy tests: a small
// interpreter for the commands the Hierarchy uses (createObject,
// deleteObject, renameObject, reparentObject, duplicateObject, setField,
// addComponent) with snapshots for undo/redo, content-hash revisions, the
// FYN-E0001 refusal on a stale base, and — when `path` points at a real
// file — save/reload through the disk so the conflict path can be driven.
import 'dart:convert';
import 'dart:io';

import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/editor_document.dart';

final class FakeDocument extends EditorDocument {
  FakeDocument({
    required this.path,
    required this.id,
    this.kind = 'scene',
    String name = 'Fake Scene',
    Map<String, Map<String, Object?>>? entities,
    List<String>? roots,
    this.readOnly,
    this.relativePath,
    this.onDisk = false,
    // ignore: prefer_initializing_formals
  }) : _name = name {
    if (onDisk && File(path).existsSync()) {
      _readFile();
    } else {
      _entities = {for (final e in (entities ?? {}).entries) e.key: _deepCopy(e.value)};
      _roots =
          roots ??
          [
            for (final e in _entities.entries)
              if (e.value['parent'] == null) e.key,
          ];
      if (onDisk) File(path).writeAsStringSync(_text());
    }
    _savedRevision = revision;
  }

  /// A scene of [count] entities in chains of [depth] under [rootsCount] roots.
  factory FakeDocument.large({required String path, int count = 10000, int depth = 4}) {
    final entities = <String, Map<String, Object?>>{};
    final roots = <String>[];
    var n = 0;
    var r = 0;
    while (n < count) {
      final rootId = 'e${n++}';
      roots.add(rootId);
      entities[rootId] = _entity('Root $r', null);
      var parent = rootId;
      for (var d = 1; d < depth && n < count; d++) {
        final id = 'e${n++}';
        entities[id] = _entity('Node $id', parent);
        (entities[parent]!['children'] as List).add(id);
        parent = id;
      }
      r++;
    }
    return FakeDocument(path: path, id: 'large', name: 'Large', entities: entities, roots: roots);
  }

  static Map<String, Object?> _entity(
    String name,
    String? parent, {
    List<Map<String, Object?>> components = const [],
  }) => {
    'name': name,
    'parent': parent,
    'children': <String>[],
    'components': {
      't': {
        'type': 'fynkel.Transform',
        'version': 1,
        'data': {
          'position': [0, 0, 0],
          'rotation': [0, 0, 0, 1],
          'scale': [1, 1, 1],
        },
      },
      for (final (i, c) in components.indexed) 'c$i': c,
    },
  };

  /// A handy entity map: Transform at [position] plus [components].
  static Map<String, Object?> entity(
    String name, {
    String? parent,
    List<String> children = const [],
    List<num> position = const [0, 0, 0],
    List<num> rotation = const [0, 0, 0, 1],
    List<num> scale = const [1, 1, 1],
    Map<String, Map<String, Object?>> components = const {},
    Map<String, Object?>? prefab,
  }) => {
    'name': name,
    'parent': parent,
    'children': List<String>.of(children),
    'components': {
      't': {
        'type': 'fynkel.Transform',
        'version': 1,
        'data': {'position': position, 'rotation': rotation, 'scale': scale},
      },
      ...components,
    },
    'prefab': ?prefab,
  };

  /// Like the session's `state.path`: a save-as moves the document to the new file.
  @override
  String path;
  @override
  String? relativePath;
  @override
  final String kind;
  @override
  final String id;
  String _name;
  @override
  String get name => _name;
  @override
  final String? readOnly;

  /// Save writes and reload reads `path` as a file.
  final bool onDisk;

  late Map<String, Map<String, Object?>> _entities;
  late List<String> _roots;
  late String _savedRevision;
  final _undo = <_Snapshot>[];
  final _redo = <_Snapshot>[];
  int _txn = 0;
  int _fresh = 0;
  bool _closed = false;
  DocumentEdit? _lastEdit;

  /// What `validate()` answers; tests set it.
  List<Diagnostic> validation = const [];

  /// Calls, for the tests: `apply`, `undo`, `redo`, `save`, `reload`.
  final calls = <String>[];

  Map<String, Map<String, Object?>> get entitiesView => _entities;

  @override
  String get revision => _hash(_text());
  @override
  bool get dirty => revision != _savedRevision;
  @override
  bool get canUndo => _undo.isNotEmpty;
  @override
  bool get canRedo => _redo.isNotEmpty;
  @override
  int get history => _undo.length;
  @override
  bool get isClosed => _closed;
  @override
  DocumentEdit? get lastEdit => _lastEdit;

  // ── Text / hash ────────────────────────────────────────────────────────

  String _text() => const JsonEncoder.withIndent('  ').convert(root());

  static String _hash(String s) {
    // FNV-1a over the UTF-8 bytes, as 16 hex digits — stable and cheap.
    var h = 0xcbf29ce484222325;
    for (final b in utf8.encode(s)) {
      h ^= b;
      h = (h * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF;
    }
    return h.toRadixString(16).padLeft(16, '0');
  }

  void _readFile() {
    final j = jsonDecode(File(path).readAsStringSync()) as Map<String, Object?>;
    _name = j['name'] as String? ?? _name;
    _entities = {
      for (final e in (j['entities'] as Map).entries) e.key as String: _deepCopy(e.value as Map<String, Object?>),
    };
    _roots = [for (final r in j['roots'] as List? ?? const []) r as String];
  }

  static Map<String, Object?> _deepCopy(Map<String, Object?> m) => jsonDecode(jsonEncode(m)) as Map<String, Object?>;

  // ── Reads ──────────────────────────────────────────────────────────────

  @override
  Map<String, Object?> root() => {
    'formatVersion': 3,
    'kind': kind,
    'id': id,
    'name': _name,
    'entities': {for (final e in _entities.entries) e.key: _deepCopy(e.value)},
    'roots': List<String>.of(_roots),
  };

  @override
  EntityPatch entities(Iterable<String> ids) => EntityPatch(
    entities: {for (final id in ids) id: _entities[id] == null ? null : _deepCopy(_entities[id]!)},
    roots: List<String>.of(_roots),
    state: _state(),
  );

  @override
  List<String> roots() => List<String>.of(_roots);

  DocumentState _state() => DocumentState.fromJson({
    'handle': 1,
    'path': path,
    'relativePath': relativePath,
    'kind': kind,
    'id': id,
    'name': _name,
    'formatVersion': 3,
    'revision': revision,
    'dirty': dirty,
    'canUndo': canUndo,
    'canRedo': canRedo,
    'history': history,
    'readOnly': readOnly,
  });

  @override
  List<Diagnostic> validate() => validation;

  @override
  Map<String, Object?> inspect() => {
    'kind': kind,
    'id': id,
    'name': _name,
    'entities': [
      for (final e in _entities.entries)
        {'id': e.key, 'name': e.value['name'], 'parent': e.value['parent'], 'children': e.value['children']},
    ],
    'roots': roots(),
  };

  // ── Commands ───────────────────────────────────────────────────────────

  @override
  Applied apply(List<Map<String, Object?>> commands, {String? baseRevision}) {
    calls.add('apply');
    final base = revision;
    Map<String, Object?> refused(List<Map<String, Object?>> diags) => {
      'ok': false,
      'transactionId': 't${++_txn}',
      'baseRevision': base,
      'revision': base,
      'mutations': const [],
      'diagnostics': diags,
      'state': _state().json,
    };
    if (readOnly != null) {
      throw AuthoringException('apply', AuthoringStatus.commandsRejected, [
        Diagnostic(code: 'FYN-E0007', severity: Severity.error, message: 'read-only: $readOnly'),
      ]);
    }
    if (baseRevision != null && baseRevision != base) {
      return Applied.fromJson(
        refused([
          {
            'code': 'FYN-E0001',
            'severity': 'error',
            'message': 'stale base revision $baseRevision; the document is at $base',
          },
        ]),
      );
    }
    final snapshot = _Snapshot(_deepCopyAll(_entities), List.of(_roots), _name);
    final mutations = <Map<String, Object?>>[];
    var ok = true;
    for (final (i, c) in commands.indexed) {
      final r = _run(c);
      mutations.add({
        'commandId': 'c$i',
        'command': c['command'],
        'affectedIds': r.affected,
        'ok': r.error == null,
        'diagnostics': r.error == null
            ? const []
            : [
                {'code': 'FYN-E0002', 'severity': 'error', 'message': r.error},
              ],
      });
      if (r.error != null) {
        ok = false;
        break;
      }
    }
    if (!ok) {
      _entities = snapshot.entities;
      _roots = snapshot.roots;
      _name = snapshot.name;
      return Applied.fromJson({
        'ok': false,
        'transactionId': 't${++_txn}',
        'baseRevision': base,
        'revision': base,
        'mutations': mutations,
        'diagnostics': const [],
        'state': _state().json,
      });
    }
    _undo.add(snapshot..affected = [for (final m in mutations) ...m['affectedIds'] as List<String>]);
    _redo.clear();
    final a = Applied.fromJson({
      'ok': true,
      'transactionId': 't${++_txn}',
      'baseRevision': base,
      'revision': revision,
      'mutations': mutations,
      'diagnostics': const [],
      'state': _state().json,
    });
    _lastEdit = DocumentEdit(DocumentChangeKind.applied, affectedIds: a.affectedIds);
    notifyListeners();
    return a;
  }

  static Map<String, Map<String, Object?>> _deepCopyAll(Map<String, Map<String, Object?>> m) => {
    for (final e in m.entries) e.key: _deepCopy(e.value),
  };

  _Result _run(Map<String, Object?> c) {
    switch (c['command']) {
      case 'createObject':
        final parent = c['parent'] as String?;
        if (parent != null && !_entities.containsKey(parent)) return _Result.error('no entity $parent');
        final id = c['id'] as String? ?? 'new${++_fresh}';
        final comps = <String, Object?>{};
        for (final (i, comp) in (c['components'] as List? ?? const []).indexed) {
          final m = comp as Map<String, Object?>;
          comps[m['id'] as String? ?? 'k$i'] = {
            'type': m['type'],
            'version': m['version'] ?? 1,
            'data': m['data'] ?? {},
          };
        }
        _entities[id] = {'name': c['name'], 'parent': parent, 'children': <String>[], 'components': comps};
        _insert(parent, id, c['index'] as int?);
        return _Result([id, ?parent]);
      case 'deleteObject':
        final id = c['id'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final removed = <String>[];
        _delete(id, removed);
        final parent = e['parent'] as String?;
        _detach(parent, id);
        return _Result([...removed, ?parent]);
      case 'renameObject':
        final id = c['id'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        e['name'] = c['name'];
        return _Result([id]);
      case 'reparentObject':
        final id = c['id'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final parent = c['parent'] as String?;
        if (parent != null && !_entities.containsKey(parent)) return _Result.error('no entity $parent');
        if (parent != null && (parent == id || _isDescendant(parent, id))) return _Result.error('cycle');
        final old = e['parent'] as String?;
        _detach(old, id);
        e['parent'] = parent;
        _insert(parent, id, c['index'] as int?);
        return _Result([id, ?old, ?parent]);
      case 'duplicateObject':
        final id = c['id'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final ids = <String>[];
        final copy = _copy(id, e['parent'] as String?, c['newId'] as String?, ids);
        if (c['name'] != null) _entities[copy]!['name'] = c['name'];
        final parent = e['parent'] as String?;
        final siblings = parent == null ? _roots : _entities[parent]!['children'] as List;
        final index = c['index'] as int? ?? siblings.indexOf(id) + 1;
        _insert(parent, copy, index);
        return _Result([...ids, ?parent]);
      case 'setField':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final comp = (e['components'] as Map)[c['component']] as Map?;
        if (comp == null) return _Result.error('no component ${c['component']} on $id');
        (comp['data'] as Map)[c['field']] = c['value'];
        return _Result([id, c['component'] as String]);
      case 'addComponent':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final m = c['component'] as Map<String, Object?>;
        final cid = m['id'] as String? ?? 'k${++_fresh}';
        (e['components'] as Map)[cid] = {'type': m['type'], 'version': m['version'] ?? 1, 'data': m['data'] ?? {}};
        return _Result([id, cid]);
      default:
        return _Result.error('unknown command ${c['command']}');
    }
  }

  bool _isDescendant(String id, String ancestor) {
    var p = _entities[id]?['parent'] as String?;
    while (p != null) {
      if (p == ancestor) return true;
      p = _entities[p]?['parent'] as String?;
    }
    return false;
  }

  void _insert(String? parent, String id, int? index) {
    if (parent == null && kind == 'scene') {
      // Like the crate: a scene has no `roots` list — its roots follow entity key order.
      _roots = [
        for (final e in _entities.entries)
          if (e.value['parent'] == null) e.key,
      ];
      return;
    }
    final list = parent == null ? _roots : _entities[parent]!['children'] as List;
    if (index == null || index >= list.length) {
      list.add(id);
    } else {
      list.insert(index < 0 ? 0 : index, id);
    }
  }

  void _detach(String? parent, String id) {
    final list = parent == null ? _roots : _entities[parent]?['children'] as List?;
    list?.remove(id);
  }

  void _delete(String id, List<String> removed) {
    final e = _entities.remove(id);
    if (e == null) return;
    removed.add(id);
    for (final ch in List.of(e['children'] as List)) {
      _delete(ch as String, removed);
    }
  }

  String _copy(String id, String? parent, String? newId, List<String> ids) {
    final src = _entities[id]!;
    final copyId = newId ?? '$id-copy${++_fresh}';
    ids.add(copyId);
    final e = _deepCopy(src);
    e['parent'] = parent;
    e['children'] = <String>[];
    _entities[copyId] = e;
    for (final ch in src['children'] as List) {
      final c = _copy(ch as String, copyId, null, ids);
      (e['children'] as List).add(c);
    }
    return copyId;
  }

  // ── History ────────────────────────────────────────────────────────────

  @override
  Replayed undo() => _replay(_undo, _redo, DocumentChangeKind.undone, 'undo');
  @override
  Replayed redo() => _replay(_redo, _undo, DocumentChangeKind.redone, 'redo');

  Replayed _replay(List<_Snapshot> from, List<_Snapshot> to, DocumentChangeKind kind, String op) {
    calls.add(op);
    if (from.isEmpty) {
      throw AuthoringException(op, AuthoringStatus.commandsRejected, [
        Diagnostic(code: 'FYN-E0009', severity: Severity.error, message: 'nothing to $op'),
      ]);
    }
    final base = revision;
    final s = from.removeLast();
    to.add(_Snapshot(_deepCopyAll(_entities), List.of(_roots), _name)..affected = s.affected);
    _entities = s.entities;
    _roots = s.roots;
    _name = s.name;
    final r = Replayed.fromJson({
      'transactionId': 't${++_txn}',
      'baseRevision': base,
      'revision': revision,
      'commands': const [],
      'affectedIds': s.affected,
      'state': _state().json,
    });
    _lastEdit = DocumentEdit(kind, affectedIds: s.affected);
    notifyListeners();
    return r;
  }

  // ── Disk ───────────────────────────────────────────────────────────────

  @override
  String save({String? path}) {
    calls.add('save');
    final target = path ?? this.path;
    if (onDisk || path != null) File(target).writeAsStringSync(_text());
    if (path != null && path != this.path) {
      this.path = path;
      relativePath = null; // the session recomputes it; the fake does not know the assets root
    }
    _savedRevision = revision;
    _lastEdit = const DocumentEdit(DocumentChangeKind.saved, affectedIds: []);
    notifyListeners();
    return target;
  }

  @override
  void reload() {
    calls.add('reload');
    if (onDisk) {
      if (!File(path).existsSync()) {
        throw AuthoringException('reload', AuthoringStatus.loadFailed, [
          Diagnostic(code: 'FYN-D0900', severity: Severity.error, message: 'no file at $path'),
        ]);
      }
      _readFile();
    } else if (_undo.isNotEmpty) {
      final first = _undo.first;
      _entities = first.entities;
      _roots = first.roots;
      _name = first.name;
    }
    _undo.clear();
    _redo.clear();
    _savedRevision = revision;
    _lastEdit = const DocumentEdit(DocumentChangeKind.reloaded);
    notifyListeners();
  }

  @override
  void close() {
    _closed = true;
  }
}

final class _Snapshot {
  _Snapshot(this.entities, this.roots, this.name);

  final Map<String, Map<String, Object?>> entities;
  final List<String> roots;
  final String name;
  List<String> affected = const [];
}

final class _Result {
  _Result(this.affected) : error = null;
  _Result.error(this.error) : affected = const [];

  final List<String> affected;
  final String? error;
}
