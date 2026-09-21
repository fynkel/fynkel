// One document tab: the `EditorDocument` plus what only the editor knows
// about it — the Hierarchy tree (built once, then patched from each
// transaction's `affectedIds`), the selection, the document's diagnostics
// by entity, the last refused transaction, and the file's baseline text so
// an external change is told apart from the editor's own save (no local
// edits → silent reload; local edits → a conflict that is never overwritten).
import 'dart:collection';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../hierarchy/hierarchy_tree.dart';
import '../project/editor_project.dart';
import 'editor_document.dart';

/// The file changed under an open tab that has unsaved edits (or vanished).
final class DiskConflict {
  const DiskConflict({
    required this.baseRevision,
    required this.ourRevision,
    required this.diskRevision,
    required this.baselineText,
    required this.diskText,
    required this.history,
    required this.detectedAt,
  });

  /// The revision the tab was opened from or last saved (the file's then).
  final String baseRevision;

  /// The editor's revision now (unsaved).
  final String ourRevision;

  /// The file's revision now, or null when it cannot be read.
  final String? diskRevision;

  /// The file's text when the tab last agreed with it.
  final String? baselineText;

  /// The file's text now; null when the file is gone.
  final String? diskText;

  /// Committed transactions in the tab's history.
  final int history;
  final DateTime detectedAt;

  bool get fileMissing => diskText == null;
}

/// The lines a rejected transaction shows: every error, the transaction's
/// and each command's. A summary that carries the real reasons as `related`
/// diagnostics (`FYN-E0005`, "it would leave the document with N error(s)")
/// is replaced by those reasons — "entity already has a fynkel.Transform
/// component" is what the user needs to read, with the crate's suggestion.
String describeRejection(Applied a) {
  if (a.isConflict) return 'The document changed under this edit (FYN-E0001); nothing was written. Try again.';
  final lines = <String>[];
  void add(Diagnostic d) {
    if (d.severity != Severity.error) return;
    final reasons = d.related.where((r) => r.severity == Severity.error).toList();
    if (reasons.isNotEmpty) {
      reasons.forEach(add);
      return;
    }
    lines.add('${d.code}: ${d.message}${d.suggestion == null ? '' : ' — ${d.suggestion}'}');
  }

  a.diagnostics.forEach(add);
  for (final m in a.mutations) {
    m.diagnostics.forEach(add);
  }
  return lines.isEmpty ? 'rejected' : lines.join('\n');
}

final class OpenDocument extends ChangeNotifier {
  OpenDocument(this.document, {required this.project}) {
    _baseRevision = document.revision;
    _baselineText = _readDisk();
    document.addListener(_onDocument);
    project.addListener(_onProject);
  }

  final EditorDocument document;
  final EditorProject project;

  HierarchyTree? _tree;
  late String _baseRevision;
  String? _baselineText;
  DiskConflict? conflict;

  /// The last refused transaction or failed call, as text; null after a success.
  String? lastError;

  /// The document's own diagnostics (validate) — refreshed after every change.
  List<Diagnostic> diagnostics = const [];

  final LinkedHashSet<String> selection = LinkedHashSet<String>();

  /// Where a range selection starts; the last clicked row.
  String? anchor;
  String? lead;

  // ── Identity ──────────────────────────────────────────────────────────

  String get path => document.path;
  String get fileName => p.basename(document.path);
  String get title => document.name.isNotEmpty ? document.name : fileName;

  /// The file's stem without the kind's extension (`main-mine` for `main-mine.scene.json`).
  String get fileStem {
    var stem = fileName;
    for (final ext in const ['.scene.json', '.prefab.json', '.material.json', '.json']) {
      if (stem.endsWith(ext)) return stem.substring(0, stem.length - ext.length);
    }
    return stem;
  }

  /// What the tab shows: the document's name, plus the file's stem when the two
  /// disagree ("Main · main-mine" after a Save As — the name inside the file is
  /// still "Main", and the gate found that confusing without the file).
  String get tabLabel {
    String norm(String s) => s.toLowerCase().replaceAll(RegExp(r'[\s_\-]+'), '');
    if (document.name.isEmpty || norm(document.name) == norm(fileStem)) return title;
    return '$title · $fileStem';
  }

  String get kind => document.kind;
  bool get dirty => document.dirty;
  bool get hasConflict => conflict != null;

  /// "Undo (3)" — the menu label with the history depth.
  String get canUndoLabel => document.canUndo ? 'Undo (${document.history})' : 'Undo';

  /// What the tab's tooltip shows.
  String get location => document.relativePath ?? project.relative(document.path);

  // ── Hierarchy ─────────────────────────────────────────────────────────

  /// The tree, built on first use from the whole document.
  HierarchyTree get tree {
    if (_tree == null) _buildTree();
    return _tree!;
  }

  bool get treeBuilt => _tree != null;

  void _buildTree() {
    final t = _tree ??= HierarchyTree();
    t.build(document.root(), roots: document.roots());
    _refreshDiagnostics(t);
    _pruneSelection();
  }

  void _patchTree(List<String> ids) {
    final t = _tree;
    if (t == null) return;
    if (ids.isEmpty) return;
    var patch = document.entities(ids);
    var suspects = _suspects(t, patch);
    t.patch(patch);
    // A parent whose children list the query did not carry (the other end
    // of a move, a duplicate's parent) is re-read once more.
    if (suspects.isNotEmpty) {
      patch = document.entities(suspects);
      t.patch(patch);
    }
    _refreshDiagnostics(t);
    _pruneSelection();
  }

  /// Parents the patch leaves inconsistent: the old parent of an entity
  /// that moved away, and a new parent that does not list its child yet.
  List<String> _suspects(HierarchyTree t, EntityPatch patch) {
    final out = <String>{};
    for (final e in patch.entities.entries) {
      final before = t[e.key];
      final after = e.value;
      final oldParent = before?.parent;
      final newParent = after?['parent'] as String?;
      if (oldParent != null && oldParent != newParent && !patch.entities.containsKey(oldParent)) out.add(oldParent);
      if (newParent != null && !patch.entities.containsKey(newParent)) {
        final pn = t[newParent];
        if (pn == null || !pn.children.contains(e.key)) out.add(newParent);
      }
      if (after == null && before != null) {
        // A deleted entity's parent still listed it.
        if (oldParent != null && !patch.entities.containsKey(oldParent)) out.add(oldParent);
      }
    }
    return out.toList();
  }

  void _refreshDiagnostics(HierarchyTree t) {
    try {
      diagnostics = document.validate();
    } on AuthoringException catch (e) {
      diagnostics = e.diagnostics;
    }
    t.setErrors(_errorsByEntity(t));
  }

  /// Error diagnostics of this document keyed by the entity they point at
  /// (`id`, or the `/entities/<id>` prefix of the pointer).
  Map<String, List<Diagnostic>> _errorsByEntity(HierarchyTree t) {
    final out = <String, List<Diagnostic>>{};
    void add(Diagnostic d) {
      if (d.severity != Severity.error) return;
      String? id = d.id;
      if (id == null || !t.nodes.containsKey(id)) {
        final m = RegExp(r'^/entities/([^/]+)').firstMatch(d.pointer ?? '');
        if (m != null) id = m.group(1);
      }
      if (id != null && t.nodes.containsKey(id)) (out[id] ??= []).add(d);
    }

    diagnostics.forEach(add);
    for (final d in project.diagnostics) {
      if (d.document != null && p.equals(d.document!, document.path)) add(d);
    }
    return out;
  }

  /// Every error diagnostic of this document (its own validation plus the
  /// project's records for its file), for the tab's badge.
  int get errorCount {
    final own = diagnostics.where((d) => d.severity == Severity.error).length;
    final fromProject = project.diagnostics
        .where((d) => d.severity == Severity.error && d.document != null && p.equals(d.document!, document.path))
        .length;
    return own + fromProject;
  }

  void _onProject() {
    final t = _tree;
    if (t != null) t.setErrors(_errorsByEntity(t));
    notifyListeners();
  }

  // ── Changes ───────────────────────────────────────────────────────────

  void _onDocument() {
    final edit = document.lastEdit;
    switch (edit?.kind) {
      case DocumentChangeKind.saved:
        _baselineText = _readDisk();
        _baseRevision = document.revision;
        conflict = null;
      case DocumentChangeKind.reloaded:
        _baselineText = _readDisk();
        _baseRevision = document.revision;
        conflict = null;
        if (_tree != null) _buildTree();
      case DocumentChangeKind.applied || DocumentChangeKind.undone || DocumentChangeKind.redone:
        if (edit!.wholeDocument) {
          if (_tree != null) _buildTree();
        } else {
          _patchTree(edit.affectedIds!);
        }
      case null:
        break;
    }
    notifyListeners();
  }

  /// One transaction from the current revision. Returns the record (read
  /// `ok`), or null when the call itself failed; a refusal is in [lastError].
  Applied? transaction(List<Map<String, Object?>> commands) {
    if (document.readOnly != null) {
      lastError = 'Read-only: ${document.readOnly}';
      notifyListeners();
      return null;
    }
    try {
      final a = document.apply(commands, baseRevision: document.revision);
      if (a.ok) {
        lastError = null;
      } else if (a.isConflict) {
        // FYN-E0001: the document moved on since this revision — nothing
        // was touched. Re-read and let the user try again.
        lastError = 'The document changed under this edit (FYN-E0001); nothing was written. Try again.';
        if (_tree != null) _buildTree();
      } else {
        lastError = describeRejection(a);
      }
      notifyListeners();
      return a;
    } on AuthoringException catch (e) {
      lastError = e.toString();
      notifyListeners();
      return null;
    }
  }

  bool undo() => _replay(document.undo);
  bool redo() => _replay(document.redo);

  bool _replay(Replayed Function() f) {
    try {
      f();
      lastError = null;
      return true;
    } on AuthoringException catch (e) {
      if (!e.isNothingToReplay) {
        lastError = e.toString();
        notifyListeners();
      }
      return false;
    }
  }

  // ── Disk ──────────────────────────────────────────────────────────────

  String? _readDisk() {
    try {
      return File(document.path).readAsStringSync();
    } catch (_) {
      return null;
    }
  }

  /// The file watcher saw this path. An unchanged file (the editor's own
  /// save) is ignored; a changed file reloads silently when there are no
  /// local edits and becomes a [conflict] when there are. Returns true when
  /// the tab changed.
  bool checkDisk() {
    final now = _readDisk();
    if (now == _baselineText) return false;
    if (now != null && !document.dirty && conflict == null) {
      try {
        document.reload();
        lastError = null;
      } on AuthoringException catch (e) {
        lastError = 'The file changed on disk but no longer reads: $e';
        _baselineText = now;
        notifyListeners();
      }
      return true;
    }
    conflict = DiskConflict(
      baseRevision: _baseRevision,
      ourRevision: document.revision,
      diskRevision: now == null ? null : project.fileRevision(document.path),
      baselineText: _baselineText,
      diskText: now,
      history: document.history,
      detectedAt: DateTime.now(),
    );
    notifyListeners();
    return true;
  }

  /// Saves in place. Refused (false, with [lastError]) while the file has
  /// changed on disk — the editor never overwrites another writer's work.
  bool save() {
    if (conflict == null) checkDisk();
    if (conflict != null) {
      lastError = conflict!.fileMissing
          ? 'The file is gone from disk; reload to drop the tab\'s edits or save a copy.'
          : 'The file changed on disk since you opened it; reload or save a copy — it is not overwritten.';
      notifyListeners();
      return false;
    }
    return _save(null);
  }

  /// Saves to [absolutePath] (which becomes the tab's path); the conflict, if any, is left behind.
  bool saveAs(String absolutePath) {
    if (_save(absolutePath)) {
      conflict = null;
      notifyListeners();
      return true;
    }
    return false;
  }

  bool _save(String? path) {
    try {
      document.save(path: path);
      lastError = null;
      return true;
    } on AuthoringException catch (e) {
      lastError = e.toString();
      notifyListeners();
      return false;
    }
  }

  /// Revert to saved / reload from disk: history dropped, edits lost.
  bool reload() {
    try {
      document.reload();
      lastError = null;
      return true;
    } on AuthoringException catch (e) {
      lastError = e.toString();
      notifyListeners();
      return false;
    }
  }

  /// Keep the unsaved edits with the conflict still shown on the tab.
  void keepEditing() => notifyListeners();

  // ── Selection ─────────────────────────────────────────────────────────

  bool isSelected(String id) => selection.contains(id);

  /// Click: select; ⌘-click: [toggle]; ⇧-click: [range] from the anchor over
  /// the visible rows.
  void select(String id, {bool toggle = false, bool range = false}) {
    if (range && anchor != null && _tree != null) {
      final rows = tree.rows;
      final a = rows.indexWhere((r) => r.id == anchor);
      final b = rows.indexWhere((r) => r.id == id);
      if (a >= 0 && b >= 0) {
        final lo = a < b ? a : b, hi = a < b ? b : a;
        if (!toggle) selection.clear();
        for (var i = lo; i <= hi; i++) {
          selection.add(rows[i].id);
        }
        lead = id;
        notifyListeners();
        return;
      }
    }
    if (toggle) {
      if (!selection.remove(id)) selection.add(id);
    } else {
      selection
        ..clear()
        ..add(id);
    }
    anchor = id;
    lead = id;
    notifyListeners();
  }

  void setSelection(Iterable<String> ids) {
    selection
      ..clear()
      ..addAll(ids);
    anchor = selection.isEmpty ? null : selection.first;
    lead = selection.isEmpty ? null : selection.last;
    notifyListeners();
  }

  void clearSelection() {
    if (selection.isEmpty) return;
    selection.clear();
    anchor = null;
    lead = null;
    notifyListeners();
  }

  void _pruneSelection() {
    final t = _tree;
    if (t == null) return;
    selection.removeWhere((id) => !t.nodes.containsKey(id));
    if (anchor != null && !t.nodes.containsKey(anchor!)) anchor = null;
    if (lead != null && !t.nodes.containsKey(lead!)) lead = null;
  }

  // ── Lifecycle ─────────────────────────────────────────────────────────

  /// Drops the document (unsaved edits are lost — the tab bar asks first).
  void close() {
    document.removeListener(_onDocument);
    project.removeListener(_onProject);
    document.close();
  }

  @override
  void dispose() {
    document.removeListener(_onDocument);
    project.removeListener(_onProject);
    super.dispose();
  }

  @override
  String toString() => 'OpenDocument($fileName${dirty ? ' •' : ''})';
}
