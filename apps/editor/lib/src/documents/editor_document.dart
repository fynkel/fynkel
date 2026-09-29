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

  /// Why the document cannot be edited: the project's reason, or — for an
  /// imported hierarchy — the source its entities are derived from.
  String? get readOnly;

  /// `prefab` when this is an imported hierarchy opened as `<id>#prefab`
  /// (a model source's or a rig document's node tree, read-only, under the
  /// derived ids its instances use); null for a document of the project.
  String? get sub;
  bool get isImportedHierarchy => sub != null;
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

  /// Prefab mode (item 7): the expansion of the prefab instance anchored
  /// at [anchor] — its derived entities with the overrides applied and
  /// marked, the prefab it comes from, why it did not expand when it did
  /// not. Live: an open prefab document contributes its unsaved edits.
  /// Not an instance: `AuthoringException` FYN-E0008; unknown: FYN-E0002.
  PrefabExpansion instance(String anchor);

  /// The edit-preview world of this scene or prefab (item 6), opened on
  /// first use; a material has none (`AuthoringException`, FYN-T0003).
  PreviewBackend openPreview();

  /// The preview while it is open.
  PreviewBackend? get preview;
}

/// The edit-preview world as the Scene view uses it — `ScenePreview` behind
/// a `SessionDocument`, an in-memory fake in the widget tests. Vectors are
/// plain lists like the package's.
abstract class PreviewBackend {
  /// The native `FynkelWorld*` the viewport renders; 0 once closed.
  int get worldAddress;
  bool get isClosed;

  /// The counts after the last call that returned them.
  PreviewStats get stats;

  /// The load's and the description's diagnostics.
  List<Diagnostic> get diagnostics;

  /// The cook that opened the cache or the last rebuild, when this
  /// preview ran it.
  CookReport? get cook;

  /// One report per sync the session ran for this preview after a
  /// transaction, undo, redo, reload or rescan — the incremental sync
  /// rule at work, `reloaded` when it fell back to a fresh load.
  Stream<PreviewSyncReport> get syncs;

  void setCamera(PreviewCamera camera);
  int setHidden(Iterable<String> ids);
  PreviewPick pick(PreviewCamera camera, double aspect, List<double> ndc);
  List<String> pickRect(PreviewCamera camera, double aspect, List<double> min, List<double> max);
  PreviewLines lines({
    Iterable<String> outline = const [],
    List<double>? outlineColor,
    bool outlineBoxes = true,
    double? iconsAspect,
  });

  /// The renderer's handles of [ids] (with a prefab instance's spawned
  /// entities) for the selection outline (`ViewportOverlay.outline`).
  List<int> entities(Iterable<String> ids);
  Map<String, List<double>> worldMatrices(Iterable<String> ids);
  void setTransforms(Iterable<PreviewTransform> transforms);
  void restoreTransforms(Iterable<String> ids);
  int setComponents(Iterable<({String id, String type, Map<String, Object?> data})> components);
  void restoreComponents(Iterable<String> ids);

  /// The Inspector's live material edit, in every open preview; see
  /// [ScenePreview.setMaterial].
  void setMaterial(String material, Map<String, Object?> data);
  void restoreMaterial(String material);

  /// What this preview draws for [material] now, as its document fields.
  Map<String, Object?> material(String material);

  /// The Inspector's live volume profile edit; see [ScenePreview.setVolume].
  void setVolume(String volume, Map<String, Object?> data);
  void restoreVolume(String volume);

  /// What this preview blends for [volume] now, as its present fields.
  Map<String, Object?> volume(String volume);
  PreviewSyncReport resync();
  PreviewStats reload();
  /// [live]: unsaved documents cook as the editor holds them (Play).
  CookReport recook({bool live = false});

  /// [recook] on the session's cook thread (ADR-0040 §3): returns at once;
  /// [CookRun.finish] applies it.
  CookRun startCook({bool live = false});

  /// Opened over a cache that may be stale or missing, without a cook
  /// (ADR-0040 §4): the Scene view starts one.
  bool get needsCook;
  Map<String, Object?> dump();
  PreviewStats refreshStats();
  void close();
}

/// A cook running on the session's thread ([PreviewBackend.startCook]):
/// the package's `CookJob`, a scripted one in the widget tests.
abstract class CookRun {
  /// Assets finished and in all, and the asset last started, as of the
  /// last [poll].
  int get done;
  int get total;
  String get current;

  /// Reads how far it got; true once it ended.
  bool poll();

  /// Stops it at its next asset; the cache stays as it was.
  void cancel();

  /// Applies the ended cook (every preview reloads over it) → its report.
  CookReport finish();
}

/// The package's [CookJob] as a [CookRun].
final class SessionCookRun implements CookRun {
  SessionCookRun(this.job);

  final CookJob job;

  @override
  int get done => job.done;
  @override
  int get total => job.total;
  @override
  String get current => job.current;
  @override
  bool poll() => job.poll();
  @override
  void cancel() => job.cancel();
  @override
  CookReport finish() => job.finish();
}

/// [ScenePreview] as a [PreviewBackend].
final class SessionPreview implements PreviewBackend {
  SessionPreview(this.preview);

  final ScenePreview preview;

  @override
  int get worldAddress => preview.worldAddress;
  @override
  bool get isClosed => preview.isClosed;
  @override
  PreviewStats get stats => preview.stats;
  @override
  List<Diagnostic> get diagnostics => preview.diagnostics;
  @override
  CookReport? get cook => preview.cook;
  @override
  Stream<PreviewSyncReport> get syncs => preview.session.events
      .where((e) => e is PreviewSynced && e.document == preview.document)
      .map((e) => (e as PreviewSynced).report);
  @override
  void setCamera(PreviewCamera camera) => preview.setCamera(camera);
  @override
  int setHidden(Iterable<String> ids) => preview.setHidden(ids);
  @override
  PreviewPick pick(PreviewCamera camera, double aspect, List<double> ndc) => preview.pick(camera, aspect, ndc);
  @override
  List<String> pickRect(PreviewCamera camera, double aspect, List<double> min, List<double> max) =>
      preview.pickRect(camera, aspect, min, max);
  @override
  PreviewLines lines({
    Iterable<String> outline = const [],
    List<double>? outlineColor,
    bool outlineBoxes = true,
    double? iconsAspect,
  }) => preview.lines(
    outline: outline,
    outlineColor: outlineColor,
    outlineBoxes: outlineBoxes,
    iconsAspect: iconsAspect,
  );
  @override
  List<int> entities(Iterable<String> ids) => preview.entities(ids);
  @override
  Map<String, List<double>> worldMatrices(Iterable<String> ids) => preview.worldMatrices(ids);
  @override
  void setTransforms(Iterable<PreviewTransform> transforms) => preview.setTransforms(transforms);
  @override
  void restoreTransforms(Iterable<String> ids) => preview.restoreTransforms(ids);
  @override
  int setComponents(Iterable<({String id, String type, Map<String, Object?> data})> components) =>
      preview.setComponents(components);
  @override
  void restoreComponents(Iterable<String> ids) => preview.restoreComponents(ids);
  @override
  void setMaterial(String material, Map<String, Object?> data) => preview.setMaterial(material, data);
  @override
  void restoreMaterial(String material) => preview.restoreMaterial(material);
  @override
  Map<String, Object?> material(String material) => preview.material(material);
  @override
  void setVolume(String volume, Map<String, Object?> data) => preview.setVolume(volume, data);
  @override
  void restoreVolume(String volume) => preview.restoreVolume(volume);
  @override
  Map<String, Object?> volume(String volume) => preview.volume(volume);
  @override
  PreviewSyncReport resync() => preview.resync();
  @override
  PreviewStats reload() => preview.reload();
  @override
  CookReport recook({bool live = false}) => preview.recook(live: live);
  @override
  CookRun startCook({bool live = false}) => SessionCookRun(preview.startCook(live: live));
  @override
  bool get needsCook => preview.needsCook;
  @override
  Map<String, Object?> dump() => preview.dump();
  @override
  PreviewStats refreshStats() => preview.refreshStats();
  @override
  void close() => preview.close();
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
  String? get sub => state.sub;
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
      // An asset operation moved the file: the path changed, the content did not.
      DocumentChangeKind.moved => const DocumentEdit(DocumentChangeKind.moved, affectedIds: []),
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
  PrefabExpansion instance(String anchor) => handle.instance(anchor);

  SessionPreview? _preview;

  @override
  PreviewBackend openPreview() {
    final open = _preview;
    if (open != null && !open.isClosed) return open;
    // No cook here (ADR-0040 §4): the Scene view cooks on the session's
    // thread when the preview says it needs one.
    return _preview = SessionPreview(handle.openPreview(cook: false));
  }

  @override
  PreviewBackend? get preview => _preview == null || _preview!.isClosed ? null : _preview;

  @override
  void close() {
    _sub?.cancel();
    _sub = null;
    _preview = null;
    if (!handle.isClosed) handle.close();
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }
}
