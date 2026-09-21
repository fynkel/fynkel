// The Hierarchy panel's operations over the focused tab. Every operation
// is one transaction (`apply` with the tab's current revision) so it is one
// undo step and one revision in `fynkel scene inspect --json` after save.
// Reparenting with "keep world transform" puts the `setField`s on the
// moved entity's Transform into the same transaction as the move.
import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/document_tabs.dart';
import '../documents/open_document.dart';
import '../project/editor_project.dart';
import 'hierarchy_tree.dart';
import 'transform_math.dart';

/// Where a drag lands: as a child of [parent] (null = a root) at [index].
final class DropTarget {
  const DropTarget({required this.parent, required this.index});

  final String? parent;
  final int index;

  @override
  String toString() => 'DropTarget(${parent ?? 'root'}, $index)';
}

final class HierarchyModel extends ChangeNotifier {
  HierarchyModel({required this.tabs, required this.project}) {
    tabs.addListener(_onTabs);
    _tab = tabs.focused;
    _tab?.addListener(_onTab);
  }

  final DocumentTabs tabs;
  final EditorProject project;
  OpenDocument? _tab;

  /// Reparenting keeps the entity where it is in the world (default) or
  /// keeps its local transform (off).
  bool keepWorldTransform = true;

  /// The row being renamed in place, if any.
  String? renaming;

  /// Whether the search field should take focus on the next build.
  int searchFocusRequests = 0;

  OpenDocument? get tab => _tab;
  HierarchyTree? get tree => _tab?.tree;
  bool get hasDocument => _tab != null;
  bool get canEdit => _tab != null && _tab!.document.readOnly == null;
  Iterable<String> get selection => _tab?.selection ?? const [];
  bool get hasSelection => _tab != null && _tab!.selection.isNotEmpty;

  HierarchyFilter get filter => _tab?.tree.filter ?? const HierarchyFilter();

  void _onTabs() {
    final f = tabs.focused;
    if (f != _tab) {
      _tab?.removeListener(_onTab);
      _tab = f;
      _tab?.addListener(_onTab);
      renaming = null;
    }
    notifyListeners();
  }

  void _onTab() {
    if (renaming != null && _tab?.tree[renaming!] == null) renaming = null;
    notifyListeners();
  }

  // ── Component types (for Create with… and the filter) ────────────────

  /// Native components that may be authored on an entity, `fynkel.Transform` first.
  List<ComponentType> get nativeComponentTypes {
    final all = project.componentTypes().where((c) => c.authored && c.allowed).toList()
      ..sort((a, b) => a.type.compareTo(b.type));
    return all;
  }

  /// Every component type the document's entities carry, for the filter menu.
  List<String> get presentComponentTypes {
    final t = _tab?.tree;
    if (t == null) return const [];
    final s = <String>{for (final n in t.nodes.values) ...n.componentTypes};
    return s.toList()..sort();
  }

  // ── Filter / expansion ────────────────────────────────────────────────

  void setFilter(HierarchyFilter f) {
    final t = _tab?.tree;
    if (t == null) return;
    t.setFilter(f);
    notifyListeners();
  }

  void setQuery(String q) => setFilter(filter.copyWith(query: q));
  void clearFilter() => setFilter(const HierarchyFilter());

  void toggleComponentFilter(String type) {
    final s = Set<String>.of(filter.componentTypes);
    if (!s.remove(type)) s.add(type);
    setFilter(filter.copyWith(componentTypes: s));
  }

  void requestSearchFocus() {
    searchFocusRequests++;
    notifyListeners();
  }

  void toggle(String id) {
    _tab?.tree.toggle(id);
    notifyListeners();
  }

  void expand(String id) {
    _tab?.tree.expand(id);
    notifyListeners();
  }

  void collapse(String id) {
    _tab?.tree.collapse(id);
    notifyListeners();
  }

  void expandAll() {
    _tab?.tree.expandAll();
    notifyListeners();
  }

  void collapseAll() {
    _tab?.tree.collapseAll();
    notifyListeners();
  }

  // ── Selection ─────────────────────────────────────────────────────────

  void select(String id, {bool toggle = false, bool range = false}) => _tab?.select(id, toggle: toggle, range: range);

  void selectAll() {
    final t = _tab;
    if (t == null) return;
    t.setSelection([for (final r in t.tree.rows) r.id]);
  }

  void clearSelection() => _tab?.clearSelection();

  /// Reveals (expands the ancestors of) and selects [id]; false when unknown.
  bool revealAndSelect(String id) {
    final t = _tab;
    if (t == null || t.tree[id] == null) return false;
    if (!t.tree.filter.isEmpty && !t.tree.matching.contains(id)) t.tree.setFilter(const HierarchyFilter());
    t.tree.reveal(id);
    t.select(id);
    notifyListeners();
    return true;
  }

  /// Moves the selection one visible row up or down (⇧ extends).
  void step(int delta, {bool extend = false}) {
    final t = _tab;
    if (t == null) return;
    final rows = t.tree.rows;
    if (rows.isEmpty) return;
    final from = t.lead == null ? -1 : t.tree.rowIndexOf(t.lead!);
    final i = (from < 0 ? (delta > 0 ? 0 : rows.length - 1) : (from + delta)).clamp(0, rows.length - 1);
    t.select(rows[i].id, range: extend);
  }

  /// → opens a collapsed row or goes to its first child; ← closes an open
  /// row or goes to the parent.
  void arrow(bool right) {
    final t = _tab;
    if (t == null || t.lead == null) return;
    final n = t.tree[t.lead!];
    if (n == null) return;
    if (right) {
      if (n.children.isEmpty) return;
      if (!t.tree.expanded.contains(n.id)) {
        t.tree.expand(n.id);
      } else {
        t.select(n.children.first);
      }
    } else {
      if (t.tree.expanded.contains(n.id) && n.children.isNotEmpty) {
        t.tree.collapse(n.id);
      } else if (n.parent != null) {
        t.select(n.parent!);
      }
    }
    notifyListeners();
  }

  // ── Operations (each one transaction) ────────────────────────────────

  Applied? _run(List<Map<String, Object?>> commands) {
    final t = _tab;
    if (t == null || commands.isEmpty) return null;
    final a = t.transaction(commands);
    notifyListeners();
    return a;
  }

  void beginRename([String? id]) {
    final target = id ?? _tab?.lead ?? _tab?.selection.lastOrNull;
    if (target == null || !canEdit || _tab?.tree[target] == null) return;
    renaming = target;
    notifyListeners();
  }

  void cancelRename() {
    if (renaming == null) return;
    renaming = null;
    notifyListeners();
  }

  /// Renames [id]; an unchanged or empty name is not a transaction.
  bool rename(String id, String name) {
    renaming = null;
    final n = _tab?.tree[id];
    final trimmed = name.trim();
    if (n == null || trimmed.isEmpty || trimmed == n.name) {
      notifyListeners();
      return false;
    }
    return _run([Command.renameObject(id, trimmed)])?.ok ?? false;
  }

  /// Duplicates the topmost selected entities (a selected child of a
  /// selected parent comes along with the parent's copy); the copies are
  /// selected afterwards.
  bool duplicate([Iterable<String>? ids]) {
    final t = _tab;
    if (t == null) return false;
    final targets = t.tree.topmost(ids ?? t.selection);
    if (targets.isEmpty) return false;
    final a = _run([for (final id in targets) Command.duplicateObject(id)]);
    if (a == null || !a.ok) return false;
    final copies = [
      for (final m in a.mutations)
        if (m.affectedIds.isNotEmpty) m.affectedIds.first,
    ];
    t.setSelection(copies.where((id) => t.tree[id] != null));
    return true;
  }

  /// Deletes the topmost selected entities (their subtrees go with them).
  bool delete([Iterable<String>? ids]) {
    final t = _tab;
    if (t == null) return false;
    final targets = t.tree.topmost(ids ?? t.selection);
    if (targets.isEmpty) return false;
    // Select the nearest remaining row afterwards.
    final rows = t.tree.rows;
    final firstRow = targets
        .map(t.tree.rowIndexOf)
        .where((i) => i >= 0)
        .fold<int>(rows.length, (a, b) => a < b ? a : b);
    final a = _run([for (final id in targets) Command.deleteObject(id)]);
    if (a == null || !a.ok) return false;
    final after = t.tree.rows;
    if (after.isNotEmpty) {
      t.select(after[firstRow.clamp(0, after.length - 1)].id);
    } else {
      t.clearSelection();
    }
    return true;
  }

  /// Creates an entity named "Empty" with a default Transform under
  /// [parent] (null: under the single selected entity, else a root) and
  /// selects it — ready to rename.
  bool createEmpty({String? parent, bool underSelection = true}) =>
      createWith(null, parent: parent, underSelection: underSelection);

  /// Creates an entity with a Transform and the native component [type]
  /// with its defaults, named after the type.
  bool createWith(String? type, {String? parent, bool underSelection = true}) {
    final t = _tab;
    if (t == null || !canEdit) return false;
    final under = parent ?? (underSelection && t.selection.length == 1 ? t.selection.first : null);
    final components = <Map<String, Object?>>[];
    final transformData = project.componentDefaults('fynkel.Transform') ?? Trs.identity.toData();
    components.add(componentSpec('fynkel.Transform', data: transformData));
    var name = 'Empty';
    if (type != null && type != 'fynkel.Transform') {
      final data = project.componentDefaults(type);
      if (data == null) {
        t.lastError = 'No defaults for $type — it is not an authorable native component.';
        notifyListeners();
        return false;
      }
      components.add(componentSpec(type, data: data));
      name = type.split('.').last;
    }
    final a = _run([Command.createObject(name: name, parent: under, components: components)]);
    if (a == null || !a.ok) return false;
    final created = a.mutations.isNotEmpty && a.mutations.first.affectedIds.isNotEmpty
        ? a.mutations.first.affectedIds.first
        : null;
    if (created != null && t.tree[created] != null) {
      if (under != null) t.tree.expand(under);
      t.select(created);
      renaming = created;
      notifyListeners();
    }
    return true;
  }

  /// Whether a dragged asset may be dropped: a prefab document or an
  /// imported `#prefab` (kind `prefab`), into an editable document, not
  /// into a prefab instance's expansion.
  bool canDropAsset(Map<String, Object?> ref, String kind, DropTarget target) {
    final t = _tab;
    if (t == null || !canEdit || kind != 'prefab') return false;
    if (ref['asset'] is! String) return false;
    if (t.kind == 'prefab' && ref['sub'] == null && ref['asset'] == t.document.id) return false;
    final parent = target.parent;
    if (parent != null) {
      final n = t.tree[parent];
      if (n == null || n.isPrefabInstance) return false;
    }
    return true;
  }

  /// Instantiates the prefab [ref] (`{asset, sub?}`) under [target] — one
  /// `instantiatePrefab` transaction — and selects the instance.
  bool instantiatePrefab(Map<String, Object?> ref, {required String name, DropTarget? target}) {
    final t = _tab;
    if (t == null || !canEdit) return false;
    final parent = target?.parent;
    final index = target == null ? null : (parent == null && !rootOrderIsEditable ? null : target.index);
    final a = _run([Command.instantiatePrefab(prefab: ref, name: name, parent: parent, index: index)]);
    if (a == null || !a.ok) return false;
    final created = a.mutations.isNotEmpty && a.mutations.first.affectedIds.isNotEmpty
        ? a.mutations.first.affectedIds.first
        : null;
    if (created != null && t.tree[created] != null) {
      if (parent != null) t.tree.expand(parent);
      t.select(created);
      notifyListeners();
    }
    return true;
  }

  /// Whether [ids] may be dropped at [target]: not onto themselves or
  /// their own descendants; prefab-instance children are not in the document.
  /// Whether the roots can be ordered by a drop: a prefab lists its `roots`;
  /// a scene's roots follow the entity key order and `index` is ignored there.
  bool get rootOrderIsEditable => _tab?.kind == 'prefab';

  bool canDrop(Iterable<String> ids, DropTarget target) {
    final t = _tab;
    if (t == null || !canEdit) return false;
    final moving = t.tree.topmost(ids);
    if (moving.isEmpty) return false;
    if (target.parent != null) {
      if (t.tree[target.parent!] == null) return false;
      for (final id in moving) {
        if (t.tree.isAncestorOrSelf(id, target.parent!)) return false;
      }
    } else if (t.kind == 'prefab' && t.tree.roots.isEmpty) {
      return false;
    }
    return true;
  }

  /// Moves the topmost of [ids] under [target] at its index, in their
  /// current visible order; with [keepWorldTransform] the moved entities'
  /// Transforms are rewritten in the same transaction so nothing moves on
  /// screen. One transaction for the whole drop.
  bool reparent(Iterable<String> ids, DropTarget target) {
    final t = _tab;
    if (t == null || !canDrop(ids, target)) return false;
    final tree = t.tree;
    final order = {for (var i = 0; i < tree.rows.length; i++) tree.rows[i].id: i};
    final moving = tree.topmost(ids)..sort((a, b) => (order[a] ?? 1 << 30).compareTo(order[b] ?? 1 << 30));
    final commands = <Map<String, Object?>>[];
    // An index counted in the target's current children: entities already
    // in that parent before the drop point shift it down once removed.
    var index = target.index;
    final (siblings, _) = target.parent == null ? (tree.roots, 0) : (tree[target.parent!]!.children, 0);
    for (final id in moving) {
      final pos = siblings.indexOf(id);
      if (pos >= 0 && pos < index) index--;
    }
    final parentWorld = tree.worldMatrix(target.parent);
    final invParent = matInverseAffine(parentWorld);
    for (final id in moving) {
      final n = tree[id]!;
      commands.add(Command.reparentObject(id, parent: target.parent, index: index++));
      if (keepWorldTransform && n.transformId != null && n.transform != null && n.parent != target.parent) {
        final world = tree.worldMatrix(id);
        final local = Trs.fromMatrix(matMul(invParent, world));
        if (!local.sameAs(n.transform!)) {
          final data = local.toData();
          final old = n.transform!.toData();
          for (final field in const ['position', 'rotation', 'scale']) {
            if (!listEquals(data[field] as List?, old[field] as List?)) {
              commands.add(Command.setField(id, n.transformId!, field, data[field]));
            }
          }
        }
      }
    }
    final a = _run(commands);
    if (a == null || !a.ok) return false;
    if (target.parent != null) tree.expand(target.parent!);
    t.setSelection(moving);
    return true;
  }

  /// Menu action: move the selection to the root (keeps the world transform per the policy).
  bool moveToRoot() {
    final t = _tab;
    if (t == null) return false;
    return reparent(t.selection, DropTarget(parent: null, index: t.tree.roots.length));
  }

  void toggleKeepWorldTransform() {
    keepWorldTransform = !keepWorldTransform;
    notifyListeners();
  }

  @override
  void dispose() {
    tabs.removeListener(_onTabs);
    _tab?.removeListener(_onTab);
    super.dispose();
  }
}
