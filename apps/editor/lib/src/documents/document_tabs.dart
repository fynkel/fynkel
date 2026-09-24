// The open documents and which one is focused. One tab per document
// (opening a document twice focuses its tab); the focused tab is what the
// Hierarchy shows and what ⌘S, ⌘Z and ⇧⌘Z act on. The tabs listen to the
// project's file watcher and hand each changed path to its tab. A prefab
// tab's content change is passed to every other tab (Prefab mode: the
// instances of that prefab show its unsaved edits live). A prefab opened
// from an instance remembers where (Unity's Prefab Mode in Context: the
// breadcrumbs, the scene drawn around it), and with Auto Save each change
// to a prefab is saved as it is made.
import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:path/path.dart' as p;

import '../project/editor_project.dart';
import 'editor_document.dart';
import 'open_document.dart';

final class DocumentTabs extends ChangeNotifier {
  DocumentTabs({required this.project}) {
    _files = project.fileChanges.listen(_onFiles);
  }

  final EditorProject project;
  final List<OpenDocument> open = [];
  OpenDocument? _focused;
  StreamSubscription<List<String>>? _files;

  /// The last failed open, as text.
  String? openError;

  OpenDocument? get focused => _focused;

  OpenDocument? _live;

  /// The running game while Play is up (ADR-0021 P7): not a tab — it is
  /// never saved, closed or cycled through — but what the Hierarchy and
  /// the Inspector show ([shown]). The shell sets it and disposes it.
  OpenDocument? get live => _live;
  set live(OpenDocument? tab) {
    if (identical(tab, _live)) return;
    _live = tab;
    notifyListeners();
  }

  /// What the Hierarchy and the Inspector follow: the running game during
  /// Play, the focused tab otherwise.
  OpenDocument? get shown => _live ?? _focused;
  bool get isEmpty => open.isEmpty;
  bool get anyDirty => open.any((d) => d.dirty);
  List<OpenDocument> get dirtyDocuments => [
    for (final d in open)
      if (d.dirty) d,
  ];

  /// Opens (or focuses, unless [focus] is false) a document by path or
  /// asset id — `<id>#prefab` for an imported hierarchy, read-only; null
  /// with [openError] set when the session refuses it. A prefab focused
  /// with [context] is in Prefab Mode in Context; focused without one, in
  /// isolation (Unity's, from the Project). Opened in the background it
  /// keeps whichever it had.
  OpenDocument? openDocument(String pathOrId, {bool focus = true, PrefabContext? context}) {
    final EditorDocument doc;
    try {
      doc = project.openDocument(pathOrId);
    } on AuthoringException catch (e) {
      final reasons = e.diagnostics.map((d) => '${d.code}: ${d.message}').join('\n');
      openError = 'Could not open $pathOrId.\n${reasons.isEmpty ? e : reasons}';
      notifyListeners();
      return null;
    }
    openError = null;
    var tab = open.where((o) => identical(o.document, doc) || o.document.id == doc.id).firstOrNull;
    if (tab == null) {
      tab = OpenDocument(doc, project: project);
      tab.addListener(() => _onTab(tab!));
      open.add(tab);
      if (!focus) notifyListeners();
    }
    if (focus && tab.kind == 'prefab') {
      // Not in a context of its own making (a prefab inside itself).
      final c = context != null && open.contains(context.tab) && !trail(context.tab).contains(tab) ? context : null;
      if (c == null) {
        _contexts.remove(tab);
      } else {
        _contexts[tab] = c;
      }
    }
    if (focus) this.focus(tab);
    return tab;
  }

  // ── Prefab mode ─────────────────────────────────────────────────────

  final _contexts = <OpenDocument, PrefabContext>{};

  /// Where the prefab [tab] was opened from, when it was from an instance.
  PrefabContext? contextOf(OpenDocument tab) => _contexts[tab];

  /// The breadcrumbs of [tab]: the tabs it was opened from, outermost
  /// first, then itself (a scene, the prefab opened from an instance in it,
  /// a prefab opened from one inside that…).
  List<OpenDocument> trail(OpenDocument tab) {
    final out = [tab];
    for (var c = _contexts[tab]; c != null && !out.contains(c.tab); c = _contexts[c.tab]) {
      out.insert(0, c.tab);
    }
    return out;
  }

  /// Unity's Auto Save in Prefab Mode: a prefab tab is saved after each
  /// change (a transaction, an undo, a redo, an Apply from an instance).
  /// The shell sets it from the editor state (on unless turned off).
  bool get autoSavePrefabs => _autoSave;
  bool _autoSave = false;
  set autoSavePrefabs(bool on) {
    if (on == _autoSave) return;
    _autoSave = on;
    if (on) {
      for (final t in open) {
        _autoSaveSoon(t);
      }
    }
    notifyListeners();
  }

  /// Why the last automatic save did not happen (a conflict on disk).
  String? autoSaveError;

  final _saving = <OpenDocument>{};

  void _autoSaveSoon(OpenDocument tab) {
    if (!_autoSave || tab.kind != 'prefab' || !tab.dirty || tab.document.readOnly != null) return;
    // After the change has reached every listener: saving is a change too.
    if (!_saving.add(tab)) return;
    scheduleMicrotask(() {
      _saving.remove(tab);
      if (!open.contains(tab) || !_autoSave || !tab.dirty) return;
      final ok = tab.save();
      final error = ok ? null : tab.lastError;
      if (error != autoSaveError) {
        autoSaveError = error;
        notifyListeners();
      }
    });
  }

  /// The open tab of the document with [id], if any.
  OpenDocument? byId(String id) => open.where((o) => o.document.id == id).firstOrNull;

  OpenDocument? byPath(String path) => open.where((o) => p.equals(o.path, path)).firstOrNull;

  void focus(OpenDocument? tab) {
    if (tab != null && !open.contains(tab)) return;
    if (_focused == tab) {
      notifyListeners();
      return;
    }
    _focused = tab;
    notifyListeners();
  }

  void focusIndex(int i) {
    if (open.isEmpty) return;
    focus(open[i.clamp(0, open.length - 1)]);
  }

  void focusNext() => _step(1);
  void focusPrevious() => _step(-1);

  void _step(int delta) {
    if (open.length < 2 || _focused == null) return;
    focus(open[(open.indexOf(_focused!) + delta) % open.length]);
  }

  /// Closes the tab without asking (the tab bar asks about unsaved edits).
  void closeDiscarding(OpenDocument tab) {
    final i = open.indexOf(tab);
    if (i < 0) return;
    open.removeAt(i);
    _lastPrefabEdit.remove(tab);
    // Its prefab mode ends (back where it was opened from); prefabs opened
    // from it are in isolation now.
    final back = _contexts.remove(tab)?.tab;
    _contexts.removeWhere((_, c) => identical(c.tab, tab));
    tab.close();
    tab.dispose();
    if (_focused == tab) {
      _focused = back != null && open.contains(back) ? back : (open.isEmpty ? null : open[i.clamp(0, open.length - 1)]);
    }
    // A closed prefab tab's unsaved edits are gone from the instances too.
    if (tab.kind == 'prefab') _prefabChanged(tab);
    notifyListeners();
  }

  /// Saves every dirty tab that can be saved; the ones with a conflict stay dirty.
  List<OpenDocument> saveAll() => [
    for (final d in dirtyDocuments)
      if (!d.save()) d,
  ];

  void _onTab(OpenDocument tab) {
    final edit = tab.document.lastEdit;
    if (tab.kind == 'prefab' && edit != null && edit.kind != DocumentChangeKind.moved) {
      // Only a content change (a transaction, undo, redo, reload — and a
      // save, which drops nothing but marks the moment) reaches the others.
      if (edit.kind != DocumentChangeKind.saved && !identical(_lastPrefabEdit[tab], edit)) {
        _lastPrefabEdit[tab] = edit;
        _prefabChanged(tab);
      }
    }
    _autoSaveSoon(tab);
    notifyListeners();
  }

  final _lastPrefabEdit = <OpenDocument, DocumentEdit?>{};

  /// Prefab-document changes the other tabs were told about (tests read it).
  int prefabChangeBroadcasts = 0;

  void _prefabChanged(OpenDocument prefab) {
    prefabChangeBroadcasts++;
    for (final other in open) {
      if (!identical(other, prefab)) other.onPrefabDocumentChanged();
    }
  }

  void _onFiles(List<String> paths) {
    var foreign = false;
    for (final path in paths) {
      final tab = open.where((t) => p.equals(t.path, path)).firstOrNull;
      if (tab != null) {
        tab.checkDisk();
      } else {
        foreign = true;
      }
    }
    // A file that is no tab's own changed: a prefab or a model source an
    // instance may come from — the cached expansions are re-read.
    if (foreign) {
      for (final tab in List.of(open)) {
        tab.onPrefabDocumentChanged();
      }
    }
  }

  @override
  void dispose() {
    _files?.cancel();
    for (final tab in open) {
      tab.close();
      tab.dispose();
    }
    open.clear();
    _focused = null;
    _live?.close();
    _live?.dispose();
    _live = null;
    super.dispose();
  }
}

/// Where a prefab tab was opened from: the tab (a scene, or a prefab in
/// Prefab mode) and the instance in it (Unity's Prefab Mode in Context).
final class PrefabContext {
  const PrefabContext(this.tab, this.instance);

  final OpenDocument tab;

  /// The instance's root entity id in [tab].
  final String instance;
}
