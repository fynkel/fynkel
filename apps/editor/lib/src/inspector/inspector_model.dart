// The Inspector's model: what the focused tab's selection is made of, as
// component slots the panel draws, and every edit as one transaction on
// the document (blueprint §19.2e item 4; editor-ux §22.6).
//
// Multi-object editing: the selected entities are read (`entities(ids)`)
// and their components paired by type and position — a slot is shown when
// every selected entity has that component; a field whose values differ
// across the slot's instances is *mixed*, and setting it writes one
// `setField` per instance inside a single transaction, so one undo undoes
// it everywhere. A rejected transaction (the crate's validation, a stale
// base) leaves the document untouched; the field keeps the message until
// its next accepted edit and shows the committed value again.
//
// Specs come from `field_spec.dart` over the project's schemas and the
// committed behaviour metadata; nothing here runs game code.
import 'dart:collection';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../documents/document_tabs.dart';
import '../documents/open_document.dart';
import '../project/editor_project.dart';
import 'codegen_check.dart';
import 'field_spec.dart';
import 'inspector_math.dart';

/// One component on one selected entity.
final class ComponentInstance {
  const ComponentInstance({
    required this.entityId,
    required this.componentId,
    required this.type,
    required this.version,
    required this.data,
  });

  final String entityId;
  final String componentId;
  final String type;
  final int version;
  final Map<String, Object?> data;
}

/// The same component type at the same position on every selected entity.
final class ComponentSlot {
  ComponentSlot({required this.type, required this.index, required this.spec, required this.instances});

  final String type;

  /// Position among the entity's components of this type (0 for the first).
  final int index;
  final ComponentSpec spec;

  /// One per selected entity, in selection order.
  final List<ComponentInstance> instances;

  String get key => '$type#$index';
  ComponentInstance get first => instances.first;
  bool get single => instances.length == 1;

  /// The value at [path] (a field name, or `a/1/b`) on the first instance.
  Object? value(String path) => readPath(first.data, path);

  /// Whether the instances disagree at [path].
  bool isMixed(String path) {
    if (single) return false;
    final v = value(path);
    for (final i in instances.skip(1)) {
      if (!deepEquals(readPath(i.data, path), v)) return true;
    }
    return false;
  }

  /// Whether every instance has [path] set (a missing key means the schema default applies).
  bool isSet(String path) => instances.every((i) => hasPath(i.data, path));

  @override
  String toString() => 'ComponentSlot($key × ${instances.length})';
}

/// A selected entity's header data.
final class EntityInfo {
  const EntityInfo({required this.id, required this.name, this.prefab});

  final String id;
  final String name;
  final PrefabInstanceInfo? prefab;
}

/// A scene entity that is a prefab instance anchor: `prefab: { asset, instance, overrides }`.
final class PrefabInstanceInfo {
  const PrefabInstanceInfo({required this.assetId, this.sub, required this.instance, required this.overrides});

  final String assetId;
  final String? sub;
  final String instance;

  /// The override ops in document order (prefab-local ids).
  final List<Map<String, Object?>> overrides;
}

/// A type the Add Component menu offers.
final class AddableType {
  const AddableType({required this.type, required this.spec});

  final String type;
  final ComponentSpec spec;
  String get title => spec.title;
  String get category => spec.category ?? (spec.isBehaviour ? 'Behaviours' : 'Engine');
}

final class InspectorModel extends ChangeNotifier {
  InspectorModel({required this.tabs, required this.project, this.codegen, this.openInIde, this.maxEntities = 200}) {
    tabs.addListener(_onTabs);
    project.addListener(_onProject);
    codegen?.addListener(notifyListeners);
    _onTabs();
  }

  final DocumentTabs tabs;
  final EditorProject project;
  final CodegenCheck? codegen;

  /// Opens a source file in the configured IDE; answers an error or null.
  final Future<String?> Function(String file, {int? line, int? column})? openInIde;

  /// Above this many selected entities the Inspector shows a count only.
  final int maxEntities;

  OpenDocument? _tab;
  List<String> _ids = const [];
  List<EntityInfo> _entities = const [];
  List<ComponentSlot> _slots = const [];
  final _errors = <String, String>{};
  Map<String, ComponentSpec>? _specs;
  Map<String, TypeSchema>? _schemas;
  Map<String, Map<String, Object?>>? _metadataByType;

  /// The last refusal that is not one field's (a rejected Add Component, a
  /// read-only document): shown under the cards until the selection changes,
  /// the next transaction is accepted — from anywhere — or [dismissError].
  String? lastError;

  void dismissError() {
    if (lastError == null) return;
    lastError = null;
    notifyListeners();
  }

  int refreshes = 0;

  OpenDocument? get tab => _tab;
  bool get hasDocument => _tab != null;
  bool get canEdit => _tab != null && _tab!.document.readOnly == null;
  List<String> get selection => _ids;
  List<EntityInfo> get entities => _entities;
  List<ComponentSlot> get slots => _slots;
  bool get tooMany => _ids.length > maxEntities;
  EntityInfo? get single => _entities.length == 1 ? _entities.first : null;

  /// Component types per selected entity id (a slot needs every entity; this is each one's own list).
  final Map<String, Set<String>> _typesOn = {};

  /// The selected entities that already hold a component of [type] — the
  /// document allows one per type per entity (`FYN-D0204`), so Add Component
  /// greys these out and [addComponent] refuses before asking the crate.
  List<EntityInfo> entitiesWith(String type) => [
    for (final e in _entities)
      if (_typesOn[e.id]?.contains(type) ?? false) e,
  ];

  /// Field errors by `${slot.key}/${field}`; the message stays until the field's next accepted edit.
  UnmodifiableMapView<String, String> get errors => UnmodifiableMapView(_errors);
  String? errorFor(ComponentSlot slot, String field) => _errors['${slot.key}/$field'];

  // ── Wiring ──────────────────────────────────────────────────────────────

  void _onTabs() {
    final t = tabs.focused;
    if (t != _tab) {
      _tab?.removeListener(_onTab);
      _tab = t;
      _tab?.addListener(_onTab);
      _errors.clear();
    }
    _refresh();
  }

  void _onTab() {
    // The tab's last transaction was accepted (its own message is gone): the Inspector's goes too.
    if (_tab?.lastError == null) lastError = null;
    _refresh();
  }

  void _onProject() {
    // Schemas or metadata may have changed on disk (the watcher rescanned).
    _specs = null;
    _schemas = null;
    _metadataByType = null;
    _refresh();
  }

  void _refresh() {
    final t = _tab;
    refreshes++;
    if (t == null) {
      _ids = const [];
      _entities = const [];
      _slots = const [];
      notifyListeners();
      return;
    }
    final ids = List.of(t.selection);
    if (!listEquals(ids, _ids)) lastError = null;
    _ids = ids;
    _typesOn.clear();
    if (_ids.isEmpty || tooMany) {
      _entities = const [];
      _slots = const [];
      notifyListeners();
      return;
    }
    final EntityPatch patch;
    try {
      patch = t.document.entities(_ids);
    } catch (e) {
      lastError = '$e';
      _entities = const [];
      _slots = const [];
      notifyListeners();
      return;
    }
    final infos = <EntityInfo>[];
    final perEntity = <List<ComponentInstance>>[];
    for (final id in _ids) {
      final e = patch.entities[id];
      if (e == null) continue; // deleted under us; the tab prunes the selection next
      infos.add(EntityInfo(id: id, name: e['name'] as String? ?? '', prefab: _prefabOf(e)));
      final comps = <ComponentInstance>[];
      final types = _typesOn[id] = <String>{};
      final map = e['components'];
      if (map is Map) {
        for (final entry in map.entries) {
          final c = entry.value;
          if (c is! Map) continue;
          comps.add(
            ComponentInstance(
              entityId: id,
              componentId: entry.key as String,
              type: c['type'] as String? ?? '',
              version: (c['version'] as num?)?.toInt() ?? 1,
              data: c['data'] is Map ? Map<String, Object?>.from(c['data'] as Map) : <String, Object?>{},
            ),
          );
          types.add(c['type'] as String? ?? '');
        }
      }
      perEntity.add(comps);
    }
    _entities = infos;
    _slots = _pair(perEntity);
    // Errors of slots that vanished go with them.
    _errors.removeWhere((k, _) => !_slots.any((s) => k.startsWith('${s.key}/')));
    notifyListeners();
  }

  PrefabInstanceInfo? _prefabOf(Map<String, Object?> e) {
    final prefab = e['prefab'];
    if (prefab is! Map) return null;
    final asset = prefab['asset'];
    final assetId = asset is Map ? asset['asset'] as String? ?? '' : asset?.toString() ?? '';
    return PrefabInstanceInfo(
      assetId: assetId,
      sub: asset is Map ? asset['sub'] as String? : null,
      instance: prefab['instance'] as String? ?? '',
      overrides: [
        for (final o in prefab['overrides'] as List? ?? const [])
          if (o is Map) Map<String, Object?>.from(o),
      ],
    );
  }

  /// Components of the first entity, in its order; each kept only when every
  /// other entity has a component of that type at the same position.
  List<ComponentSlot> _pair(List<List<ComponentInstance>> perEntity) {
    if (perEntity.isEmpty) return const [];
    final out = <ComponentSlot>[];
    final counts = <String, int>{};
    for (final c in perEntity.first) {
      final index = counts[c.type] ?? 0;
      counts[c.type] = index + 1;
      final instances = <ComponentInstance>[c];
      var complete = true;
      for (final others in perEntity.skip(1)) {
        final same = others.where((o) => o.type == c.type).toList();
        if (same.length <= index) {
          complete = false;
          break;
        }
        instances.add(same[index]);
      }
      if (!complete) continue;
      out.add(ComponentSlot(type: c.type, index: index, spec: specFor(c.type), instances: instances));
    }
    return out;
  }

  // ── Specs ───────────────────────────────────────────────────────────────

  Map<String, TypeSchema> get _schemaMap => _schemas ??= {for (final s in project.typeSchemas()) s.type: s};

  Map<String, Map<String, Object?>> get _metadata => _metadataByType ??= {
    for (final b in project.behaviourMetadata()?['behaviours'] as List? ?? const [])
      if (b is Map) (b['id'] as String? ?? ''): Map<String, Object?>.from(b),
  };

  /// The spec of [type]: from its schema and, for a behaviour, the committed
  /// metadata; a type nobody knows gets an empty spec whose data shows as JSON.
  ComponentSpec specFor(String type) {
    final cache = _specs ??= {};
    return cache.putIfAbsent(type, () {
      final s = _schemaMap[type];
      final meta = _metadata[type];
      if (s == null && meta == null) {
        return ComponentSpec(
          type: type,
          fields: const [],
          isBehaviour: !type.startsWith('fynkel.'),
          missingSchema: true,
          additionalProperties: true,
        );
      }
      // A behaviour whose schema file is missing still has its committed metadata's schema.
      final schema = s?.schema ?? (meta?['schema'] is Map ? Map<String, Object?>.from(meta!['schema'] as Map) : null);
      return parseComponentSpec(
        type,
        schema,
        metadata: meta,
        allowed: s?.allowed ?? true,
        authored: s?.authored ?? true,
      );
    });
  }

  /// The instant staleness signal: committed metadata vs schema files.
  List<CommittedDrift> get committedDrift => committedDrift_(project.behaviourMetadata(), {
    for (final s in _schemaMap.values)
      if (s.isBehaviour) s.type: s.schema,
  });

  /// The project has behaviour metadata at all (a project without any Dart
  /// behaviours has none and gets no banner).
  bool get hasMetadata => project.behaviourMetadata() != null;

  /// The types Add Component offers: authored, allowed native components
  /// and every behaviour, filtered by [query] over type, title and category.
  List<AddableType> addableTypes([String query = '']) {
    final q = query.trim().toLowerCase();
    final out = <AddableType>[];
    final seen = <String>{};
    for (final s in _schemaMap.values) {
      if (!s.isBehaviour && !s.authored) continue;
      if (!s.allowed) continue;
      if (!seen.add(s.type)) continue;
      out.add(AddableType(type: s.type, spec: specFor(s.type)));
    }
    for (final type in _metadata.keys) {
      if (type.isNotEmpty && seen.add(type)) out.add(AddableType(type: type, spec: specFor(type)));
    }
    out.retainWhere(
      (t) =>
          q.isEmpty ||
          t.type.toLowerCase().contains(q) ||
          t.title.toLowerCase().contains(q) ||
          t.category.toLowerCase().contains(q),
    );
    out.sort((a, b) {
      if (a.spec.isBehaviour != b.spec.isBehaviour) return a.spec.isBehaviour ? -1 : 1;
      final c = a.category.compareTo(b.category);
      return c != 0 ? c : a.title.compareTo(b.title);
    });
    return out;
  }

  /// Names of the document's entities (the entity picker), sorted, unique.
  List<String> entityNames() {
    final t = _tab;
    if (t == null) return const [];
    final names = <String>{};
    for (final n in t.tree.nodes.values) {
      if (n.name.isNotEmpty) names.add(n.name);
    }
    return names.toList()..sort((a, b) => a.toLowerCase().compareTo(b.toLowerCase()));
  }

  /// The assets an `asset(<kind>)` field may take (every asset when [kind] is null).
  List<AssetEntry> assetsFor(String? kind) {
    final all = project.assets();
    if (kind == null) return all;
    return [
      for (final a in all)
        if (a.offeredKinds.contains(kind)) a,
    ];
  }

  /// `name (path)` of an asset reference, for the field's face.
  String describeAsset(Map<String, Object?>? ref) {
    if (ref == null) return 'None';
    final id = ref['asset'] as String? ?? '';
    final sub = ref['sub'] as String?;
    for (final a in project.assets()) {
      if (a.id == id) return sub == null || sub.isEmpty ? a.name : '${a.name} › $sub';
      for (final e in a.subAssets.entries) {
        if (e.value == id) return '${a.name} › ${e.key}';
      }
    }
    return sub == null || sub.isEmpty ? id : '$id#$sub';
  }

  // ── Edits (each one transaction) ────────────────────────────────────────

  Applied? _run(List<Map<String, Object?>> commands) {
    final t = _tab;
    if (t == null || commands.isEmpty || !canEdit) return null;
    final a = t.transaction(commands);
    lastError = t.lastError;
    return a;
  }

  /// Validates [value] against [spec] before a command is sent: the hints'
  /// bounds, an asset that must exist, an enum value that must be listed.
  /// Returns the problem or null.
  String? precheck(FieldSpec spec, Object? value) {
    if (value == null) return spec.nullable ? null : '${spec.label} cannot be empty';
    switch (spec.kind) {
      case FieldKind.number:
      case FieldKind.integer:
        if (value is! num) return 'not a number';
        if (spec.kind == FieldKind.integer && value is! int && value != value.roundToDouble()) {
          return 'not a whole number';
        }
        final min = spec.min, max = spec.max;
        if (min != null && value < min) return 'below the minimum ${_fmt(min, spec)}';
        if (max != null && value > max) return 'above the maximum ${_fmt(max, spec)}';
      case FieldKind.enumeration:
        if (spec.enumValues.isNotEmpty && !spec.enumValues.contains(value)) {
          return "'$value' is not one of ${spec.enumValues.join(', ')}";
        }
      case FieldKind.assetRef:
        if (value is! Map) return 'not an asset reference';
        final id = value['asset'] as String? ?? '';
        final sub = value['sub'] as String?;
        if (!project.assetExists(id, sub: sub)) {
          return 'FYN-A0004: unknown asset ${sub == null || sub.isEmpty ? id : '$id#$sub'}';
        }
      case FieldKind.vec2:
      case FieldKind.vec3:
      case FieldKind.vec4:
      case FieldKind.quaternion:
      case FieldKind.color:
        if (value is! List || value.length != spec.components || value.any((v) => v is! num)) {
          return 'needs ${spec.components} numbers';
        }
      default:
        break;
    }
    return null;
  }

  String _fmt(num v, FieldSpec spec) => spec.angle ? '${(v * 180 / 3.141592653589793).toStringAsFixed(1)}°' : '$v';

  /// Sets [field] (a name or a `/pointer`) to [value] on every instance of
  /// [slot] in one transaction. [spec] is the field's spec when the
  /// precheck should run (a component of a vector passes null).
  bool setField(ComponentSlot slot, String field, Object? value, {FieldSpec? spec}) {
    final key = '${slot.key}/${_topLevel(field)}';
    if (spec != null) {
      final problem = precheck(spec, value);
      if (problem != null) {
        _errors[key] = problem;
        notifyListeners();
        return false;
      }
    }
    final commands = [
      for (final i in slot.instances) Command.setField(i.entityId, i.componentId, field, _copyJson(value)),
    ];
    return _commit(commands, key);
  }

  /// Several fields of the slot at once (a whole vector, Euler angles → one quaternion), one transaction.
  bool setFields(ComponentSlot slot, Map<String, Object?> values) {
    final commands = <Map<String, Object?>>[];
    for (final i in slot.instances) {
      for (final e in values.entries) {
        commands.add(Command.setField(i.entityId, i.componentId, e.key, _copyJson(e.value)));
      }
    }
    final key = '${slot.key}/${_topLevel(values.keys.first)}';
    return _commit(commands, key);
  }

  /// Removes [field] from every instance (the schema default applies again).
  bool unsetField(ComponentSlot slot, String field) {
    final key = '${slot.key}/${_topLevel(field)}';
    return _commit([for (final i in slot.instances) Command.unsetField(i.entityId, i.componentId, field)], key);
  }

  bool _commit(List<Map<String, Object?>> commands, String errorKey) {
    final a = _run(commands);
    if (a == null) {
      if (lastError != null) _errors[errorKey] = lastError!;
      notifyListeners();
      return false;
    }
    if (a.ok) {
      _errors.remove(errorKey);
      // The tab's notification refreshes the slots; this makes the clear visible at once.
      notifyListeners();
      return true;
    }
    _errors[errorKey] = describeRejection(a);
    notifyListeners();
    return false;
  }

  /// Adds [type] to every selected entity: a native component with its Rust
  /// defaults, a behaviour with its schema defaults. One transaction.
  bool addComponent(String type) {
    if (_entities.isEmpty || !canEdit) return false;
    final spec = specFor(type);
    final holders = entitiesWith(type);
    if (holders.isNotEmpty) {
      final who = holders.length == 1 && _entities.length == 1
          ? (holders.single.name.isEmpty ? holders.single.id : holders.single.name)
          : holders.length == _entities.length
          ? 'every selected entity'
          : '${holders.length} of the ${_entities.length} selected entities';
      lastError = '${spec.title} is already on $who — an entity holds one component per type (FYN-D0204).';
      notifyListeners();
      return false;
    }
    Map<String, Object?> data;
    if (spec.isBehaviour) {
      data = spec.schemaDefaults();
    } else {
      final d = project.componentDefaults(type);
      if (d == null) {
        lastError = 'No defaults for $type — it is not an authorable native component.';
        notifyListeners();
        return false;
      }
      data = d;
    }
    final commands = [
      for (final e in _entities)
        Command.addComponent(e.id, {'type': type, 'version': spec.version ?? 1, 'data': _copyJson(data)}),
    ];
    final a = _run(commands);
    notifyListeners();
    return a?.ok ?? false;
  }

  /// Removes the slot's component from every selected entity. One transaction.
  bool removeComponent(ComponentSlot slot) {
    final a = _run([for (final i in slot.instances) Command.removeComponent(i.entityId, i.componentId)]);
    _errors.removeWhere((k, _) => k.startsWith('${slot.key}/'));
    notifyListeners();
    return a?.ok ?? false;
  }

  /// Puts every field of the slot back to its default: the native `Default`
  /// or the behaviour's schema defaults, as `setField`s in one transaction.
  bool resetToDefaults(ComponentSlot slot) {
    final defaults = slot.spec.isBehaviour ? slot.spec.schemaDefaults() : project.componentDefaults(slot.type);
    if (defaults == null || defaults.isEmpty) return false;
    final commands = <Map<String, Object?>>[];
    for (final i in slot.instances) {
      for (final e in defaults.entries) {
        if (!deepEquals(i.data[e.key], e.value)) {
          commands.add(Command.setField(i.entityId, i.componentId, e.key, _copyJson(e.value)));
        }
      }
    }
    if (commands.isEmpty) return true;
    final a = _run(commands);
    if (a?.ok ?? false) _errors.removeWhere((k, _) => k.startsWith('${slot.key}/'));
    notifyListeners();
    return a?.ok ?? false;
  }

  /// Renames the single selected entity.
  bool rename(String name) {
    final e = single;
    if (e == null || name.trim().isEmpty || name == e.name) return false;
    final a = _run([Command.renameObject(e.id, name.trim())]);
    notifyListeners();
    return a?.ok ?? false;
  }

  /// Drops one prefab override of [entityId] (all of them when [index] is null).
  bool revertOverride(String entityId, {int? index}) {
    final a = _run([Command.revertPrefabOverride(entityId, index: index)]);
    notifyListeners();
    return a?.ok ?? false;
  }

  /// The behaviour's source file of [slot] (`package:<pkg>/<path>` →
  /// `<project>/lib/<path>` when the package is the project's), or null.
  String? scriptPath(ComponentSlot slot) {
    final lib = slot.spec.library;
    if (lib == null) return null;
    final uri = Uri.tryParse(lib);
    if (uri == null || uri.scheme != 'package' || uri.pathSegments.length < 2) return null;
    final package = uri.pathSegments.first;
    final rest = uri.pathSegments.skip(1).join('/');
    final own = project.behaviourMetadata()?['package'] as String?;
    if (own != null && own != package) return null;
    return p.join(project.dir, 'lib', rest);
  }

  /// Opens the behaviour's source in the IDE; answers the error or null.
  Future<String?> openScript(ComponentSlot slot) async {
    final file = scriptPath(slot);
    if (file == null) {
      return '${slot.type} has no source file in this project (${slot.spec.library ?? 'no library in the metadata'})';
    }
    final open = openInIde;
    if (open == null) return 'no IDE command is configured';
    return open(file);
  }

  static String _topLevel(String field) => field.startsWith('/') ? field.substring(1).split('/').first : field;

  @override
  void dispose() {
    tabs.removeListener(_onTabs);
    project.removeListener(_onProject);
    codegen?.removeListener(notifyListeners);
    _tab?.removeListener(_onTab);
    super.dispose();
  }
}

/// `committedDrift` under a name the model's getter does not shadow.
List<CommittedDrift> committedDrift_(Map<String, Object?>? metadata, Map<String, Map<String, Object?>?> files) =>
    committedDrift(metadata, files);

// ── Value helpers ──────────────────────────────────────────────────────────

/// Reads `a/1/b` (or a plain key) out of [data]; null when absent.
Object? readPath(Map<String, Object?> data, String path) {
  final parts = path.startsWith('/') ? path.substring(1).split('/') : path.split('/');
  Object? node = data;
  for (final part in parts) {
    if (node is Map) {
      if (!node.containsKey(part)) return null;
      node = node[part];
    } else if (node is List) {
      final i = int.tryParse(part);
      if (i == null || i < 0 || i >= node.length) return null;
      node = node[i];
    } else {
      return null;
    }
  }
  return node;
}

bool hasPath(Map<String, Object?> data, String path) {
  final parts = path.startsWith('/') ? path.substring(1).split('/') : path.split('/');
  Object? node = data;
  for (final part in parts) {
    if (node is Map) {
      if (!node.containsKey(part)) return false;
      node = node[part];
    } else if (node is List) {
      final i = int.tryParse(part);
      if (i == null || i < 0 || i >= node.length) return false;
      node = node[i];
    } else {
      return false;
    }
  }
  return true;
}

bool deepEquals(Object? a, Object? b) {
  if (a is num && b is num) return a == b;
  if (a is List && b is List) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (!deepEquals(a[i], b[i])) return false;
    }
    return true;
  }
  if (a is Map && b is Map) {
    if (a.length != b.length) return false;
    for (final k in a.keys) {
      if (!b.containsKey(k) || !deepEquals(a[k], b[k])) return false;
    }
    return true;
  }
  return a == b;
}

Object? _copyJson(Object? v) => switch (v) {
  Map() => <String, Object?>{for (final e in v.entries) e.key as String: _copyJson(e.value)},
  List() => <Object?>[for (final e in v) _copyJson(e)],
  _ => v,
};

/// The number to write into a field that held [previous] (keeps `1` an int
/// where the document had one — the byte-exact round trip).
Object numberFor(FieldSpec spec, double v, {Object? previous}) =>
    jsonNumber(v, previous: previous, integer: spec.kind == FieldKind.integer || spec.kind == FieldKind.layerMask);
