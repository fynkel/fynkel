// One open document as the editor sees it: the authoring `Editor` behind a
// handle — state, the entity tree, one transaction per gesture, undo/redo,
// save, reload. `SessionDocument` is the real one over `DocumentHandle`;
// the abstract class lets widget tests run the tabs and the Hierarchy on an
// in-memory fake without the native runtime.
import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';

/// Why a document notified: what moved and which entities it touched
/// (`null` ids = "everything", the reload case).
final class DocumentEdit {
  const DocumentEdit(this.kind, {this.affectedIds});

  final DocumentChangeKind kind;
  final List<String>? affectedIds;

  bool get wholeDocument => affectedIds == null;
}

abstract class EditorDocument extends ChangeNotifier {
  /// Absolute path the document was read from (or last saved to).
  String get path;

  /// Path relative to the project's assets directory, when under it.
  String? get relativePath;

  /// `scene`, `prefab` or `material`.
  String get kind;
  String get id;
  String get name;

  /// Content hash now; every transaction names it as its base.
  String get revision;
  bool get dirty;
  bool get canUndo;
  bool get canRedo;

  /// Committed transactions in the history.
  int get history;
  String? get readOnly;
  bool get isClosed;

  /// What the last notification was about.
  DocumentEdit? get lastEdit;

  /// The whole document as JSON (`entities`, `name`, `roots?`, …).
  Map<String, Object?> root();

  /// The named entities and the ordered roots (see `DocumentHandle.entities`).
  EntityPatch entities(Iterable<String> ids);
  List<String> roots();

  /// One transaction. A stale [baseRevision] is refused as a conflict
  /// (`Applied.isConflict`); nothing is touched.
  Applied apply(List<Map<String, Object?>> commands, {String? baseRevision});
  Replayed undo();
  Replayed redo();

  /// Writes the document where it was read from, or to [path]; returns the written path.
  String save({String? path});

  /// Re-reads the file: history dropped, edits lost.
  void reload();
  void close();

  /// The document's diagnostics now (asset references checked).
  List<Diagnostic> validate();

  /// The typed view `fynkel scene|prefab inspect --json` prints.
  Map<String, Object?> inspect();
}

/// The real document over the authoring session.
final class SessionDocument extends EditorDocument {
  SessionDocument(this.handle) {
    _sub = handle.changes.listen(_onChanged);
  }

  final DocumentHandle handle;
  StreamSubscription<DocumentChanged>? _sub;
  DocumentEdit? _lastEdit;

  DocumentState get state => handle.state;

  @override
  String get path => state.path;
  @override
  String? get relativePath => state.relativePath;
  @override
  String get kind => state.kind;
  @override
  String get id => state.id;
  @override
  String get name => state.name;
  @override
  String get revision => state.revision;
  @override
  bool get dirty => state.dirty;
  @override
  bool get canUndo => state.canUndo;
  @override
  bool get canRedo => state.canRedo;
  @override
  int get history => state.history;
  @override
  String? get readOnly => state.readOnly;
  @override
  bool get isClosed => handle.isClosed;
  @override
  DocumentEdit? get lastEdit => _lastEdit;

  void _onChanged(DocumentChanged e) {
    _lastEdit = switch (e.kind) {
      DocumentChangeKind.applied => DocumentEdit(e.kind, affectedIds: e.applied?.affectedIds ?? const []),
      DocumentChangeKind.undone ||
      DocumentChangeKind.redone => DocumentEdit(e.kind, affectedIds: e.replayed?.affectedIds ?? const []),
      DocumentChangeKind.saved => const DocumentEdit(DocumentChangeKind.saved, affectedIds: []),
      DocumentChangeKind.reloaded => const DocumentEdit(DocumentChangeKind.reloaded),
    };
    notifyListeners();
  }

  @override
  Map<String, Object?> root() => handle.root();
  @override
  EntityPatch entities(Iterable<String> ids) => handle.entities(ids);
  @override
  List<String> roots() => handle.roots();
  @override
  Applied apply(List<Map<String, Object?>> commands, {String? baseRevision}) =>
      handle.apply(commands, baseRevision: baseRevision);
  @override
  Replayed undo() => handle.undo();
  @override
  Replayed redo() => handle.redo();
  @override
  String save({String? path}) => handle.save(path: path);
  @override
  void reload() => handle.reload();
  @override
  List<Diagnostic> validate() => handle.validate();
  @override
  Map<String, Object?> inspect() => handle.inspect();

  @override
  void close() {
    _sub?.cancel();
    _sub = null;
    if (!handle.isClosed) handle.close();
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }
}
