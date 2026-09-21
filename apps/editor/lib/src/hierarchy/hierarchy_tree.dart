// The Hierarchy's model of one document: a node per entity (`name`,
// `parent`, `children`, the component types, the prefab instance, the
// transform for "keep world transform"), the ordered roots, and what is
// expanded. Built once from the whole document; afterwards patched from a
// transaction's `affectedIds` only, so a 10 000-entity scene never rebuilds
// its nodes for one rename. The flat list of visible rows (what the
// virtualised list shows) is computed lazily and cached.
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

  /// The `fynkel.Transform` component's id and data, when the entity has one.
  String? transformId;
  Trs? transform;

  /// Set for the root entity of a prefab instance.
  bool isPrefabInstance = false;
  String? prefabAsset;
  int overrideCount = 0;

  /// Bumped on every patch; a row widget keyed on it repaints only then.
  int version = 0;

  void read(Map<String, Object?> e) {
    name = e['name'] as String? ?? '';
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

  HierarchyFilter _filter = const HierarchyFilter();
  HierarchyFilter get filter => _filter;
  List<HierarchyRow>? _rows;
  Set<String>? _matching;

  bool get isEmpty => nodes.isEmpty;
  int get entityCount => nodes.length;

  HierarchyNode? operator [](String id) => nodes[id];

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
    expanded.removeWhere((id) => !nodes.containsKey(id));
    builds++;
    _invalidate();
  }

  /// Applies what a transaction touched: every named entity is re-read
  /// (dropped when `null`), the roots are replaced. Untouched nodes keep
  /// their identity and version.
  void patch(EntityPatch p) {
    for (final e in p.entities.entries) {
      final v = e.value;
      if (v == null) {
        nodes.remove(e.key);
        expanded.remove(e.key);
      } else {
        (nodes[e.key] ??= HierarchyNode(e.key)).read(v);
      }
    }
    roots = [
      for (final r in p.roots)
        if (nodes.containsKey(r)) r,
    ];
    patches++;
    _invalidate();
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

  /// A subtree's ids, [id] first, depth-first.
  List<String> subtree(String id) {
    final out = <String>[];
    void walk(String i) {
      out.add(i);
      for (final c in nodes[i]?.children ?? const <String>[]) {
        walk(c);
      }
    }

    if (nodes.containsKey(id)) walk(id);
    return out;
  }

  // ── Expansion ─────────────────────────────────────────────────────────

  bool isExpanded(String id) => expanded.contains(id);

  void expand(String id) {
    if (expanded.add(id)) _invalidate();
  }

  void collapse(String id) {
    if (expanded.remove(id)) _invalidate();
  }

  void toggle(String id) => isExpanded(id) ? collapse(id) : expand(id);

  void expandAll() {
    expanded.addAll([
      for (final n in nodes.values)
        if (n.children.isNotEmpty) n.id,
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
        out.add(HierarchyRow(n, depth: depth, hasChildren: n.children.isNotEmpty, expanded: open, matches: true));
        if (open) {
          for (final c in n.children) {
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
        for (final c in n.children)
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
