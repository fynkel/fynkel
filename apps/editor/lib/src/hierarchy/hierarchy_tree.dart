// The Hierarchy's model of one document: a node per entity (`name`,
// `parent`, `children`, the component types, the prefab instance, the
// transform for "keep world transform"), the ordered roots, and what is
// expanded. Built once from the whole document; afterwards patched from a
// transaction's `affectedIds` only, so a 10 000-entity scene never rebuilds
// its nodes for one rename. The flat list of visible rows (what the
// virtualised list shows) is computed lazily and cached.
//
// Prefab mode (item 7): a prefab instance's anchor shows its expansion —
// the prefab's entities under the derived ids the runtime uses — as
// *derived* nodes below its authored children. They are fetched lazily
// through [HierarchyTree.expansionProvider] (the session's `instance`
// query) when an anchor is first opened or one of its derived ids is asked
// for, cached per anchor, and dropped when the anchor changes, the
// document is rebuilt or a prefab document edits (`invalidateExpansions`).
import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import 'transform_math.dart';

final class HierarchyNode {
  HierarchyNode(this.id);

  final String id;
  String name = '';
  String? parent;
  List<String> children = const [];
  List<String> componentTypes = const [];

  /// The entity's own active flag (`"active": false` in the document, a
  /// derived entity's with the overrides applied); see
  /// [HierarchyTree.activeInHierarchy] for what the row shows.
  bool active = true;

  /// The `fynkel.Transform` component's id and data, when the entity has one.
  String? transformId;

  /// A derived entity's Transform under its prefab-local component id —
  /// what a `setField` override on the anchor addresses (the gizmo's
  /// drag of a derived entity, M4-07). Null for a document entity.
  String? transformLocal;
  Trs? transform;

  /// Set for the root entity of a prefab instance.
  bool isPrefabInstance = false;
  String? prefabAsset;
  int overrideCount = 0;

  /// Prefab mode: the anchor whose expansion this derived node belongs to
  /// (null for an entity of the document), the prefab-local id overrides
  /// address, whether an `addObject` override added it, and how many of
  /// its fields overrides touch.
  String? anchor;
  String? local;
  bool added = false;
  int overriddenFields = 0;

  bool get isDerived => anchor != null;

  /// Bumped on every patch; a row widget keyed on it repaints only then.
  int version = 0;

  void read(Map<String, Object?> e) {
    name = e['name'] as String? ?? '';
    active = e['active'] != false;
    parent = e['parent'] as String?;
    children = [
      for (final c in e['children'] as List? ?? const [])
        if (c is String) c,
    ];
    final types = <String>[];
    transformId = null;
    transform = null;
    final components = e['components'];
    if (components is Map) {
      for (final entry in components.entries) {
        final c = entry.value;
        if (c is! Map) continue;
        final type = c['type'] as String? ?? '';
        types.add(type);
        if (type == 'fynkel.Transform' && transformId == null) {
          transformId = entry.key as String;
          final data = c['data'];
          transform = Trs.fromData(data is Map<String, Object?> ? data : null);
        }
      }
    }
    componentTypes = types;
    final prefab = e['prefab'];
    isPrefabInstance = prefab is Map;
    if (prefab is Map) {
      final asset = prefab['asset'];
      prefabAsset = asset is Map ? asset['asset'] as String? : asset as String?;
      overrideCount = (prefab['overrides'] as List?)?.length ?? 0;
    } else {
      prefabAsset = null;
      overrideCount = 0;
    }
    version++;
  }

  /// Reads a derived entity of [anchor]'s expansion: the applied
  /// components (so the icons, the filter and the world matrix are right);
  /// `transformId` is the derived component id and `transformLocal` the
  /// prefab's — the gizmo's drag of a derived entity becomes a `setField`
  /// override on the anchor, never a write to the prefab.
  void readExpanded(ExpandedEntity e, {required String anchor}) {
    name = e.name;
    active = e.active;
    parent = e.parent;
    children = List.of(e.children);
    this.anchor = anchor;
    local = e.local;
    added = e.added;
    transformId = null;
    transformLocal = null;
    transform = null;
    final types = <String>[];
    var overridden = e.activeOverridden ? 1 : 0;
    for (final c in e.components.values) {
      types.add(c.type);
      overridden += c.overridden.length;
      if (c.type == 'fynkel.Transform' && transformId == null) {
        transformId = c.id;
        transformLocal = c.local ?? c.id;
        transform = Trs.fromData(c.data);
      }
    }
    componentTypes = types;
    overriddenFields = overridden;
    isPrefabInstance = false;
    prefabAsset = null;
    overrideCount = 0;
    version++;
  }

  /// `Transform` counts as "a component" for the filter, but is on nearly
  /// everything, so a search on it is rarely meant.
  bool hasComponent(String type) => componentTypes.contains(type);

  @override
  String toString() => 'HierarchyNode($id "$name")';
}

/// One visible row: a node at a depth, with what the expander needs.
final class HierarchyRow {
  const HierarchyRow(
    this.node, {
    required this.depth,
    required this.hasChildren,
    required this.expanded,
    required this.matches,
  });

  final HierarchyNode node;
  final int depth;
  final bool hasChildren;
  final bool expanded;

  /// False for an ancestor shown only because a descendant matches the filter.
  final bool matches;

  String get id => node.id;
}

enum PrefabFilter { any, instances, plain }

/// What the toolbar filters on; `any`/empty means "everything".
final class HierarchyFilter {
  const HierarchyFilter({
    this.query = '',
    this.componentTypes = const {},
    this.prefab = PrefabFilter.any,
    this.hasError = false,
  });

  final String query;
  final Set<String> componentTypes;
  final PrefabFilter prefab;
  final bool hasError;

  bool get isEmpty => query.trim().isEmpty && componentTypes.isEmpty && prefab == PrefabFilter.any && !hasError;

  HierarchyFilter copyWith({String? query, Set<String>? componentTypes, PrefabFilter? prefab, bool? hasError}) =>
      HierarchyFilter(
        query: query ?? this.query,
        componentTypes: componentTypes ?? this.componentTypes,
        prefab: prefab ?? this.prefab,
        hasError: hasError ?? this.hasError,
      );

  bool matches(HierarchyNode n, {required bool hasError}) {
    final q = query.trim().toLowerCase();
    if (q.isNotEmpty) {
      final words = q.split(RegExp(r'\s+'));
      final hay = '${n.name.toLowerCase()} ${n.id} ${n.componentTypes.join(' ').toLowerCase()}';
      if (!words.every(hay.contains)) return false;
    }
    if (componentTypes.isNotEmpty && !componentTypes.every(n.hasComponent)) return false;
    switch (prefab) {
      case PrefabFilter.any:
        break;
      case PrefabFilter.instances:
        if (!n.isPrefabInstance) return false;
      case PrefabFilter.plain:
        if (n.isPrefabInstance) return false;
    }
    if (this.hasError && !hasError) return false;
    return true;
  }

  @override
  bool operator ==(Object other) =>
      other is HierarchyFilter &&
      other.query == query &&
      setEquals(other.componentTypes, componentTypes) &&
      other.prefab == prefab &&
      other.hasError == hasError;

  @override
  int get hashCode => Object.hash(query, Object.hashAllUnordered(componentTypes), prefab, hasError);
}

final class HierarchyTree extends ChangeNotifier {
  final nodes = <String, HierarchyNode>{};
  List<String> roots = const [];
  final expanded = <String>{};

  /// Entity ids the document's diagnostics point at → the diagnostics.
  Map<String, List<Diagnostic>> errors = const {};

  /// Counters the tests read: whole builds and incremental patches.
  int builds = 0;
  int patches = 0;

  /// Prefab mode: answers an anchor's expansion (the `instance` query), or
  /// null when it cannot be asked; set by the tab. Called lazily — when an
  /// anchor is opened, or a derived id it owns is looked up.
  PrefabExpansion? Function(String anchor)? expansionProvider;

  /// The expansions fetched so far, by anchor (an entry may be an
  /// unexpanded one: `expanded` false with its diagnostics).
  final expansions = <String, PrefabExpansion>{};

  /// Anchors asked and answered null (no provider, a query failure) — not
  /// asked again until invalidated.
  final _unanswered = <String>{};

  /// Derived id → its anchor, for every derived id ever fetched here; kept
  /// through invalidation because the ids are deterministic, so a selected
  /// derived entity survives its anchor's refetch.
  final _anchorOfDerived = <String, String>{};

  /// Counter the tests read: `instance` queries asked.
  int expansionFetches = 0;

  HierarchyFilter _filter = const HierarchyFilter();
  HierarchyFilter get filter => _filter;
  List<HierarchyRow>? _rows;
  Set<String>? _matching;

  bool get isEmpty => entityCount == 0;

  /// The document's own entities (derived nodes are not counted).
  int get entityCount => _documentEntities;
  int _documentEntities = 0;

  /// The node of [id]; a derived id whose anchor's expansion is not
  /// fetched yet (or was dropped) fetches it first.
  HierarchyNode? operator [](String id) {
    final n = nodes[id];
    if (n != null) return n;
    final anchor = _anchorOfDerived[id];
    if (anchor != null && nodes[anchor]?.isPrefabInstance == true) {
      expansionOf(anchor);
      return nodes[id];
    }
    return null;
  }

  bool contains(String id) => this[id] != null;

  /// [id] and every ancestor active (Unity's `activeInHierarchy`): a row
  /// under an inactive parent is greyed like the parent.
  bool activeInHierarchy(String id) {
    for (String? at = id; at != null;) {
      final n = nodes[at];
      if (n == null) return true;
      if (!n.active) return false;
      at = n.parent;
    }
    return true;
  }

  /// Reads the whole document (`entities` by id, `roots` when the kind has them).
  void build(Map<String, Object?> root, {List<String>? roots}) {
    nodes.clear();
    final entities = root['entities'];
    if (entities is Map) {
      for (final e in entities.entries) {
        if (e.value is Map<String, Object?>) {
          nodes[e.key as String] = HierarchyNode(e.key as String)..read(e.value as Map<String, Object?>);
        }
      }
    }
    this.roots =
        roots ??
        [
          for (final r in root['roots'] as List? ?? const [])
            if (r is String && nodes.containsKey(r)) r,
        ];
    if (this.roots.isEmpty && root['roots'] is! List) {
      this.roots = [
        for (final n in nodes.values)
          if (n.parent == null) n.id,
      ];
    }
    _documentEntities = nodes.length;
    expanded.removeWhere((id) => !nodes.containsKey(id) && !_anchorOfDerived.containsKey(id));
    expansions.clear();
    _unanswered.clear();
    builds++;
    _invalidate();
  }

  /// Applies what a transaction touched: every named entity is re-read
  /// (dropped when `null`), the roots are replaced. Untouched nodes keep
  /// their identity and version.
  void patch(EntityPatch p) {
    for (final e in p.entities.entries) {
      final v = e.value;
      final before = nodes[e.key];
      if (before != null && !before.isDerived) _documentEntities--;
      if (before?.isPrefabInstance ?? false) invalidateExpansion(e.key);
      if (v == null) {
        nodes.remove(e.key);
        expanded.remove(e.key);
      } else {
        (nodes[e.key] ??= HierarchyNode(e.key)).read(v);
        _documentEntities++;
      }
    }
    roots = [
      for (final r in p.roots)
        if (nodes.containsKey(r)) r,
    ];
    patches++;
    _invalidate();
  }

  // ── Prefab mode: expansions ───────────────────────────────────────────

  /// The expansion of [anchor], fetched now when it is not cached; null
  /// when [anchor] is not a prefab instance or the provider cannot answer.
  PrefabExpansion? expansionOf(String anchor) {
    final cached = expansions[anchor];
    if (cached != null) return cached;
    final n = nodes[anchor];
    if (n == null || !n.isPrefabInstance || _unanswered.contains(anchor)) return null;
    final provider = expansionProvider;
    if (provider == null) return null;
    expansionFetches++;
    final x = provider(anchor);
    if (x == null) {
      _unanswered.add(anchor);
      return null;
    }
    expansions[anchor] = x;
    for (final e in x.entities) {
      _anchorOfDerived[e.id] = anchor;
      (nodes[e.id] ??= HierarchyNode(e.id)).readExpanded(e, anchor: anchor);
    }
    _rows = null;
    _matching = null;
    return x;
  }

  /// The instance roots the expansion of [anchor] adds below its authored
  /// children (empty for a plain entity or an unexpanded instance).
  List<String> instanceRootsOf(String anchor) {
    final x = expansionOf(anchor);
    if (x == null) return const [];
    return [
      for (final e in x.entities)
        if (e.parent == anchor) e.id,
    ];
  }

  /// The children a row opens to: the document's, then the instance's roots.
  List<String> childrenOf(String id) {
    final n = nodes[id];
    if (n == null) return const [];
    if (!n.isPrefabInstance) return n.children;
    final derived = instanceRootsOf(id);
    return derived.isEmpty ? n.children : [...n.children, ...derived];
  }

  /// Drops the cached expansion of [anchor] and its derived nodes; the next
  /// look re-asks the provider.
  void invalidateExpansion(String anchor) {
    final x = expansions.remove(anchor);
    _unanswered.remove(anchor);
    if (x == null) return;
    for (final e in x.entities) {
      if (nodes[e.id]?.anchor == anchor) nodes.remove(e.id);
    }
    _rows = null;
    _matching = null;
  }

  /// Drops every cached expansion (a prefab document changed, a reload).
  void invalidateExpansions() {
    for (final a in expansions.keys.toList()) {
      invalidateExpansion(a);
    }
    _unanswered.clear();
    _invalidate();
  }

  /// Re-fetches the expansions that were cached (so an open instance stays
  /// open with fresh derived nodes) and notifies.
  void refreshExpansions() {
    final anchors = expansions.keys.toList();
    invalidateExpansions();
    for (final a in anchors) {
      expansionOf(a);
    }
    _invalidate();
  }

  /// The anchor a derived [id] belongs to, or null for a document entity.
  String? anchorOf(String id) => nodes[id]?.anchor ?? _anchorOfDerived[id];

  /// Makes [id] a known node when it is a derived entity of an instance
  /// whose expansion was not fetched yet (a Console diagnostic names an
  /// expanded entity): asks each unexpanded instance in turn. False when no
  /// node and no expansion holds it.
  bool locate(String id) {
    if (nodes[id] != null) return true;
    for (final n in nodes.values.toList()) {
      if (!n.isPrefabInstance || expansions.containsKey(n.id)) continue;
      expansionOf(n.id);
      if (nodes[id] != null) return true;
    }
    return false;
  }

  void setErrors(Map<String, List<Diagnostic>> e) {
    errors = e;
    _invalidate();
  }

  void setFilter(HierarchyFilter f) {
    if (f == _filter) return;
    _filter = f;
    _invalidate();
  }

  bool hasError(String id) => errors[id]?.isNotEmpty ?? false;

  // ── Shape ─────────────────────────────────────────────────────────────

  int depthOf(String id) {
    var d = 0;
    var cur = nodes[id]?.parent;
    while (cur != null && d < 100000) {
      d++;
      cur = nodes[cur]?.parent;
    }
    return d;
  }

  /// [ancestor] is [id] itself or above it.
  bool isAncestorOrSelf(String ancestor, String id) {
    String? cur = id;
    var hops = 0;
    while (cur != null && hops++ < 100000) {
      if (cur == ancestor) return true;
      cur = nodes[cur]?.parent;
    }
    return false;
  }

  /// The ids from the root down to (not including) [id].
  List<String> ancestorsOf(String id) {
    final out = <String>[];
    var cur = nodes[id]?.parent;
    while (cur != null && out.length < 100000) {
      out.insert(0, cur);
      cur = nodes[cur]?.parent;
    }
    return out;
  }

  /// [ids] without any that is below another of them (a drag of a parent
  /// and its child moves the parent only).
  List<String> topmost(Iterable<String> ids) {
    final set = ids.toSet();
    return [
      for (final id in ids)
        if (nodes.containsKey(id) && !ancestorsOf(id).any(set.contains)) id,
    ];
  }

  /// The sibling list [id] sits in and its index there.
  (List<String>, int) siblingsOf(String id) {
    final n = nodes[id];
    final list = n?.parent == null ? roots : (nodes[n!.parent!]?.children ?? const []);
    return (list, list.indexOf(id));
  }

  /// The world matrix of [id]: its ancestors' transforms applied, root first.
  List<double> worldMatrix(String? id) {
    if (id == null) return List.of(identityMatrix);
    final chain = [...ancestorsOf(id), id];
    var m = List.of(identityMatrix);
    for (final a in chain) {
      final t = nodes[a]?.transform;
      if (t != null) m = matMul(m, t.toMatrix());
    }
    return m;
  }

  /// A subtree's ids, [id] first, depth-first (an anchor's fetched
  /// expansion included).
  List<String> subtree(String id) {
    final out = <String>[];
    void walk(String i) {
      out.add(i);
      for (final c in childrenOf(i)) {
        walk(c);
      }
    }

    if (nodes.containsKey(id)) walk(id);
    return out;
  }

  // ── Expansion ─────────────────────────────────────────────────────────

  bool isExpanded(String id) => expanded.contains(id);

  /// Opens [id]; a prefab instance opens whole (its derived subtree is
  /// small and read as one thing — the first look at an instance shows
  /// all of it), and a derived node only itself.
  void expand(String id) {
    var changed = expanded.add(id);
    final n = nodes[id];
    if (n != null && n.isPrefabInstance) {
      final x = expansionOf(id);
      if (x != null) {
        for (final e in x.entities) {
          if (e.children.isNotEmpty) changed |= expanded.add(e.id);
        }
      }
    }
    if (changed) _invalidate();
  }

  void collapse(String id) {
    if (expanded.remove(id)) _invalidate();
  }

  void toggle(String id) => isExpanded(id) ? collapse(id) : expand(id);

  void expandAll() {
    expanded.addAll([
      for (final n in nodes.values.toList())
        if (childrenOf(n.id).isNotEmpty) n.id,
    ]);
    _invalidate();
  }

  void collapseAll() {
    expanded.clear();
    _invalidate();
  }

  /// Expands every ancestor so [id] is a visible row.
  void reveal(String id) {
    var changed = false;
    for (final a in ancestorsOf(id)) {
      changed |= expanded.add(a);
    }
    if (changed) _invalidate();
  }

  // ── Rows ──────────────────────────────────────────────────────────────

  /// The ids the filter matches (every id when the filter is empty).
  Set<String> get matching => _matching ??= _computeMatching();

  Set<String> _computeMatching() {
    if (_filter.isEmpty) return nodes.keys.toSet();
    return {
      for (final n in nodes.values)
        if (_filter.matches(n, hasError: hasError(n.id))) n.id,
    };
  }

  /// The rows the list shows now. With a filter, a match and its ancestors
  /// are shown (the ancestors dimmed) and the path to a match is open
  /// whether or not it is expanded.
  List<HierarchyRow> get rows => _rows ??= _computeRows();

  List<HierarchyRow> _computeRows() {
    final out = <HierarchyRow>[];
    if (_filter.isEmpty) {
      void walk(String id, int depth) {
        final n = nodes[id];
        if (n == null) return;
        final open = expanded.contains(id);
        // A collapsed anchor's expansion is not fetched for the row: an
        // instance has children (its prefab is never empty) unless it is
        // fetched and known not to.
        final kids = open || expansions.containsKey(id) ? childrenOf(id) : n.children;
        final hasChildren = kids.isNotEmpty || (n.isPrefabInstance && !expansions.containsKey(id));
        out.add(HierarchyRow(n, depth: depth, hasChildren: hasChildren, expanded: open, matches: true));
        if (open) {
          for (final c in kids) {
            walk(c, depth + 1);
          }
        }
      }

      for (final r in roots) {
        walk(r, 0);
      }
      return out;
    }
    final matches = matching;
    // Which subtrees hold a match: a node is kept when it or a descendant matches.
    final keep = <String>{};
    for (final id in matches) {
      keep.add(id);
      keep.addAll(ancestorsOf(id));
    }
    void walk(String id, int depth) {
      final n = nodes[id];
      if (n == null || !keep.contains(id)) return;
      final kids = [
        for (final c in childrenOf(id))
          if (keep.contains(c)) c,
      ];
      out.add(
        HierarchyRow(n, depth: depth, hasChildren: kids.isNotEmpty, expanded: true, matches: matches.contains(id)),
      );
      for (final c in kids) {
        walk(c, depth + 1);
      }
    }

    for (final r in roots) {
      walk(r, 0);
    }
    return out;
  }

  int rowIndexOf(String id) => rows.indexWhere((r) => r.id == id);

  void _invalidate() {
    _rows = null;
    _matching = null;
    notifyListeners();
  }
}
