// The open documents and which one is focused. One tab per document
// (opening a document twice focuses its tab); the focused tab is what the
// Hierarchy shows and what ⌘S, ⌘Z and ⇧⌘Z act on. The tabs listen to the
// project's file watcher and hand each changed path to its tab.
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
  bool get isEmpty => open.isEmpty;
  bool get anyDirty => open.any((d) => d.dirty);
  List<OpenDocument> get dirtyDocuments => [
    for (final d in open)
      if (d.dirty) d,
  ];

  /// Opens (or focuses) a document by path or asset id; null with
  /// [openError] set when the session refuses it.
  OpenDocument? openDocument(String pathOrId) {
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
      tab.addListener(_onTab);
      open.add(tab);
    }
    focus(tab);
    return tab;
  }

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
    tab.removeListener(_onTab);
    tab.close();
    tab.dispose();
    if (_focused == tab) _focused = open.isEmpty ? null : open[i.clamp(0, open.length - 1)];
    notifyListeners();
  }

  /// Saves every dirty tab that can be saved; the ones with a conflict stay dirty.
  List<OpenDocument> saveAll() => [
    for (final d in dirtyDocuments)
      if (!d.save()) d,
  ];

  void _onTab() => notifyListeners();

  void _onFiles(List<String> paths) {
    for (final tab in List.of(open)) {
      if (paths.any((c) => p.equals(c, tab.path))) tab.checkDisk();
    }
  }

  @override
  void dispose() {
    _files?.cancel();
    for (final tab in open) {
      tab.removeListener(_onTab);
      tab.close();
      tab.dispose();
    }
    open.clear();
    _focused = null;
    super.dispose();
  }
}
