// An in-memory document for the tabs and Hierarchy tests: a small
// interpreter for the commands the Hierarchy uses (createObject,
// deleteObject, renameObject, setObjectActive, reparentObject,
// duplicateObject, setField, addComponent) and the Inspector uses (setField by JSON pointer,
// unsetField, assignAsset, removeComponent, moveComponent, setPrefabOverride,
// revertPrefabOverride, unpackPrefab; `reject` stands in for the crate's
// validation) with snapshots for undo/redo, content-hash revisions, the
// FYN-E0001 refusal on a stale base, and — when `path` points at a real
// file — save/reload through the disk so the conflict path can be driven.
// Prefab mode: `instance(anchor)` answers the `instance` query through
// [FakeDocument.expander] (the fake project's `fakeExpand`, which walks
// the prefab document's entities and applies the override ops the way the
// crate does, under deterministic derived ids `<instance>:<local>`).
import 'dart:convert';
import 'dart:io';

import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_editor/src/documents/editor_document.dart';

import 'fake_preview.dart';

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
    this.sub,
    this.sourceId,
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

  static String _overrideTarget(Map o) => switch (o['op']) {
    'setField' || 'unsetField' => 'field:${o['entity']}:${o['component']}:${o['field']}',
    'addComponent' || 'removeComponent' => 'component:${o['entity']}:${o['component']}',
    'addObject' || 'removeObject' => 'object:${o['entity']}',
    _ => 'other:${o['op']}:${o['entity']}:${o['component']}:${o['field']}',
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

  /// `prefab` for a fake imported hierarchy (`<id>#prefab`), with the
  /// source asset's id in [sourceId] (the document's own id is the
  /// sub-asset's, as the session derives it).
  @override
  final String? sub;
  final String? sourceId;

  /// Answers the `instance` query for an anchor of this document; the fake
  /// project sets it (`fakeExpand` over its prefab documents). Without one
  /// every instance is unexpanded (FYN-D0401).
  PrefabExpansion Function(FakeDocument doc, String anchor)? expander;

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

  /// The crate's validation, faked: returns a message to refuse a command
  /// (the whole transaction rolls back with `FYN-E0002`), null to accept.
  String? Function(Map<String, Object?> command)? reject;

  /// The crate's validation of the whole document after the commands ran:
  /// the errors it would be left with (JSON diagnostics), or empty. Any →
  /// rollback and one `FYN-E0005` carrying them as `related`, as the crate
  /// does. Tests set it.
  List<Map<String, Object?>> Function(List<Map<String, Object?>> commands)? validateAfter;

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
  PrefabExpansion instance(String anchor) {
    calls.add('instance');
    final e = _entities[anchor];
    if (e == null) {
      throw AuthoringException('query', AuthoringStatus.invalidArgument, [
        Diagnostic(code: 'FYN-E0002', severity: Severity.error, message: 'unknown entity $anchor'),
      ]);
    }
    if (e['prefab'] is! Map) {
      throw AuthoringException('query', AuthoringStatus.invalidArgument, [
        Diagnostic(code: 'FYN-E0008', severity: Severity.error, message: 'instance: $anchor is not a prefab instance'),
      ]);
    }
    final x = expander;
    if (x != null) return x(this, anchor);
    return fakeExpand(this, anchor, prefabs: (_, _) => null);
  }

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
    final left = ok ? (validateAfter?.call(commands) ?? const []) : const <Map<String, Object?>>[];
    if (!ok || left.isNotEmpty) {
      _entities = snapshot.entities;
      _roots = snapshot.roots;
      _name = snapshot.name;
      return Applied.fromJson({
        'ok': false,
        'transactionId': 't${++_txn}',
        'baseRevision': base,
        'revision': base,
        'mutations': mutations,
        'diagnostics': left.isEmpty
            ? const []
            : [
                {
                  'code': 'FYN-E0005',
                  'severity': 'error',
                  'message':
                      'transaction rejected: it would leave the document with ${left.length} error(s); nothing was applied',
                  'related': left,
                },
              ],
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
      case 'instantiatePrefab':
        final parent = c['parent'] as String?;
        if (parent != null && !_entities.containsKey(parent)) return _Result.error('no entity $parent');
        final prefab = c['prefab'];
        if (prefab is! Map || prefab['asset'] is! String) return _Result.error('prefab names no asset');
        final id = c['id'] as String? ?? 'inst${++_fresh}';
        // The anchor's own components: a Transform by default, replaced by
        // the spec's (the Scene view's drop puts the instance where it hit).
        final components = <String, Object?>{
          't': {
            'type': 'fynkel.Transform',
            'version': 1,
            'data': {
              'position': [0, 0, 0],
              'rotation': [0, 0, 0, 1],
              'scale': [1, 1, 1],
            },
          },
        };
        for (final (i, spec) in (c['components'] as List? ?? const []).indexed) {
          if (spec is! Map) continue;
          final type = spec['type'];
          final key = type == 'fynkel.Transform' ? 't' : (spec['id'] as String? ?? 'c$i');
          components[key] = {
            'type': type,
            'version': spec['version'] ?? 1,
            'data': _deepCopy(spec['data'] as Map<String, Object?>? ?? {}),
          };
        }
        _entities[id] = {
          'name': c['name'] ?? 'Instance',
          'parent': parent,
          'children': <String>[],
          'components': components,
          'prefab': {
            'asset': {for (final e in prefab.entries) '${e.key}': e.value},
            'overrides': <Object?>[],
          },
        };
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
      case 'setObjectActive':
        final id = c['id'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        if (c['active'] == false) {
          e['active'] = false;
        } else {
          e.remove('active');
        }
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
      case 'setField' || 'assignAsset':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final comp = (e['components'] as Map)[c['component']] as Map?;
        if (comp == null) return _Result.error('no component ${c['component']} on $id');
        final refusal = reject?.call(c);
        if (refusal != null) return _Result.error(refusal);
        final value = c['command'] == 'assignAsset' ? c['asset'] : c['value'];
        final err = _setPointer(comp['data'] as Map, c['field'] as String, _deepCopyValue(value));
        if (err != null) return _Result.error(err);
        return _Result([id, c['component'] as String]);
      case 'unsetField':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final comp = (e['components'] as Map)[c['component']] as Map?;
        if (comp == null) return _Result.error('no component ${c['component']} on $id');
        final refusal = reject?.call(c);
        if (refusal != null) return _Result.error(refusal);
        (comp['data'] as Map).remove(c['field']);
        return _Result([id, c['component'] as String]);
      case 'addComponent':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final refusal = reject?.call(c);
        if (refusal != null) return _Result.error(refusal);
        final m = c['component'] as Map<String, Object?>;
        final cid = m['id'] as String? ?? 'k${++_fresh}';
        if ((e['components'] as Map).containsKey(cid)) return _Result.error('FYN-E0010 component $cid exists');
        (e['components'] as Map)[cid] = {
          'type': m['type'],
          'version': m['version'] ?? 1,
          'data': _deepCopyValue(m['data'] ?? <String, Object?>{}),
        };
        return _Result([id, cid]);
      case 'removeComponent':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final removed = (e['components'] as Map).remove(c['component']);
        if (removed == null) return _Result.error('no component ${c['component']} on $id');
        return _Result([id, c['component'] as String]);
      case 'moveComponent':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final map = e['components'] as Map;
        final moved = map.remove(c['component']);
        if (moved == null) return _Result.error('no component ${c['component']} on $id');
        final entries = map.entries.toList()
          ..insert((c['index'] as int).clamp(0, map.length), MapEntry(c['component'], moved));
        map
          ..clear()
          ..addEntries(entries);
        return _Result([id, c['component'] as String]);
      case 'setPrefabOverride':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final prefab = e['prefab'] as Map?;
        if (prefab == null) return _Result.error('$id is not a prefab instance');
        final list = (prefab['overrides'] as List?) ?? (prefab['overrides'] = <Object?>[]);
        final op = _deepCopyValue(c['override']) as Map;
        // The crate replaces the override with the same target: a field's
        // setField/unsetField, a component's add/remove, an object's add/remove.
        final i = list.indexWhere((o) => o is Map && _overrideTarget(o) == _overrideTarget(op));
        if (i >= 0) {
          list[i] = op;
        } else {
          list.add(op);
        }
        return _Result([id]);
      case 'revertPrefabOverride':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        final prefab = e['prefab'] as Map?;
        if (prefab == null) return _Result.error('$id is not a prefab instance');
        final list = prefab['overrides'] as List? ?? [];
        final index = c['index'] as int?;
        if (index == null) {
          list.clear();
        } else if (index < 0 || index >= list.length) {
          return _Result.error('no override $index');
        } else {
          list.removeAt(index);
        }
        return _Result([id]);
      case 'unpackPrefab':
        final id = c['entity'] as String;
        final e = _entities[id];
        if (e == null) return _Result.error('no entity $id');
        if (e['prefab'] is! Map) return _Result.error('FYN-E0008 $id is not a prefab instance');
        final x = instance(id);
        if (!x.expanded) {
          return _Result.error(
            'FYN-E0004 unpackPrefab: the instance at $id cannot be expanded '
            '(${x.diagnostics.map((d) => '${d.code}: ${d.message}').join('; ')})',
          );
        }
        e.remove('prefab');
        final roots = [
          for (final d in x.entities)
            if (d.parent == id) d.id,
        ];
        e['children'] = [...(e['children'] as List? ?? const []), ...roots];
        // The derived entities follow the anchor in the document, in expansion order.
        final next = <String, Map<String, Object?>>{};
        final affected = <String>[id];
        for (final entry in _entities.entries) {
          next[entry.key] = entry.value;
          if (entry.key != id) continue;
          for (final d in x.entities) {
            next[d.id] = {
              'name': d.name,
              'parent': d.parent,
              'children': List<String>.of(d.children),
              'components': {
                for (final comp in d.components.values)
                  comp.id: {'type': comp.type, 'version': comp.version, 'data': _deepCopyValue(comp.data)},
              },
            };
            affected.add(d.id);
            affected.addAll(d.components.keys);
          }
        }
        _entities = next;
        return _Result(affected);
      default:
        return _Result.error('unknown command ${c['command']}');
    }
  }

  /// `field` is a key of `data` or a JSON pointer into it (`/position/1`);
  /// a missing key is added, an array end appended to — like the crate.
  String? _setPointer(Map data, String field, Object? value) {
    if (!field.startsWith('/')) {
      data[field] = value;
      return null;
    }
    final parts = field.substring(1).split('/');
    Object? node = data;
    for (var i = 0; i < parts.length - 1; i++) {
      final key = parts[i];
      node = node is Map ? node[key] : (node is List ? node[int.parse(key)] : null);
      if (node == null) return 'setField: cannot set $field: no $key';
    }
    final last = parts.last;
    if (node is Map) {
      node[last] = value;
    } else if (node is List) {
      final i = int.tryParse(last);
      if (i == null || i > node.length) return 'setField: cannot set $field: $last is not an array end';
      if (i == node.length) {
        node.add(value);
      } else {
        node[i] = value;
      }
    } else {
      return 'setField: cannot set $field: not an object or array';
    }
    return null;
  }

  static Object? _deepCopyValue(Object? v) => switch (v) {
    Map() => <String, Object?>{for (final e in v.entries) e.key as String: _deepCopyValue(e.value)},
    List() => <Object?>[for (final e in v) _deepCopyValue(e)],
    _ => v,
  };

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
      // Like the crate: a scene has no `roots` list — its roots follow entity
      // key order, and an index moves the entity before the root now there.
      if (index != null) {
        final others = [
          for (final e in _entities.entries)
            if (e.value['parent'] == null && e.key != id) e.key,
        ];
        final entity = _entities.remove(id)!;
        final before = index < others.length ? others[index < 0 ? 0 : index] : null;
        final rebuilt = <String, Map<String, Object?>>{};
        for (final e in _entities.entries) {
          if (e.key == before) rebuilt[id] = entity;
          rebuilt[e.key] = e.value;
        }
        if (before == null) rebuilt[id] = entity;
        _entities
          ..clear()
          ..addAll(rebuilt);
      }
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

  // ── Preview ────────────────────────────────────────────────────────────

  FakePreviewBackend? _preview;

  /// Previews opened so far (a closed tab's preview is closed with it).
  int previewsOpened = 0;

  @override
  PreviewBackend openPreview() {
    if (kind == 'material') {
      throw AuthoringException('previewOpen', AuthoringStatus.invalidArgument, [
        Diagnostic(code: 'FYN-T0003', severity: Severity.error, message: 'preview: a material has no world'),
      ]);
    }
    final open = _preview;
    if (open != null && !open.isClosed) return open;
    previewsOpened++;
    return _preview = FakePreviewBackend(this);
  }

  @override
  PreviewBackend? get preview => _preview == null || _preview!.isClosed ? null : _preview;

  /// The fake backend itself, open or not (tests read its calls).
  FakePreviewBackend? get fakePreview => _preview;

  /// Like the session: a closed handle's unsaved edits are gone — the next
  /// reader (an instance's expansion) sees the saved document.
  @override
  void close() {
    _closed = true;
    _preview?.close();
    while (dirty && _undo.isNotEmpty) {
      final s = _undo.removeLast();
      _entities = s.entities;
      _roots = s.roots;
      _name = s.name;
    }
    _undo.clear();
    _redo.clear();
  }
}

/// The `instance` query, faked: expands the prefab document [prefabs]
/// answers for the anchor's `prefab.asset` (null → FYN-D0401, unexpanded)
/// and applies the overrides in order the way `fynkel_document::prefab`
/// does — a bad target is FYN-D0403 and leaves the instance unexpanded,
/// the anchor's data untouched. Derived ids are `<instance>:<local>`.
PrefabExpansion fakeExpand(
  FakeDocument doc,
  String anchor, {
  required FakeDocument? Function(String asset, String? sub) prefabs,
  int? Function(FakeDocument prefab)? handleOf,
}) {
  final e = doc.entitiesView[anchor]!;
  final prefab = e['prefab'] as Map;
  final ref = prefab['asset'];
  final asset = ref is Map ? ref['asset'] as String? ?? '' : ref?.toString() ?? '';
  final sub = ref is Map ? ref['sub'] as String? : null;
  final instance = prefab['instance'] as String? ?? anchor;
  final rawOverrides = [
    for (final o in prefab['overrides'] as List? ?? const [])
      if (o is Map) Map<String, Object?>.from(o),
  ];
  String derive(String local) => '$instance:$local';
  final pointerBase = '/entities/$anchor/prefab/overrides';
  final overrides = <Map<String, Object?>>[
    for (final (i, o) in rawOverrides.indexed) {...o, 'index': i, 'pointer': '$pointerBase/$i'},
  ];
  final diagnostics = <Map<String, Object?>>[];
  final source = prefabs(asset, sub);
  Map<String, Object?>? prefabBlock;
  if (source == null) {
    diagnostics.add({
      'code': 'FYN-D0401',
      'severity': 'error',
      'message': 'unknown prefab ${sub == null ? asset : '$asset#$sub'}',
      'id': anchor,
      'pointer': '/entities/$anchor/prefab/asset',
    });
  } else {
    prefabBlock = {
      'id': source.id,
      'name': source.name,
      'path': source.relativePath,
      'kind': source.sub != null ? 'source' : 'prefab',
      'open': source.sub != null ? '${source.id}#${source.sub}' : source.id,
      'handle': handleOf?.call(source),
      'readOnly': source.readOnly,
    };
  }
  // The prefab's entities, deep-copied, then the overrides applied.
  final entities = <String, Map<String, Object?>>{
    if (source != null)
      for (final en in source.entitiesView.entries) en.key: jsonDecode(jsonEncode(en.value)) as Map<String, Object?>,
  };
  var roots = source == null ? <String>[] : List<String>.of(source.roots());
  final overridden = <String, Set<String>>{}; // local component → fields
  final addedComponents = <String>{};
  final addedEntities = <String>{};
  void bad(Map<String, Object?> o, String why) {
    diagnostics.add({
      'code': 'FYN-D0403',
      'severity': 'error',
      'message': 'override ${o['index']}: $why',
      'id': anchor,
      'pointer': o['pointer'],
    });
  }

  if (source != null) {
    for (final o in overrides) {
      final local = o['entity'] as String? ?? '';
      final target = entities[local];
      switch (o['op']) {
        case 'setField' || 'unsetField':
          final comp = target == null ? null : ((target['components'] as Map)[o['component']] as Map?);
          if (comp == null) {
            bad(o, 'no component ${o['component']} on entity $local in the prefab');
            continue;
          }
          final field = o['field'] as String? ?? '';
          if (o['op'] == 'setField') {
            (comp['data'] as Map)[field] = jsonDecode(jsonEncode(o['value']));
          } else {
            (comp['data'] as Map).remove(field);
          }
          (overridden[o['component'] as String] ??= {}).add(field);
        case 'addComponent':
          if (target == null) {
            bad(o, 'no entity $local in the prefab');
            continue;
          }
          final cid = o['component'] as String;
          (target['components'] as Map)[cid] = {
            'type': o['type'],
            'version': o['version'] ?? 1,
            'data': jsonDecode(jsonEncode(o['data'] ?? <String, Object?>{})),
          };
          addedComponents.add(cid);
        case 'removeComponent':
          final removed = target == null ? null : (target['components'] as Map).remove(o['component']);
          if (removed == null) {
            bad(o, 'no component ${o['component']} on entity $local in the prefab');
            continue;
          }
        case 'addObject':
          final parent = o['parent'] as String?;
          if (parent != null && !entities.containsKey(parent)) {
            bad(o, 'no parent $parent in the prefab');
            continue;
          }
          final comps = <String, Object?>{};
          final given = o['components'];
          if (given is Map) {
            for (final c in given.entries) {
              comps[c.key as String] = jsonDecode(jsonEncode(c.value));
              addedComponents.add(c.key as String);
            }
          }
          entities[local] = {'name': o['name'] ?? '', 'parent': parent, 'children': <String>[], 'components': comps};
          if (parent == null) {
            roots.add(local);
          } else {
            (entities[parent]!['children'] as List).add(local);
          }
          addedEntities.add(local);
        case 'removeObject':
          if (target == null) {
            bad(o, 'no entity $local in the prefab');
            continue;
          }
          void drop(String id) {
            final t = entities.remove(id);
            if (t == null) return;
            for (final c in t['children'] as List) {
              drop(c as String);
            }
          }
          final parent = target['parent'] as String?;
          drop(local);
          if (parent == null) {
            roots.remove(local);
          } else {
            (entities[parent]?['children'] as List?)?.remove(local);
          }
        default:
          diagnostics.add({
            'code': 'FYN-D0407',
            'severity': 'error',
            'message': 'override ${o['index']}: malformed op ${o['op']}',
            'id': anchor,
            'pointer': o['pointer'],
          });
      }
    }
  }
  final expanded = source != null && diagnostics.isEmpty;
  final entityJson = <Map<String, Object?>>[];
  if (expanded) {
    for (final en in entities.entries) {
      final local = en.key;
      final t = en.value;
      final parent = t['parent'] as String?;
      entityJson.add({
        'id': derive(local),
        'local': local,
        'name': t['name'],
        'parent': parent == null ? anchor : derive(parent),
        'children': [for (final c in t['children'] as List) derive(c as String)],
        'added': addedEntities.contains(local),
        'components': {
          for (final c in (t['components'] as Map).entries)
            derive(c.key as String): {
              'id': derive(c.key as String),
              'local': c.key,
              'type': (c.value as Map)['type'],
              'version': (c.value as Map)['version'] ?? 1,
              'data': (c.value as Map)['data'],
              'added': addedComponents.contains(c.key) || addedEntities.contains(local),
              'overridden': (overridden[c.key]?.toList() ?? const <String>[]),
            },
        },
      });
    }
  }
  // Annotate the overrides with where they landed.
  for (final o in overrides) {
    final local = o['entity'] as String?;
    if (expanded && local != null && entities.containsKey(local)) {
      o['entityId'] = derive(local);
      o['entityName'] = entities[local]!['name'];
      final comp = o['component'] as String?;
      if (comp != null) {
        final c = (entities[local]!['components'] as Map)[comp] as Map?;
        if (c != null) {
          o['componentId'] = derive(comp);
          o['componentType'] = c['type'];
        } else if (o['op'] == 'removeComponent') {
          final original = (source.entitiesView[local]?['components'] as Map?)?[comp] as Map?;
          o['componentType'] = original?['type'];
        }
      }
    } else if (local != null && source != null && source.entitiesView.containsKey(local)) {
      o['entityName'] = source.entitiesView[local]!['name'];
    }
  }
  return PrefabExpansion.fromJson({
    'anchor': anchor,
    'instance': instance,
    'asset': {'asset': asset, 'sub': ?sub},
    'expanded': expanded,
    'prefab': prefabBlock,
    'diagnostics': diagnostics,
    'entities': entityJson,
    'overrides': overrides,
  });
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
