// The Hierarchy panel's operations over the focused tab. Every operation
// is one transaction (`apply` with the tab's current revision) so it is one
// undo step and one revision in `fynkel scene inspect --json` after save.
// Reparenting with "keep world transform" puts the `setField`s on the
// moved entity's Transform into the same transaction as the move.
// Prefab mode (item 7, ADR-0022): the instance is the prefab's root, so
// its own edits are overrides on the root; a derived entity (one of the
// root's descendants) is not in the document, so its operations are
// override ops on the instance — Delete is a `removeObject` override (or the revert
// of the `addObject` that added it), Create Empty Child an `addObject`
// override, Rename only for an added object (its `addObject` re-issued);
// derived entities are never duplicated, dragged or drop targets.
// Item 9e: the padlock keeps a Hierarchy on one document while another tab
// is focused (a second Hierarchy copy locked to a prefab beside the scene).
import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/document_tabs.dart';
import '../documents/open_document.dart';
import '../inspector/component_specs.dart';
import '../inspector/prefab_actions.dart' as prefab;
import '../play/apply_to_scene.dart';
import '../play/play_document.dart';
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
    _tab = tabs.shown;
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

  /// [id] is a derived entity of a prefab instance (not in the document).
  bool isDerived(String id) => _tab?.tree[id]?.isDerived ?? false;

  /// Whether [id] can be renamed in place: a document entity, or an
  /// object an override added to an instance (its `addObject` is re-issued).
  bool canRename(String id) {
    final n = _tab?.tree[id];
    return canEdit && n != null && (!n.isDerived || n.added);
  }

  Iterable<String> get selection => _tab?.selection ?? const [];
  bool get hasSelection => _tab != null && _tab!.selection.isNotEmpty;

  HierarchyFilter get filter => _tab?.tree.filter ?? const HierarchyFilter();

  // ── The padlock (item 9e) ────────────────────────────────────────────

  OpenDocument? _pinned;

  /// Showing one document whatever tab is focused; its close lets go.
  bool get locked => _pinned != null;

  void setLocked(bool on) {
    if (on == locked || (on && _tab == null)) return;
    _pinned = on ? _tab : null;
    _onTabs();
  }

  void _onTabs() {
    if (_pinned != null && !tabs.open.contains(_pinned) && !identical(_pinned, tabs.live)) _pinned = null;
    final f = _pinned ?? tabs.shown;
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

  /// Reveals (expands the ancestors of) and selects [id]; false when
  /// unknown. A derived entity of an instance not opened yet is found by
  /// expanding the instance (`HierarchyTree.locate`).
  bool revealAndSelect(String id) {
    final t = _tab;
    if (t == null || !t.tree.locate(id)) return false;
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
      if (!t.tree.expanded.contains(n.id)) {
        if (n.children.isEmpty && !n.isPrefabInstance) return;
        t.tree.expand(n.id);
        if (t.tree.childrenOf(n.id).isEmpty) t.tree.collapse(n.id);
      } else {
        final kids = t.tree.childrenOf(n.id);
        if (kids.isNotEmpty) t.select(kids.first);
      }
    } else {
      if (t.tree.expanded.contains(n.id) && t.tree.childrenOf(n.id).isNotEmpty) {
        t.tree.collapse(n.id);
      } else if (n.parent != null) {
        t.select(n.parent!);
      }
    }
    notifyListeners();
  }

  // ── Apply to Scene (ADR-0021 P7) ─────────────────────────────────────

  /// The running game, while the Hierarchy shows it.
  PlayDocument? get _play => _tab?.document is PlayDocument ? _tab!.document as PlayDocument : null;

  /// Whether the Hierarchy shows the running game (Apply to Scene is
  /// offered then).
  bool get showsPlay => _play != null;

  /// Apply to Scene: the selected running objects — their names, parents,
  /// components and children, what was made during Play created — written
  /// into the scene the game was started from, as one transaction on its
  /// tab. A problem is shown on the running game's tab.
  bool applyToScene([Iterable<String>? ids]) {
    final t = _tab, play = _play;
    if (t == null || play == null) return false;
    final targets = (ids ?? t.selection).toList();
    if (targets.isEmpty) return false;
    return _applyToScene(t, play, () {
      final specs = ComponentSpecs(project);
      Map<String, Object?> defaultsOf(String type) {
        final spec = specs.of(type);
        return (spec.isBehaviour ? spec.schemaDefaults() : project.componentDefaults(type)) ?? const {};
      }

      return applyObjectsToSceneCommands(playSceneTab(tabs, play)!, play, targets, defaultsOf: defaultsOf);
    });
  }

  /// The names of the objects deleted during Play that the scene still
  /// has.
  List<String> get deletionsToApply {
    final play = _play;
    final scene = play == null ? null : playSceneTab(tabs, play);
    if (play == null || scene == null || play.deletedHere.isEmpty) return const [];
    return [for (final id in deletionsToSceneCommands(scene, play).ids) scene.tree[id]?.name ?? id];
  }

  /// Apply to Scene for what was deleted during Play.
  bool applyDeletionsToScene() {
    final t = _tab, play = _play;
    if (t == null || play == null) return false;
    List<String> done = const [];
    final ok = _applyToScene(t, play, () {
      final d = deletionsToSceneCommands(playSceneTab(tabs, play)!, play);
      done = d.ids;
      return d.commands;
    });
    if (ok) play.deletedHere.removeAll(done);
    notifyListeners();
    return ok;
  }

  bool _applyToScene(OpenDocument t, PlayDocument play, List<Map<String, Object?>> Function() commands) {
    final scene = playSceneTab(tabs, play);
    String? why;
    if (scene == null) {
      why = 'The scene the game was started from is not open.';
    } else if (scene.document.readOnly != null) {
      why = 'The scene is read-only.';
    } else {
      try {
        final c = commands();
        if (c.isEmpty) {
          why = 'Nothing to apply: the scene has these objects as they run.';
        } else {
          final a = scene.transaction(c);
          if (a == null || !a.ok) why = scene.lastError ?? 'The scene refused it.';
        }
      } on StateError catch (e) {
        why = e.message;
      }
    }
    t.lastError = why;
    t.keepEditing();
    notifyListeners();
    return why == null;
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
    if (target == null || !canRename(target)) {
      final t = _tab;
      if (t != null && target != null && canEdit && isDerived(target)) {
        t.lastError = 'A prefab instance\'s entity is named by the prefab — rename it there (Open prefab).';
        t.keepEditing();
      }
      return;
    }
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
    if (n.isDerived) {
      final t = _tab!;
      final x = t.expansionOf(n.anchor!);
      final e = x?.entity(id);
      if (x == null || e == null || !e.added) {
        t.lastError = 'A prefab instance\'s entity is named by the prefab — rename it there (Open prefab).';
        notifyListeners();
        return false;
      }
      final op = prefab.reissuedAddObjectOverride(x, e, name: trimmed);
      return _run([Command.setPrefabOverrideOp(n.anchor!, op)])?.ok ?? false;
    }
    return _run([Command.renameObject(id, trimmed)])?.ok ?? false;
  }

  /// Duplicates the topmost selected entities (a selected child of a
  /// selected parent comes along with the parent's copy); the copies are
  /// selected afterwards.
  bool duplicate([Iterable<String>? ids]) {
    final t = _tab;
    if (t == null) return false;
    final all = t.tree.topmost(ids ?? t.selection);
    final targets = all.where((id) => !t.tree[id]!.isDerived).toList();
    if (targets.isEmpty) {
      if (all.isNotEmpty) {
        t.lastError = 'A prefab instance\'s entities cannot be duplicated on their own — duplicate the instance.';
        t.keepEditing();
      }
      return false;
    }
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
  /// A derived entity is removed from its instance instead: a
  /// `removeObject` override, or — for an object an override added — the
  /// revert of that `addObject`. One transaction for the whole selection.
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
    final commands = <Map<String, Object?>>[];
    final reverts = <String, List<int>>{};
    final removes = <Map<String, Object?>>[];
    for (final id in targets) {
      final n = t.tree[id]!;
      if (!n.isDerived) {
        commands.add(Command.deleteObject(id));
        continue;
      }
      final anchor = n.anchor!;
      final x = t.expansionOf(anchor);
      final e = x?.entity(id);
      if (x == null || e == null) {
        t.lastError = 'The prefab instance did not expand; nothing to remove.';
        notifyListeners();
        return false;
      }
      // The added objects below it lose their target with it: their
      // `addObject`s are reverted too.
      for (final d in [id, ...t.tree.subtree(id).where((s) => s != id)]) {
        final de = x.entity(d);
        if (de == null) continue;
        if (de.added) {
          final i = prefab.addObjectIndexOf(x, de);
          if (i != null) (reverts[anchor] ??= []).add(i);
        } else if (d == id) {
          removes.add(Command.setPrefabOverrideOp(anchor, prefab.removeObjectOverride(e)));
        }
      }
    }
    // Reverts by index highest first (a removal never shifts the next),
    // then the new override ops (appended after the removals).
    for (final entry in reverts.entries) {
      final indices = entry.value.toSet().toList()..sort((a, b) => b.compareTo(a));
      commands.addAll([for (final i in indices) Command.revertPrefabOverride(entry.key, index: i)]);
    }
    commands.addAll(removes);
    final a = _run(commands);
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
    final underNode = under == null ? null : t.tree[under];
    if (underNode != null && underNode.isDerived) return _createDerived(underNode, name, components);
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

  /// Under a derived entity: an `addObject` override on its anchor (one
  /// transaction); the new derived entity is selected once the refreshed
  /// expansion shows it.
  bool _createDerived(HierarchyNode under, String name, List<Map<String, Object?>> components) {
    final t = _tab!;
    final anchor = under.anchor!;
    final x = t.expansionOf(anchor);
    final e = x?.entity(under.id);
    if (x == null || e == null) return false;
    final op = prefab.addObjectOverride(name: name, parent: e.local, components: components);
    final a = _run([Command.setPrefabOverrideOp(anchor, op)]);
    if (a == null || !a.ok) return false;
    final created = t.expansionOf(anchor)?.byLocal(op['entity'] as String)?.id;
    if (created != null && t.tree[created] != null) {
      t.tree.expand(under.id);
      t.select(created);
      renaming = created;
      notifyListeners();
    }
    return true;
  }

  /// Why [ids] (a Hierarchy drag) cannot become a prefab, or null when
  /// it can: one plain object of an editable scene, with no prefab
  /// instance in its subtree (nested prefabs from here come later).
  String? cannotCreatePrefab(List<String> ids) {
    final t = _tab;
    if (t == null || !canEdit) return 'Nothing editable is open.';
    if (t.kind != 'scene') return 'Prefabs are made from a scene\'s objects.';
    if (ids.length != 1) return 'Drag one object to make a prefab.';
    final n = t.tree[ids.single];
    if (n == null) return 'The object is gone.';
    if (n.isDerived) return 'That object belongs to a prefab instance; open the prefab to change it.';
    final entities = _entities(t);
    for (final id in _subtree(entities, ids.single)) {
      if ((entities[id] as Map?)?['prefab'] != null) {
        return id == ids.single
            ? '${n.name} is a prefab instance already.'
            : '${n.name} holds a prefab instance; nested prefabs from the Hierarchy are not supported yet.';
      }
    }
    return null;
  }

  /// Drag from the Hierarchy to the Project panel (Unity's way to make a
  /// prefab): writes `<folder>/<name>.prefab.json` with the object and its
  /// children — through the prefab's own commands, then saved — and
  /// replaces the object in the scene by an instance of it at the same
  /// place (one scene transaction, one ⌘Z; the prefab file stays, as in
  /// Unity). The object becomes the instance under its own id — the
  /// prefab's root (ADR-0022) — and its Transform stays the root's, with
  /// no override, so the scene looks the same. Returns the new prefab's
  /// id.
  String? createPrefab(String id, String folder) {
    final t = _tab;
    final why = cannotCreatePrefab([id]);
    if (t == null || why != null) {
      if (t != null) {
        t.lastError = why;
        notifyListeners();
      }
      return null;
    }
    final entities = _entities(t);
    final root = entities[id] as Map;
    final name = root['name'] as String? ?? 'Prefab';
    final path = _freePrefabPath(folder, name);
    final String prefabId;
    try {
      prefabId = project.newDocument('prefab', path, name: name).id!;
    } on AuthoringException catch (e) {
      t.lastError = 'Could not create the prefab: ${_why(e)}';
      notifyListeners();
      return null;
    }
    final doc = project.openDocument(prefabId);
    try {
      // The new prefab has its one root (ADR-0022), named after it with
      // the default Transform: the object fills it — its Transform's
      // values, its other components — and its children go under it.
      final prefabRoot = doc.root();
      final rootId = prefabRoot['root'] as String;
      final rootComponents =
          (((prefabRoot['entities'] as Map)[rootId] as Map)['components'] as Map).cast<String, Object?>();
      final rootTransform = rootComponents.entries.firstWhere((c) => (c.value as Map)['type'] == 'fynkel.Transform').key;
      final commands = <Map<String, Object?>>[];
      final inactive = <String>[];
      var transformKept = false;
      for (final c in ((root['components'] as Map?) ?? const {}).entries) {
        final spec = c.value as Map;
        final data = spec['data'] as Map<String, Object?>?;
        if (spec['type'] == 'fynkel.Transform' && !transformKept) {
          transformKept = true;
          for (final f in (data ?? const {}).entries) {
            commands.add(Command.setField(rootId, rootTransform, f.key, f.value));
          }
          continue;
        }
        commands.add(
          Command.addComponent(
            rootId,
            componentSpec(spec['type'] as String, id: c.key as String, version: spec['version'] as int?, data: data),
          ),
        );
      }
      if (!transformKept) commands.add(Command.removeComponent(rootId, rootTransform));
      if (root['active'] == false) inactive.add(rootId);
      for (final e in _subtree(entities, id).skip(1)) {
        final entity = entities[e] as Map;
        final components = <Map<String, Object?>>[];
        for (final c in ((entity['components'] as Map?) ?? const {}).entries) {
          final spec = c.value as Map;
          final data = spec['data'] as Map<String, Object?>?;
          components.add(componentSpec(spec['type'] as String, id: c.key as String, version: spec['version'] as int?, data: data));
        }
        final parent = entity['parent'] as String?;
        commands.add(
          Command.createObject(
            id: e,
            name: entity['name'] as String? ?? '',
            parent: parent == id ? rootId : parent,
            components: components,
          ),
        );
        if (entity['active'] == false) inactive.add(e);
      }
      commands.addAll([for (final e in inactive) Command.setObjectActive(e, false)]);
      final a = doc.apply(commands, baseRevision: doc.revision);
      if (!a.ok) {
        t.lastError = 'Could not fill the prefab: ${describeRejection(a)}';
        notifyListeners();
        return null;
      }
      doc.save();
    } on AuthoringException catch (e) {
      t.lastError = 'Could not fill the prefab: ${_why(e)}';
      notifyListeners();
      return null;
    } finally {
      doc.close();
    }
    // The scene: the object out, the instance in at its place.
    final parent = root['parent'] as String?;
    final siblings = parent == null
        ? t.document.roots()
        : (((entities[parent] as Map?)?['children'] as List?) ?? const []).cast<String>();
    final index = siblings.indexOf(id);
    final a = _run([
      Command.deleteObject(id),
      Command.instantiatePrefab(
        prefab: {'asset': prefabId},
        id: id,
        name: name,
        parent: parent,
        // Its place among the siblings — a scene root's too (key order).
        index: index < 0 ? null : index,
      ),
    ]);
    if (a == null || !a.ok) return prefabId;
    if (t.tree[id] != null) {
      t.select(id);
      notifyListeners();
    }
    return prefabId;
  }

  static String _why(AuthoringException e) =>
      e.diagnostics.isEmpty ? e.toString() : e.diagnostics.map((d) => d.message).join('; ');

  static Map<String, Object?> _entities(OpenDocument t) =>
      (t.document.root()['entities'] as Map?)?.cast<String, Object?>() ?? const {};

  /// [id] and its descendants, parents first, children in order.
  static List<String> _subtree(Map<String, Object?> entities, String id) {
    final out = <String>[];
    void walk(String e) {
      out.add(e);
      for (final c in (((entities[e] as Map?)?['children'] as List?) ?? const []).cast<String>()) {
        walk(c);
      }
    }

    walk(id);
    return out;
  }

  /// `<folder>/<name>.prefab.json`, numbered when taken (`Coin 1`, …).
  String _freePrefabPath(String folder, String name) {
    final clean = name.replaceAll(RegExp(r'[/\\:*?"<>|]'), '_').trim();
    final base = clean.isEmpty ? 'Prefab' : clean;
    final taken = {for (final a in project.assets()) a.path.toLowerCase()};
    String at(String n) => folder.isEmpty ? '$n.prefab.json' : '$folder/$n.prefab.json';
    var candidate = at(base);
    for (var i = 1; taken.contains(candidate.toLowerCase()); i++) {
      candidate = at('$base $i');
    }
    return candidate;
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
      if (n == null || n.isPrefabInstance || n.isDerived) return false;
    }
    return true;
  }

  /// Instantiates the prefab [ref] (`{asset, sub?}`) under [target] — one
  /// `instantiatePrefab` transaction — and selects the instance. The
  /// instance is the prefab's root (ADR-0022) and has its Transform: where
  /// the prefab puts it, or at [transform]'s position (the Scene view's
  /// drop, where the pointer hit) — a `position` override, as Unity
  /// stores it.
  bool instantiatePrefab(Map<String, Object?> ref, {required String name, DropTarget? target, Trs? transform}) {
    final t = _tab;
    if (t == null || !canEdit) return false;
    final parent = target?.parent;
    if (parent != null && (t.tree[parent]?.isDerived ?? true)) return false;
    final index = target == null ? null : (parent == null && !rootOrderIsEditable ? null : target.index);
    final components = [
      if (transform != null) componentSpec('fynkel.Transform', data: {'position': transform.toData()['position']}),
    ];
    final a = _run([
      Command.instantiatePrefab(prefab: ref, name: name, parent: parent, index: index, components: components),
    ]);
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
    // A derived entity sits where its prefab puts it; an instance's
    // expansion takes no document entities.
    if (moving.any((id) => t.tree[id]!.isDerived)) return false;
    if (target.parent != null) {
      final p = t.tree[target.parent!];
      if (p == null || p.isDerived) return false;
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
      final world = tree.worldMatrix(id); // reads an instance's root first
      if (keepWorldTransform && n.transformId != null && n.transform != null && n.parent != target.parent) {
        final local = Trs.fromMatrix(matMul(invParent, world));
        if (!local.sameAs(n.transform!)) {
          final data = local.toData();
          final old = n.transform!.toData();
          for (final field in const ['position', 'rotation', 'scale']) {
            if (listEquals(data[field] as List?, old[field] as List?)) continue;
            final anchor = n.overrideAnchor;
            commands.add(
              anchor == null
                  ? Command.setField(id, n.transformId!, field, data[field])
                  // An instance moves by an override on its root (ADR-0022).
                  : Command.setPrefabOverrideOp(
                      anchor,
                      prefab.setFieldOverride(
                        entity: n.overrideLocal!,
                        component: n.transformLocal ?? n.transformId!,
                        data: old,
                        path: field,
                        value: data[field],
                      ),
                    ),
            );
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

/// The instance's name from a dragged asset's name: the prefab file's
/// stem, or the model's stem for an imported `#prefab`.
String prefabInstanceName(String assetName) {
  var n = assetName;
  final hash = n.indexOf('#');
  if (hash >= 0) n = n.substring(0, hash);
  for (final suffix in ['.prefab.json', '.glb', '.gltf', '.fbx']) {
    if (n.toLowerCase().endsWith(suffix)) return n.substring(0, n.length - suffix.length);
  }
  return n;
}
