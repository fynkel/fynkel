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
//
// Prefab mode (item 7): a selected derived entity (one a prefab instance
// expands to) is read from its anchor's expansion, not the document; its
// slots show the applied values with the overridden fields marked, and
// every edit becomes an override op on the anchor (`setPrefabOverride`,
// one transaction) — a field's `setField`/`unsetField`, `addComponent`,
// `removeComponent`, an added object's re-issued `addObject`. Per field:
// Revert (drop that override) and Apply to prefab (`prefab_actions.dart`).
import 'dart:collection';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../documents/document_tabs.dart';
import '../documents/open_document.dart';
import '../hierarchy/hierarchy_tree.dart' show HierarchyNode;
import '../hierarchy/transform_math.dart' show Trs;
import '../play/apply_to_scene.dart';
import '../play/play_document.dart' show PlayDocument;
import '../project/editor_project.dart';
import '../project_browser/project_browser_model.dart' show AssetPreview;
import '../scene_view/component_handles.dart' show ComponentHandlers;
import '../scene_view/declared_gizmos.dart' show DeclaredGizmos;
import 'codegen_check.dart';
import 'component_specs.dart';
import 'field_spec.dart';
import 'inspector_math.dart';
import 'prefab_actions.dart' as prefab;

/// One component on one selected entity.
final class ComponentInstance {
  const ComponentInstance({
    required this.entityId,
    required this.componentId,
    required this.type,
    required this.version,
    required this.data,
    this.anchor,
    this.entityLocal,
    this.local,
    this.added = false,
    this.overridden = const {},
  });

  final String entityId;
  final String componentId;
  final String type;
  final int version;
  final Map<String, Object?> data;

  /// Set on a derived component: the prefab instance's anchor, the
  /// prefab-local ids of the entity and the component (what an override
  /// names), whether an override added the component, and the keys of
  /// `data` an override wrote.
  final String? anchor;
  final String? entityLocal;
  final String? local;
  final bool added;
  final Set<String> overridden;

  bool get isDerived => anchor != null;

  /// The same component with the same values.
  bool sameAs(ComponentInstance o) =>
      entityId == o.entityId &&
      componentId == o.componentId &&
      type == o.type &&
      version == o.version &&
      anchor == o.anchor &&
      entityLocal == o.entityLocal &&
      local == o.local &&
      added == o.added &&
      setEquals(overridden, o.overridden) &&
      deepEquals(data, o.data);
}

/// The same component type at the same position on every selected entity.
/// Every object has a Transform, as in Unity: the Inspector never removes it.
const transformType = 'fynkel.Transform';
const transformStays = 'Every object has a Transform: it cannot be removed.';

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

  /// What the Inspector shows at [path]: the value, or — when the data
  /// does not have the field (a field added to the script after the
  /// component was) — its schema default, which is what the game uses.
  Object? shown(String path) => _shownIn(first.data, path);

  /// [shown] on one of the instances.
  Object? shownFor(ComponentInstance i, String path) => _shownIn(i.data, path);

  Object? _shownIn(Map<String, Object?> data, String path) {
    if (hasPath(data, path)) return readPath(data, path);
    final top = topLevelKey(path);
    if (data.containsKey(top)) return null;
    final f = spec.field(top);
    return f == null || !f.hasDefault ? null : readPath({top: f.defaultValue}, path);
  }

  /// Whether the instances disagree at [path] (as shown).
  bool isMixed(String path) {
    if (single) return false;
    final v = shown(path);
    for (final i in instances.skip(1)) {
      if (!deepEquals(_shownIn(i.data, path), v)) return true;
    }
    return false;
  }

  /// Whether every instance has [path] set (a missing key means the schema default applies).
  bool isSet(String path) => instances.every((i) => hasPath(i.data, path));

  /// Every instance is a derived component of a prefab instance.
  bool get isDerived => instances.every((i) => i.isDerived);

  /// Some instance is derived (mixed derived and document components edit
  /// per instance: overrides for the derived, `setField`s for the rest).
  bool get anyDerived => instances.any((i) => i.isDerived);

  /// Whether [path]'s top-level key is overridden on some derived instance (shown bold).
  bool isOverridden(String path) {
    final key = topLevelKey(path);
    return instances.any((i) => i.isDerived && i.overridden.contains(key));
  }

  /// The same components with the same values, under the same spec.
  bool sameAs(ComponentSlot o) {
    if (type != o.type || index != o.index || !identical(spec, o.spec)) return false;
    if (instances.length != o.instances.length) return false;
    for (var i = 0; i < instances.length; i++) {
      if (!instances[i].sameAs(o.instances[i])) return false;
    }
    return true;
  }

  @override
  String toString() => 'ComponentSlot($key × ${instances.length})';
}

/// The components the Inspector draws in the object header rather than as
/// cards (ADR-0020): the object's layer and its tags.
const headerTypes = {'fynkel.Layer', 'fynkel.Tags'};

/// Whether every, none or only some of the selected entities carry a tag.
enum TagState { all, none, mixed }

/// A selected entity's header data.
final class EntityInfo {
  const EntityInfo({
    required this.id,
    required this.name,
    this.prefab,
    this.expansion,
    this.anchor,
    this.local,
    this.added = false,
    this.active = true,
    this.activeOverridden = false,
  });

  final String id;
  final String name;
  final PrefabInstanceInfo? prefab;

  /// The entity's own active flag (Unity's `activeSelf`), and for a
  /// derived entity whether a `setActive` override set it.
  final bool active;
  final bool activeOverridden;

  /// For an anchor: its expansion (the prefab it comes from, the annotated
  /// overrides, why it did not expand); null when the query failed.
  final PrefabExpansion? expansion;

  /// For a derived entity: its instance's anchor, its prefab-local id
  /// (what overrides address), and whether an override added it.
  final String? anchor;
  final String? local;
  final bool added;

  bool get isDerived => anchor != null;
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
  late final _specs = ComponentSpecs(project);

  /// The last refusal that is not one field's (a rejected Add Component, a
  /// read-only document): shown under the cards until the selection changes,
  /// the next transaction is accepted — from anywhere — or [dismissError].
  String? lastError;

  /// Select prefab asset: reveals the asset in the Project browser (wired
  /// by the shell); null when no browser is around.
  void Function(String assetId, String? sub)? revealAsset;

  /// Unity's ping: the asset outlined in the Project browser, nothing
  /// selected (set by the shell).
  void Function(String assetId)? pingAsset;

  /// An asset's picture for the asset slots and their picker (a texture's
  /// pixels, a material's swatch), from the Project browser (wired by the
  /// shell); null draws the kind's icon.
  AssetPreview Function(AssetEntry entry)? assetPreview;

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

  /// Each selected entity's `fynkel.Layer` and `fynkel.Tags` (absent when it has none).
  final Map<String, ComponentInstance> _layerOn = {}, _tagsOn = {};

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

  // ── Edit in the Scene view (item 9g) ─────────────────────────────────────

  /// Whether [slot] has handles the Scene view can show: one entity shown,
  /// the tab's lead, a writable document, a type with registered handles
  /// for its data (a mesh collider has none) or with a declared gizmo to
  /// drag (`@Gizmo`, item 9h).
  bool canEditInScene(ComponentSlot slot) {
    final t = _tab, e = single;
    return t != null &&
        e != null &&
        canEdit &&
        t.lead == e.id &&
        (ComponentHandlers.of(slot.type) != null
            ? ComponentHandlers.supports(slot.type, slot.first.data)
            : DeclaredGizmos.of(slot.spec)?.draggable ?? false);
  }

  /// The Scene view shows [slot]'s handles now.
  bool isEditingInScene(ComponentSlot slot) => canEditInScene(slot) && _tab!.editingType == slot.type;

  /// The Edit toggle: [slot]'s handles in the Scene view, or off again.
  void toggleEditInScene(ComponentSlot slot) {
    if (!canEditInScene(slot)) return;
    _tab!.editComponent(isEditingInScene(slot) ? null : slot.type);
  }

  // ── Wiring ──────────────────────────────────────────────────────────────

  // ── Lock (item 9c) ──────────────────────────────────────────────────────

  List<String>? _lockedIds;

  /// The Inspector keeps showing these entities of this tab while the
  /// selection moves on (Unity's padlock); null when it follows the
  /// selection.
  bool get locked => _lockedIds != null;

  /// Pins the entities shown now; unpinning follows the selection again.
  void setLocked(bool on) {
    if (on == locked) return;
    if (on && (_tab == null || _ids.isEmpty)) return;
    _lockedIds = on ? List.of(_ids) : null;
    _onTabs();
  }

  void _onTabs() {
    // A locked Inspector stays on its tab until that tab closes.
    if (locked && !tabs.open.contains(_tab) && !identical(_tab, tabs.live)) _lockedIds = null;
    final t = locked ? _tab : tabs.shown;
    if (t != _tab) {
      _tab?.removeListener(_onTab);
      _liveValues?.removeListener(_onLiveValues);
      _tab = t;
      _tab?.addListener(_onTab);
      _liveValues = switch (_tab?.document) {
        final PlayDocument d => d.values,
        _ => null,
      };
      _liveValues?.addListener(_onLiveValues);
      _errors.clear();
      _fieldValues.clear();
    }
    _refresh();
  }

  // ── Live Play (ADR-0021 P7) ─────────────────────────────────────────────

  /// The running game's value changes (a selected entity moved), when the
  /// tab shown is the running game.
  ValueListenable<int>? _liveValues;
  bool _liveOnly = false;
  int _generation = 0;

  /// Moves on every change but a live value refresh: while it stands, a
  /// card whose slot is the same object may be drawn as it was.
  int get generation => _generation;

  void _onLiveValues() {
    _liveOnly = true;
    try {
      _refresh();
    } finally {
      _liveOnly = false;
    }
  }

  /// The number, vector and rotation fields shown of the running game's
  /// one selected entity, by slot key and path: what their editors listen
  /// to between builds.
  final _fieldValues = <String, Map<String, ValueNotifier<Object?>>>{};

  /// [slot]'s value at [path] as it moves while the game runs, for a field
  /// editor to follow without a rebuild; null unless the Inspector shows
  /// one entity of the running game.
  ValueListenable<Object?>? liveField(ComponentSlot slot, String path) {
    if (_liveValues == null || !slot.single) return null;
    return (_fieldValues[slot.key] ??= {}).putIfAbsent(path, () => ValueNotifier(slot.value(path)));
  }

  /// A live refresh whose only changes are values [liveField] carries:
  /// [next] replaces the slots without telling the panel, and the fields
  /// follow on their own. False when anything else changed (a name, a
  /// component, a field nobody follows), for a rebuild.
  bool _moveInPlace(List<EntityInfo> infos, List<ComponentSlot> next) {
    if (!_liveOnly || next.length != _slots.length || infos.length != _entities.length) return false;
    for (var i = 0; i < infos.length; i++) {
      final a = infos[i], b = _entities[i];
      if (a.id != b.id || a.name != b.name || a.active != b.active) return false;
    }
    for (var i = 0; i < next.length; i++) {
      final a = _slots[i], b = next[i];
      if (identical(a, b)) continue;
      if (a.key != b.key || !identical(a.spec, b.spec) || !a.single || !b.single) return false;
      final x = a.first, y = b.first;
      if (x.entityId != y.entityId || x.componentId != y.componentId || x.version != y.version) return false;
      if (!setEquals(x.data.keys.toSet(), y.data.keys.toSet())) return false;
      final followed = _fieldValues[a.key] ?? const {};
      for (final k in y.data.keys) {
        if (!deepEquals(x.data[k], y.data[k]) && !followed.containsKey(k)) return false;
      }
    }
    return true;
  }

  /// Every followed field takes its value from the slots shown; the ones
  /// no slot has any more are dropped.
  void _pushFieldValues() {
    if (_fieldValues.isEmpty) return;
    final byKey = {for (final s in _slots) s.key: s};
    _fieldValues.removeWhere((key, _) => byKey[key]?.single != true);
    for (final MapEntry(key: key, value: fields) in _fieldValues.entries) {
      final slot = byKey[key]!;
      for (final MapEntry(key: path, value: n) in fields.entries) {
        final v = slot.value(path);
        if (!deepEquals(n.value, v)) n.value = v;
      }
    }
  }

  @override
  void notifyListeners() {
    if (!_liveOnly) _generation++;
    super.notifyListeners();
  }

  void _onTab() {
    // The tab's last transaction was accepted (its own message is gone): the Inspector's goes too.
    if (_tab?.lastError == null) lastError = null;
    _refresh();
  }

  void _onProject() {
    // Schemas or metadata may have changed on disk (the watcher rescanned).
    _specs.clear();
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
    final pinned = _lockedIds;
    // Pinned entities that were deleted drop out; when none is left the lock lets go.
    if (pinned != null) pinned.retainWhere((id) => t.tree[id] != null);
    if (pinned != null && pinned.isEmpty) _lockedIds = null;
    final ids = List.of(_lockedIds ?? t.selection);
    if (!listEquals(ids, _ids)) {
      lastError = null;
      _fieldValues.clear();
    }
    _ids = ids;
    _typesOn.clear();
    _layerOn.clear();
    _tagsOn.clear();
    if (_ids.isEmpty || tooMany) {
      _entities = const [];
      _slots = const [];
      notifyListeners();
      return;
    }
    // Derived entities are read from their anchor's expansion; the rest
    // from the document in one `entities` query.
    final documentIds = [
      for (final id in _ids)
        if (!(t.tree[id]?.isDerived ?? false)) id,
    ];
    EntityPatch? patch;
    if (documentIds.isNotEmpty) {
      try {
        patch = t.document.entities(documentIds);
      } catch (e) {
        lastError = '$e';
        _entities = const [];
        _slots = const [];
        notifyListeners();
        return;
      }
    }
    final infos = <EntityInfo>[];
    final perEntity = <List<ComponentInstance>>[];
    for (final id in _ids) {
      final node = t.tree[id];
      if (node != null && node.isDerived) {
        final x = t.expansionOf(node.anchor!);
        final de = x?.entity(id);
        if (x == null || de == null) continue; // the expansion changed under us; the tab prunes next
        infos.add(
          EntityInfo(
            id: id,
            name: de.name,
            anchor: node.anchor,
            local: de.local,
            added: de.added,
            active: de.active,
            activeOverridden: de.activeOverridden,
          ),
        );
        final types = _typesOn[id] = <String>{};
        perEntity.add([
          for (final c in de.components.entries)
            ComponentInstance(
              entityId: id,
              componentId: c.key,
              type: c.value.type,
              version: c.value.version,
              data: Map<String, Object?>.from(c.value.data),
              anchor: node.anchor,
              entityLocal: de.local,
              local: c.value.local,
              added: c.value.added,
              overridden: c.value.overridden.toSet(),
            ),
        ]);
        types.addAll(de.components.values.map((c) => c.type));
        continue;
      }
      final e = patch?.entities[id];
      if (e == null) continue; // deleted under us; the tab prunes the selection next
      final p = _prefabOf(e);
      final x = p == null ? null : t.expansionOf(id);
      infos.add(
        EntityInfo(
          id: id,
          name: e['name'] as String? ?? '',
          active: e['active'] != false,
          prefab: p,
          expansion: x,
        ),
      );
      final comps = <ComponentInstance>[];
      final types = _typesOn[id] = <String>{};
      // A prefab instance is its prefab's root (ADR-0022): the root's
      // components as the instance applies them, every edit an override
      // on the instance itself.
      final root = x != null && x.expanded ? x.entity(id) : null;
      if (root != null) {
        for (final c in root.components.entries) {
          comps.add(
            ComponentInstance(
              entityId: id,
              componentId: c.key,
              type: c.value.type,
              version: c.value.version,
              data: Map<String, Object?>.from(c.value.data),
              anchor: id,
              entityLocal: root.local,
              local: c.value.local,
              added: c.value.added,
              overridden: c.value.overridden.toSet(),
            ),
          );
          types.add(c.value.type);
        }
        perEntity.add(comps);
        continue;
      }
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
    for (final c in perEntity.expand((l) => l)) {
      if (c.type == 'fynkel.Layer') _layerOn.putIfAbsent(c.entityId, () => c);
      if (c.type == 'fynkel.Tags') _tagsOn.putIfAbsent(c.entityId, () => c);
    }
    final next = _keepUnchanged(_pair(perEntity));
    if (_moveInPlace(infos, next)) {
      _slots = next;
      _pushFieldValues();
      return;
    }
    _entities = infos;
    _slots = next;
    // Errors of slots that vanished go with them.
    _errors.removeWhere((k, _) => !_slots.any((s) => k.startsWith('${s.key}/')));
    _pushFieldValues();
    notifyListeners();
  }

  /// [next] with each slot that did not change replaced by the one shown
  /// now, so the panel can keep its card.
  List<ComponentSlot> _keepUnchanged(List<ComponentSlot> next) {
    if (_slots.isEmpty || next.isEmpty) return next;
    final before = {for (final s in _slots) s.key: s};
    return [
      for (final s in next)
        if (before[s.key] case final old? when old.sameAs(s)) old else s,
    ];
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
      if (headerTypes.contains(c.type)) continue;
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

  Map<String, TypeSchema> get _schemaMap => _specs.schemas;

  /// The spec of [type]: from its schema and, for a behaviour, the committed
  /// metadata; a type nobody knows gets an empty spec whose data shows as JSON.
  ComponentSpec specFor(String type) => _specs.of(type);

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
      if (!s.allowed || headerTypes.contains(s.type)) continue;
      if (!seen.add(s.type)) continue;
      out.add(AddableType(type: s.type, spec: specFor(s.type)));
    }
    for (final type in _specs.metadata.keys) {
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

  /// The objects a GameObject or script field may take, in Hierarchy
  /// order with their paths: every object, or those carrying the script
  /// [spec] names.
  List<({String id, String name, String path})> objectsFor(FieldSpec spec) {
    final t = _tab;
    if (t == null) return const [];
    final nodes = t.tree.nodes;
    String pathOf(HierarchyNode n) {
      final parts = <String>[];
      for (var p = n.parent == null ? null : nodes[n.parent]; p != null; p = p.parent == null ? null : nodes[p.parent]) {
        parts.insert(0, p.name);
      }
      return parts.join('/');
    }

    final out = <({String id, String name, String path})>[];
    void visit(String id) {
      final n = nodes[id];
      if (n == null) return;
      if (objectRef(spec, id) != null) out.add((id: id, name: n.name, path: pathOf(n)));
      n.children.forEach(visit);
    }

    t.tree.roots.forEach(visit);
    return out;
  }

  /// The value a GameObject or script field takes for the object [entity]
  /// (a picked or dropped one): `{entity}`, or `{entity, component}` of
  /// its script of the field's type — null when it has none.
  Map<String, Object?>? objectRef(FieldSpec spec, String entity) {
    final n = _tab?.tree.nodes[entity];
    if (n == null) return null;
    if (spec.kind != FieldKind.scriptRef) return {'entity': entity};
    for (final (id, type) in n.components) {
      if (type == spec.behaviourType) return {'entity': entity, 'component': id};
    }
    return null;
  }

  /// The name of the object [ref] names; null when it (or, for a script
  /// field, the script on it) is not in the document — the field shows
  /// Missing.
  String? describeObject(Map<String, Object?> ref) {
    final n = _tab?.tree.nodes[ref['entity']];
    if (n == null) return null;
    final component = ref['component'];
    if (component != null && !n.components.any((c) => c.$1 == component)) return null;
    return n.name;
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
    final commands = [for (final i in slot.instances) _setFieldCommand(i, field, value)];
    return _commit(commands, key);
  }

  /// Shows [locals] (entity id → its Transform as the drag has it) in
  /// [tab]'s Scene view without a transaction; null puts the world back to
  /// the document. Set by the shell.
  void Function(OpenDocument tab, Map<String, Trs>? locals, Iterable<String> ids)? previewTransforms;

  /// Shows [data] (entity id → the whole data of a [type] component) in
  /// [tab]'s Scene view without a transaction; null puts the world back
  /// to the document. Set by the shell.
  void Function(OpenDocument tab, String type, Map<String, Map<String, Object?>>? data, Iterable<String> ids)?
  previewComponents;

  /// A live edit, as Unity's: a Transform field's label drag or a colour
  /// being picked. The Scene view follows each instance with [path] set
  /// to `valueFor(i)`; nothing is written until the release (or OK)
  /// commits. A behaviour has nothing to show.
  void previewEach(ComponentSlot slot, String path, Object? Function(ComponentInstance i) valueFor) {
    final t = _tab;
    if (t == null) return;
    final top = _topLevel(path);
    final ids = [for (final i in slot.instances) i.entityId];
    if (slot.type == 'fynkel.Transform') {
      previewTransforms?.call(t, {
        for (final i in slot.instances)
          i.entityId: Trs.fromData({...i.data, top: valueFor(i)}),
      }, ids);
    } else if (slot.type.startsWith('fynkel.')) {
      previewComponents?.call(t, slot.type, {
        for (final i in slot.instances)
          i.entityId: {...i.data, top: _copyJson(valueFor(i))},
      }, ids);
    }
  }

  /// The drag ended: the world back to the document (the commit follows).
  void endPreview(ComponentSlot slot) {
    final t = _tab;
    if (t == null) return;
    final ids = [for (final i in slot.instances) i.entityId];
    if (slot.type == 'fynkel.Transform') {
      previewTransforms?.call(t, null, ids);
    } else if (slot.type.startsWith('fynkel.')) {
      previewComponents?.call(t, slot.type, null, ids);
    }
  }

  /// One field on every instance, each with its own value (one Euler axis
  /// of several objects' rotations), one transaction.
  bool setFieldEach(ComponentSlot slot, String field, Object? Function(ComponentInstance i) valueFor) {
    final key = '${slot.key}/${_topLevel(field)}';
    final commands = [for (final i in slot.instances) _setFieldCommand(i, field, valueFor(i))];
    return _commit(commands, key);
  }

  /// `setField` on a document component; a `setField` override on the
  /// anchor for a derived one (the whole top-level key, folded from the
  /// applied value when [field] is a pointer below it).
  Map<String, Object?> _setFieldCommand(ComponentInstance i, String field, Object? value) {
    if (!i.isDerived) return Command.setField(i.entityId, i.componentId, field, _copyJson(value));
    return Command.setPrefabOverrideOp(
      i.anchor!,
      prefab.setFieldOverride(
        entity: i.entityLocal!,
        component: i.local ?? i.componentId,
        data: i.data,
        path: field,
        value: value,
      ),
    );
  }

  /// Several fields of the slot at once (a whole vector, Euler angles → one quaternion), one transaction.
  bool setFields(ComponentSlot slot, Map<String, Object?> values) {
    final commands = <Map<String, Object?>>[];
    for (final i in slot.instances) {
      // A derived instance's folded keys: later entries see the earlier ones'
      // values when two paths share a top-level key.
      var data = i.data;
      for (final e in values.entries) {
        if (i.isDerived) {
          final op = prefab.setFieldOverride(
            entity: i.entityLocal!,
            component: i.local ?? i.componentId,
            data: data,
            path: e.key,
            value: e.value,
          );
          final field = op['field'] as String;
          // The value it has already, inherited: no override (as Unity).
          if (!i.overridden.contains(field) && _sameValue(data[field], op['value'])) continue;
          data = {...data, field: op['value']};
          commands.add(Command.setPrefabOverrideOp(i.anchor!, op));
        } else {
          commands.add(Command.setField(i.entityId, i.componentId, e.key, _copyJson(e.value)));
        }
      }
    }
    if (commands.isEmpty) return true;
    final key = '${slot.key}/${_topLevel(values.keys.first)}';
    return _commit(commands, key);
  }

  /// JSON equality with numbers by value (`1` is `1.0`).
  static bool _sameValue(Object? a, Object? b) {
    if (a is num && b is num) return a == b;
    if (a is List && b is List) {
      if (a.length != b.length) return false;
      for (var k = 0; k < a.length; k++) {
        if (!_sameValue(a[k], b[k])) return false;
      }
      return true;
    }
    if (a is Map && b is Map) {
      if (a.length != b.length) return false;
      for (final e in a.entries) {
        if (!b.containsKey(e.key) || !_sameValue(e.value, b[e.key])) return false;
      }
      return true;
    }
    return a == b;
  }

  /// Removes [field] from every instance (the schema default applies again).
  bool unsetField(ComponentSlot slot, String field) {
    final key = '${slot.key}/${_topLevel(field)}';
    return _commit([
      for (final i in slot.instances)
        i.isDerived
            ? Command.setPrefabOverrideOp(
                i.anchor!,
                prefab.unsetFieldOverride(entity: i.entityLocal!, component: i.local ?? i.componentId, path: field),
              )
            : Command.unsetField(i.entityId, i.componentId, field),
    ], key);
  }

  // ── Prefab mode ─────────────────────────────────────────────────────────

  /// The override indices, per anchor, that wrote [field] of the slot's
  /// derived instances (empty when the field is inherited everywhere).
  Map<String, List<int>> _fieldOverrides(ComponentSlot slot, String field) {
    final t = _tab;
    final out = <String, List<int>>{};
    if (t == null) return out;
    final key = topLevelKey(field);
    for (final i in slot.instances) {
      if (!i.isDerived) continue;
      final x = t.expansionOf(i.anchor!);
      if (x == null) continue;
      final index = prefab.fieldOverrideIndexOf(
        x,
        entity: i.entityLocal!,
        component: i.local ?? i.componentId,
        field: key,
      );
      if (index != null) (out[i.anchor!] ??= []).add(index);
    }
    return out;
  }

  /// Whether Revert / Apply to prefab is offered on [field] of [slot]: the
  /// field is overridden on a derived instance and the document is editable.
  bool canRevertField(ComponentSlot slot, String field) => canEdit && slot.isOverridden(field);

  /// Why Apply to prefab is not possible for the slot's anchors, or null.
  String? cannotApplyField(ComponentSlot slot, String field) {
    final t = _tab;
    if (t == null) return 'No document.';
    if (!slot.isOverridden(field)) return 'This field is the prefab\'s.';
    for (final i in slot.instances) {
      if (!i.isDerived) continue;
      final x = t.expansionOf(i.anchor!);
      if (x == null) return 'The instance did not expand.';
      final why = prefab.cannotApply(t, x);
      if (why != null) return why;
    }
    return null;
  }

  /// Revert override on one field: drops the override(s) that wrote it on
  /// the slot's derived instances. One transaction.
  bool revertField(ComponentSlot slot, String field) {
    final per = _fieldOverrides(slot, field);
    if (per.isEmpty) return false;
    final commands = <Map<String, Object?>>[];
    for (final e in per.entries) {
      final indices = e.value.toSet().toList()..sort((a, b) => b.compareTo(a));
      commands.addAll([for (final i in indices) Command.revertPrefabOverride(e.key, index: i)]);
    }
    return _commit(commands, '${slot.key}/${_topLevel(field)}');
  }

  /// Apply to prefab on one field: the override(s) that wrote it become the
  /// prefab's value (a transaction on the prefab document), then are
  /// reverted on the instance — per anchor, two undo steps each.
  bool applyField(ComponentSlot slot, String field, {int depth = 0}) {
    final t = _tab;
    final per = _fieldOverrides(slot, field);
    if (t == null || per.isEmpty) return false;
    final key = '${slot.key}/${_topLevel(field)}';
    for (final e in per.entries) {
      final outcome = prefab.applyOverrides(tabs, t, e.key, indices: e.value, depth: depth);
      if (!outcome.ok) {
        _errors[key] = outcome.error ?? 'Apply to prefab failed.';
        lastError = outcome.error;
        notifyListeners();
        return false;
      }
    }
    _errors.remove(key);
    notifyListeners();
    return true;
  }

  /// The expansion of the selected anchor [entityId] (fetched now when needed).
  PrefabExpansion? expansionOf(String entityId) => _tab?.expansionOf(entityId);

  /// Why Apply to prefab is not possible for the anchor [entityId], or null.
  String? cannotApply(String entityId) {
    final t = _tab;
    final x = t?.expansionOf(entityId);
    if (t == null || x == null) return 'Not a prefab instance.';
    return prefab.cannotApply(t, x);
  }

  /// Why Unpack is not possible for the anchor [entityId], or null.
  String? cannotUnpack(String entityId) {
    final t = _tab;
    final x = t?.expansionOf(entityId);
    if (t == null || x == null) return 'Not a prefab instance.';
    return prefab.cannotUnpack(t, x);
  }

  /// Apply to prefab: every override of the instance [entityId] (or the one
  /// at [index]) is written to the prefab document — its tab opens behind
  /// this one — then reverted here. Two undo steps, one per document.
  bool applyOverrides(String entityId, {int? index, int depth = 0}) {
    final t = _tab;
    if (t == null || !canEdit) return false;
    final outcome = prefab.applyOverrides(tabs, t, entityId, indices: index == null ? null : [index], depth: depth);
    lastError = outcome.ok ? null : outcome.error;
    notifyListeners();
    return outcome.ok;
  }

  /// Where Apply can write the overrides of the instance [entityId]: its
  /// prefab, then (a variant's) the bases, nearest first.
  List<prefab.ApplyTarget> applyTargets(String entityId) {
    final x = _tab?.expansionOf(entityId);
    return x == null ? const [] : prefab.applyTargets(tabs, x);
  }

  /// Unpack: the instance [entityId] becomes plain entities, one transaction
  /// — one level (nested instances stay), or [completely].
  bool unpack(String entityId, {bool completely = false}) {
    final t = _tab;
    if (t == null || !canEdit) return false;
    final ok = prefab.unpackInstance(t, entityId, completely: completely);
    lastError = ok ? null : t.lastError;
    notifyListeners();
    return ok;
  }

  /// Open prefab: the instance's prefab document (an imported hierarchy
  /// read-only) in its own tab, focused, in the context of the instance
  /// (Unity's Prefab Mode in Context).
  bool openPrefab(String entityId) {
    final from = _tab;
    final x = from?.expansionOf(entityId);
    if (from == null || x == null) return false;
    final tab = tabs.openDocument(x.open, context: PrefabContext(from, x.anchor));
    if (tab == null) {
      lastError = tabs.openError;
      notifyListeners();
      return false;
    }
    return true;
  }

  /// Select prefab asset: reveals the prefab (or the model source) in the
  /// Project browser.
  bool selectPrefabAsset(String entityId) {
    final reveal = revealAsset;
    final x = _tab?.expansionOf(entityId);
    final e = _entities.where((e) => e.id == entityId).firstOrNull;
    final asset = x?.asset ?? e?.prefab?.assetId;
    if (reveal == null || asset == null) return false;
    reveal(asset, x == null ? e?.prefab?.sub : (x.sub.isEmpty ? null : x.sub));
    return true;
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
    final commands = [for (final e in _entities) ?_addCommand(e, type, spec.version ?? 1, data)];
    final a = _run(commands);
    notifyListeners();
    return a?.ok ?? false;
  }

  /// Adds a [type] component with [data] to [e]: an `addComponent`, or an
  /// `addComponent` override on a derived entity's anchor (null when its
  /// expansion is gone).
  Map<String, Object?>? _addCommand(EntityInfo e, String type, int version, Map<String, Object?> data) {
    // An instance is its prefab's root: an override on itself (ADR-0022).
    final anchor = e.anchor ?? (e.prefab != null ? e.id : null);
    if (anchor == null) return Command.addComponent(e.id, {'type': type, 'version': version, 'data': _copyJson(data)});
    final local = _tab!.expansionOf(anchor)?.entity(e.id)?.local;
    if (local == null) return null;
    return Command.setPrefabOverrideOp(
      anchor,
      prefab.addComponentOverride(entity: local, type: type, version: version, data: data),
    );
  }

  // ── The object header: layer and tags (ADR-0020) ────────────────────────

  /// The project's layer names (index `i` = layer `i`).
  List<String> get layerNames => project.layerNames();

  /// The project's tag names (index `i` = bit `i` of a tag set).
  List<String> get tagNames => project.tagNames();

  /// [entityId]'s layer index (0 without a `fynkel.Layer`).
  int layerOf(String entityId) => (_layerOn[entityId]?.data['layer'] as num?)?.toInt() ?? 0;

  /// [entityId]'s tag bits (0 without a `fynkel.Tags`).
  int tagBitsOf(String entityId) => (_tagsOn[entityId]?.data['bits'] as num?)?.toInt() ?? 0;

  /// The selection's layer index, or null when the entities disagree.
  int? get layer {
    if (_entities.isEmpty) return null;
    final first = layerOf(_entities.first.id);
    return _entities.every((e) => layerOf(e.id) == first) ? first : null;
  }

  /// Whether the selected entities carry tag [index].
  TagState tagState(int index) {
    final bit = 1 << index;
    final n = _entities.where((e) => tagBitsOf(e.id) & bit != 0).length;
    return n == 0 ? TagState.none : (n == _entities.length ? TagState.all : TagState.mixed);
  }

  /// The tag indices any selected entity carries, ascending (a bit past the
  /// project's list included, so it can be seen and taken off).
  List<int> get tagsInUse {
    final any = _entities.fold(0, (bits, e) => bits | tagBitsOf(e.id));
    return [
      for (var i = 0; i < 32; i++)
        if (any & (1 << i) != 0) i,
    ];
  }

  /// Puts every selected entity on layer [index], one transaction: a
  /// `setField` of its `fynkel.Layer`, or the component added when it has
  /// none (and [index] is not the default).
  bool setLayer(int index) {
    if (!canEdit) return false;
    final layers = layerNames;
    if (index < 0 || index >= layers.length) {
      return _refuseHeader('FYN-H0012: layer $index is not declared: the project\'s layers are [${layers.join(', ')}]');
    }
    final commands = <Map<String, Object?>>[];
    for (final e in _entities) {
      final i = _layerOn[e.id];
      if (i != null) {
        if (layerOf(e.id) != index) commands.add(_setFieldCommand(i, 'layer', index));
      } else if (index != 0) {
        final command = _addCommand(e, 'fynkel.Layer', specFor('fynkel.Layer').version ?? 1, {'layer': index});
        if (command != null) commands.add(command);
      }
    }
    return _commitHeader(commands);
  }

  /// Gives every selected entity tag [index] ([on]) or takes it off, one
  /// transaction; the others' tags stay.
  bool setTag(int index, bool on) {
    if (!canEdit) return false;
    final tags = tagNames;
    if (on && (index < 0 || index >= tags.length)) {
      return _refuseHeader('FYN-H0012: tag $index is not declared: the project\'s tags are [${tags.join(', ')}]');
    }
    final bit = 1 << index;
    final commands = <Map<String, Object?>>[];
    for (final e in _entities) {
      final bits = tagBitsOf(e.id);
      final next = on ? bits | bit : bits & ~bit;
      if (next == bits) continue;
      final i = _tagsOn[e.id];
      if (i != null) {
        commands.add(_setFieldCommand(i, 'bits', next));
      } else {
        final command = _addCommand(e, 'fynkel.Tags', specFor('fynkel.Tags').version ?? 1, {'bits': next});
        if (command != null) commands.add(command);
      }
    }
    return _commitHeader(commands);
  }

  /// A header value the project does not declare: refused before any
  /// command, as the crate does not check the project's lists at apply
  /// (`fynkel validate` does).
  bool _refuseHeader(String message) {
    lastError = message;
    notifyListeners();
    return false;
  }

  bool _commitHeader(List<Map<String, Object?>> commands) {
    if (commands.isEmpty) return true;
    final a = _run(commands);
    if (a != null && !a.ok) lastError = describeRejection(a);
    notifyListeners();
    return a?.ok ?? false;
  }

  /// Removes the slot's component from every selected entity. One
  /// transaction. On a derived instance: the `addComponent` override that
  /// added it is reverted, an added object's `addObject` is re-issued
  /// without it, an inherited component gets a `removeComponent` override.
  bool removeComponent(ComponentSlot slot) {
    final t = _tab;
    if (t == null) return false;
    if (slot.type == transformType) {
      lastError = transformStays;
      notifyListeners();
      return false;
    }
    final commands = <Map<String, Object?>>[];
    final reverts = <String, List<int>>{};
    final ops = <Map<String, Object?>>[];
    for (final i in slot.instances) {
      if (!i.isDerived) {
        commands.add(Command.removeComponent(i.entityId, i.componentId));
        continue;
      }
      final x = t.expansionOf(i.anchor!);
      final de = x?.entity(i.entityId);
      if (x == null || de == null) continue;
      final component = i.local ?? i.componentId;
      if (de.added) {
        ops.add(Command.setPrefabOverrideOp(i.anchor!, prefab.reissuedAddObjectOverride(x, de, without: component)));
      } else if (i.added) {
        final index = prefab.addComponentIndexOf(x, entity: de.local, component: component);
        if (index != null) (reverts[i.anchor!] ??= []).add(index);
      } else {
        ops.add(
          Command.setPrefabOverrideOp(
            i.anchor!,
            prefab.removeComponentOverride(entity: de.local, component: component),
          ),
        );
      }
    }
    for (final e in reverts.entries) {
      final indices = e.value.toSet().toList()..sort((a, b) => b.compareTo(a));
      commands.addAll([for (final i in indices) Command.revertPrefabOverride(e.key, index: i)]);
    }
    commands.addAll(ops);
    final a = _run(commands);
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
        if (!deepEquals(i.data[e.key], e.value)) commands.add(_setFieldCommand(i, e.key, e.value));
      }
    }
    if (commands.isEmpty) return true;
    final a = _run(commands);
    if (a?.ok ?? false) _errors.removeWhere((k, _) => k.startsWith('${slot.key}/'));
    notifyListeners();
    return a?.ok ?? false;
  }

  // ── Copy / Paste Component Values (Unity) ───────────────────────────────

  /// The values last copied from a component, for Paste Component Values
  /// on any component of that type, in any document.
  static ({String type, Map<String, Object?> data})? componentClipboard;

  void copyValues(ComponentSlot slot) {
    componentClipboard = (type: slot.type, data: Map<String, Object?>.from(_copyJson(slot.first.data) as Map));
    notifyListeners();
  }

  bool canPasteValues(ComponentSlot slot) => canEdit && componentClipboard?.type == slot.type;

  /// Every selected instance takes the copied values — a key the copy does
  /// not hold goes back to its default — in one transaction.
  bool pasteValues(ComponentSlot slot) {
    final c = componentClipboard;
    if (c == null || !canPasteValues(slot)) return false;
    final commands = <Map<String, Object?>>[];
    for (final i in slot.instances) {
      for (final e in c.data.entries) {
        if (!deepEquals(i.data[e.key], e.value)) commands.add(_setFieldCommand(i, e.key, e.value));
      }
      if (i.isDerived) continue;
      for (final k in i.data.keys) {
        if (!c.data.containsKey(k)) commands.add(Command.unsetField(i.entityId, i.componentId, k));
      }
    }
    if (commands.isEmpty) return true;
    final a = _run(commands);
    if (a?.ok ?? false) _errors.removeWhere((k, _) => k.startsWith('${slot.key}/'));
    notifyListeners();
    return a?.ok ?? false;
  }

  // ── Move Up / Move Down (Unity) ─────────────────────────────────────────

  /// The card [delta] places away ([slot]'s neighbour: -1 above, +1
  /// below) that [slot] would trade places with, or null when it cannot
  /// move: one object selected, its own components (not a prefab's), not
  /// while the game runs, and the Transform stays first, as in Unity.
  ComponentSlot? _moveTarget(ComponentSlot slot, int delta) {
    if (!canEdit || showsPlay || !slot.single || slot.anyDerived || slot.type == 'fynkel.Transform') return null;
    final i = _slots.indexOf(slot);
    final j = i + delta;
    if (i < 0 || j < 0 || j >= _slots.length) return null;
    final other = _slots[j];
    return other.type == 'fynkel.Transform' || other.anyDerived ? null : other;
  }

  bool canMove(ComponentSlot slot, int delta) => _moveTarget(slot, delta) != null;

  /// Moves [slot] one card up (`-1`) or down (`+1`): a `moveComponent` to
  /// its neighbour's place in the entity's components — one undo step.
  bool moveComponent(ComponentSlot slot, int delta) {
    final other = _moveTarget(slot, delta);
    final t = _tab;
    if (other == null || t == null) return false;
    final entity = slot.first.entityId;
    final components = t.document.entities([entity]).entities[entity]?['components'];
    if (components is! Map) return false;
    final index = components.keys.toList().indexOf(other.first.componentId);
    if (index < 0) return false;
    final a = _run([Command.moveComponent(entity, slot.first.componentId, index)]);
    notifyListeners();
    return a?.ok ?? false;
  }

  // ── Apply to Scene (ADR-0021 P7 L4) ─────────────────────────────────────

  /// Whether the Inspector shows the running game (Apply to Scene is
  /// offered then).
  bool get showsPlay => _tab?.document is PlayDocument;

  /// The open tab of the scene the game was started from, while the
  /// Inspector shows the running game.
  OpenDocument? get playScene {
    final doc = _tab?.document;
    return doc is PlayDocument ? playSceneTab(tabs, doc) : null;
  }

  /// Why the slot's running values cannot be applied to the scene, or null.
  String? cannotApplyToScene(ComponentSlot slot) {
    final scene = playScene;
    if (scene == null) return 'The scene the game was started from is not open.';
    if (scene.document.readOnly != null) return 'The scene is read-only.';
    for (final i in slot.instances) {
      if (sceneNodeOf(scene, i.entityId) == null) return 'The game made this object; it is not in the scene.';
    }
    return null;
  }

  /// Apply to Scene: the slot's running values (all of them, or only
  /// [field]'s top-level key) written into the scene the game was started
  /// from — one transaction on its tab, which goes dirty. Only what
  /// differs from the scene is written.
  bool applyToScene(ComponentSlot slot, {String? field}) {
    final scene = playScene;
    final errorKey = field == null ? null : '${slot.key}/${_topLevel(field)}';
    String? why = cannotApplyToScene(slot);
    final commands = <Map<String, Object?>>[];
    if (why == null) {
      final defaults = (slot.spec.isBehaviour ? slot.spec.schemaDefaults() : project.componentDefaults(slot.type)) ?? {};
      try {
        for (final i in slot.instances) {
          commands.addAll(
            applyToSceneCommands(
              scene!,
              (entity: i.entityId, type: slot.type, version: i.version, index: slot.index, data: i.data),
              defaults: defaults,
              field: field,
            ),
          );
        }
      } on StateError catch (e) {
        why = e.message;
      }
    }
    if (why == null && commands.isEmpty) why = 'Nothing to apply: the scene has these values.';
    if (why == null) {
      final a = scene!.transaction(commands);
      if (a == null || !a.ok) why = scene.lastError ?? 'The scene refused it.';
    }
    // A field's problem shows under the field, a component's above the cards.
    if (errorKey == null) {
      lastError = why;
    } else if (why == null) {
      _errors.remove(errorKey);
    } else {
      _errors[errorKey] = why;
    }
    notifyListeners();
    return why == null;
  }

  /// Renames the single selected entity. A derived entity is named by its
  /// prefab — only an object an override added is renamed (its `addObject`
  /// re-issued).
  bool rename(String name) {
    final e = single;
    if (e == null || name.trim().isEmpty || name == e.name) return false;
    if (e.isDerived) {
      final x = _tab?.expansionOf(e.anchor!);
      final de = x?.entity(e.id);
      if (x == null || de == null || !de.added) {
        lastError = 'A prefab instance\'s entity is named by the prefab — rename it there (Open prefab).';
        notifyListeners();
        return false;
      }
      final a = _run([
        Command.setPrefabOverrideOp(e.anchor!, prefab.reissuedAddObjectOverride(x, de, name: name.trim())),
      ]);
      notifyListeners();
      return a?.ok ?? false;
    }
    final a = _run([Command.renameObject(e.id, name.trim())]);
    notifyListeners();
    return a?.ok ?? false;
  }

  /// The selection's own active flag (the header's checkbox); null when
  /// the selected entities disagree.
  bool? get active {
    if (_entities.isEmpty) return null;
    final first = _entities.first.active;
    return _entities.every((e) => e.active == first) ? first : null;
  }

  /// A `setActive` override set the flag of a selected derived entity.
  bool get activeOverridden => _entities.any((e) => e.activeOverridden);

  /// Switches the selected entities on or off, one undo step: the
  /// document's `active` for its own entities, a `setActive` override on
  /// the anchor for a prefab instance's (docs/design/set-active.md § 5).
  bool setActive(bool value) {
    final commands = [
      for (final e in _entities)
        if (e.active != value)
          e.isDerived
              ? Command.setPrefabOverrideOp(e.anchor!, {'op': 'setActive', 'entity': e.local, 'value': value})
              : Command.setObjectActive(e.id, value),
    ];
    final a = _run(commands);
    notifyListeners();
    return a?.ok ?? false;
  }

  /// Revert override on the checkbox: drops the `setActive` overrides of
  /// the selected derived entities.
  bool revertActive() {
    final t = _tab;
    if (t == null) return false;
    final per = <String, List<int>>{};
    for (final e in _entities) {
      if (!e.activeOverridden) continue;
      for (final o in t.expansionOf(e.anchor!)?.overrides ?? const <ExpansionOverride>[]) {
        if (o.op == 'setActive' && o.entityId == e.id) (per[e.anchor!] ??= []).add(o.index);
      }
    }
    final commands = [
      for (final e in per.entries)
        for (final i in (e.value..sort((a, b) => b.compareTo(a)))) Command.revertPrefabOverride(e.key, index: i),
    ];
    final a = _run(commands);
    notifyListeners();
    return a?.ok ?? false;
  }

  /// Whether the single selected entity's name can be edited.
  bool get canRename {
    final e = single;
    return canEdit && e != null && (!e.isDerived || e.added);
  }

  /// The prefab's own value that the override [o] of the instance
  /// [entityId] replaces — the Overrides dropdown's comparison — read from
  /// the prefab's file. `(false, null)` when the prefab has none there (an
  /// imported hierarchy, a nested instance's object, a field left to its
  /// default) or [o] is not a field.
  (bool, Object?) prefabValueOf(String entityId, ExpansionOverride o) {
    final info = expansionOf(entityId)?.prefab;
    final path = info?.path;
    if (info == null || info.kind != 'prefab' || path == null || o.component == null || o.field == null) {
      return (false, null);
    }
    try {
      final json = jsonDecode(File(project.assetFile(path)).readAsStringSync());
      final data = json['entities'][o.entity]['components'][o.component]['data'];
      if (data is! Map || !data.containsKey(o.field)) return (false, null);
      return (true, data[o.field]);
    } catch (_) {
      return (false, null);
    }
  }

  /// The instance and override index that added [slot]'s component (an
  /// `addComponent` override on a prefab instance), or null.
  (String, int)? addedComponentOverride(ComponentSlot slot) {
    if (!slot.single) return null;
    final i = slot.first;
    if (!i.isDerived || !i.added || i.entityLocal == null || i.local == null) return null;
    final x = expansionOf(i.anchor!);
    final index = x == null ? null : prefab.addComponentIndexOf(x, entity: i.entityLocal!, component: i.local!);
    return index == null ? null : (i.anchor!, index);
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

  /// Opens the behaviour's source in the IDE at its class; answers the
  /// error or null.
  Future<String?> openScript(ComponentSlot slot) async {
    final file = scriptPath(slot);
    if (file == null) {
      return '${slot.type} has no source file in this project (${slot.spec.library ?? 'no library in the metadata'})';
    }
    final open = openInIde;
    if (open == null) return 'no IDE command is configured';
    return open(file, line: classLine(file, slot.spec.className));
  }

  /// The 1-based line that declares `class <name>` in [file], or null.
  static int? classLine(String file, String? name) {
    if (name == null) return null;
    final f = File(file);
    if (!f.existsSync()) return null;
    final declares = RegExp('^\\s*(?:[a-z]+\\s+)*class\\s+${RegExp.escape(name)}\\b');
    final lines = f.readAsLinesSync();
    for (var i = 0; i < lines.length; i++) {
      if (declares.hasMatch(lines[i])) return i + 1;
    }
    return null;
  }

  static String _topLevel(String field) => topLevelKey(field);

  @override
  void dispose() {
    tabs.removeListener(_onTabs);
    project.removeListener(_onProject);
    codegen?.removeListener(notifyListeners);
    _tab?.removeListener(_onTab);
    _liveValues?.removeListener(_onLiveValues);
    super.dispose();
  }
}

/// `committedDrift` under a name the model's getter does not shadow.
List<CommittedDrift> committedDrift_(Map<String, Object?>? metadata, Map<String, Map<String, Object?>?> files) =>
    committedDrift(metadata, files);

// ── Value helpers ──────────────────────────────────────────────────────────

/// The top-level key of a field path (`/position/1` → `position`).
String topLevelKey(String field) => field.startsWith('/') ? field.substring(1).split('/').first : field;

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
