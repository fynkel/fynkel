// The edit-preview world (blueprint §19.2e item 6, authoring ABI v3): a
// DocumentHandle's ScenePreview — the fynkel_core world in edit mode the
// Scene view draws, picks in and drags gizmos over. The session keeps it
// equal to the live document after every transaction, undo, redo, reload
// and rescan (the incremental sync rule); the reports arrive as
// PreviewSynced events. Vectors are plain lists here (this package stays
// Flutter- and math-library-free like the rest of it); the editor app
// wraps them in its own types.

part of 'session.dart';

/// A preview's world moved with its document (a transaction, undo, redo,
/// reload, save, rescan or asset operation on this session): the Scene
/// view showing it draws a new frame.
final class PreviewSynced extends AuthoringEvent {
  const PreviewSynced(this.document, this.report);

  /// The document whose preview synced (null when its handle closed
  /// meanwhile).
  final DocumentHandle? document;
  final PreviewSyncReport report;
}

/// The edit-preview world of one scene or prefab document, from
/// [DocumentHandle.openPreview]. Its [worldAddress] is what the viewport
/// renders (`FynkelViewportController.renderHandle`); everything else is a
/// call on the world. Dead after [close], the document's close, the
/// project's close or the session's dispose.
final class ScenePreview {
  ScenePreview._(this.document, this.stats, this.diagnostics, this.cook);

  final DocumentHandle document;

  /// The counts after the last call that returned them.
  PreviewStats stats;

  /// The description's and the load's diagnostics after open, Reload or
  /// recook.
  List<Diagnostic> diagnostics;

  /// The cook that opened the cache, when this preview ran it (the first of
  /// a session), or the last `recook`.
  CookReport? cook;

  bool _closed = false;
  bool get isClosed => _closed;

  AuthoringSession get session => document.session;
  int get handle => document.handle;

  /// The native `FynkelWorld*` for the viewport ABI; 0 when closed.
  int get worldAddress =>
      _closed ? 0 : n.fynkel_authoring_preview_world(session._ptr, handle).address;

  /// The editor camera the picture is drawn through.
  void setCamera(PreviewCamera camera) {
    _call('camera', camera.toJson());
  }

  /// Exactly [ids] and their descendants left out of the picture and of
  /// picking — the editor's hidden flag; an empty list shows everything.
  /// Returns how many entities are hidden.
  int setHidden(Iterable<String> ids) => _call('hidden', {'ids': ids.toList()})['hidden'] as int? ?? 0;

  /// Every entity under the pixel at [ndc] (`[x, y]`, −1..1, +Y up) of
  /// [camera] at [aspect], nearest first.
  PreviewPick pick(PreviewCamera camera, double aspect, List<double> ndc) =>
      PreviewPick.fromJson(_call('pick', {'view': camera.viewJson(aspect), 'ndc': ndc}));

  /// Every entity along the ray from [origin] in [dir] (world space).
  PreviewPick pickRay(List<double> origin, List<double> dir) =>
      PreviewPick.fromJson(_call('pick', {'ray': {'origin': origin, 'dir': dir}}));

  /// The entities whose projected bounds overlap the NDC rectangle [min]…
  /// [max] (the marquee).
  List<String> pickRect(PreviewCamera camera, double aspect, List<double> min, List<double> max) =>
      _strings(_call('pickRect', {'view': camera.viewJson(aspect), 'min': min, 'max': max})['ids']);

  /// The selection outline of [outline] (each id's bounds as twelve edges,
  /// a cross for an entity without a mesh, in [outlineColor] `[r, g, b, a]`
  /// — orange by default) and, with [iconsAspect], the icons of every
  /// light, camera and collider for a viewport of that aspect. With
  /// [outlineBoxes] false only the crosses are drawn — the meshes are
  /// ringed by the renderer's selection outline instead ([entities],
  /// `ViewportOverlay.outline`).
  PreviewLines lines({
    Iterable<String> outline = const [],
    List<double>? outlineColor,
    bool outlineBoxes = true,
    double? iconsAspect,
  }) =>
      PreviewLines.fromJson(_call('lines', {
        if (outline.isNotEmpty)
          'outline': {'ids': outline.toList(), 'color': ?outlineColor, if (!outlineBoxes) 'boxes': false},
        if (iconsAspect != null) 'icons': {'aspect': iconsAspect},
      }));

  /// The renderer's handles (`Entity::to_bits`) of [ids] and, for a
  /// prefab instance's id, of every entity it spawned — what
  /// `ViewportOverlay.outline` takes. An id the world lacks, and an entity
  /// hidden in the editor, has none; each handle comes once.
  List<int> entities(Iterable<String> ids) =>
      [for (final e in _call('entities', {'ids': ids.toList()})['entities'] as List? ?? const []) (e as num).toInt()];

  /// The world matrices of [ids] now (16 numbers column-major each);
  /// throws (`invalidArgument`, FYN-T0003) for an id the world lacks.
  Map<String, List<double>> worldMatrices(Iterable<String> ids) => {
        for (final e in _mapOf(_call('worldMatrix', {'ids': ids.toList()})['matrices']).entries)
          e.key: [for (final x in e.value as List? ?? const []) (x as num).toDouble()],
      };

  /// The gizmo's preview: the local transforms set in the world only — the
  /// document is untouched and stays clean. The drag's end applies one
  /// transaction with the same values, after which the next sync has
  /// nothing to do.
  void setTransforms(Iterable<PreviewTransform> transforms) {
    _call('setTransforms', {'transforms': [for (final t in transforms) t.toJson()]});
  }

  /// The transforms of [ids] back to the document's (a cancelled drag).
  void restoreTransforms(Iterable<String> ids) {
    _call('restoreTransforms', {'ids': ids.toList()});
  }

  /// The Inspector's live edit (a colour being picked): each component's
  /// whole [data] set on the entity in the world only, the document
  /// untouched. A type that is not built in (a behaviour) is skipped;
  /// returns how many were set. The commit's transaction follows with the
  /// same values.
  int setComponents(Iterable<({String id, String type, Map<String, Object?> data})> components) =>
      _call('setComponents', {
        'components': [for (final c in components) {'id': c.id, 'type': c.type, 'data': c.data}],
      })['count'] as int? ?? 0;

  /// The components of [ids] back to the document's (a cancelled edit).
  void restoreComponents(Iterable<String> ids) {
    _call('restoreComponents', {'ids': ids.toList()});
  }

  /// The Inspector's live edit of a material document: [data] (its
  /// `fynkel.pbr@1` fields) drawn in place of the cooked material
  /// [material] in every open preview, the file untouched. The save's
  /// recook brings the cache to the same values.
  void setMaterial(String material, Map<String, Object?> data) {
    _call('setMaterial', {'material': material, 'data': data});
  }

  /// What this preview draws for [material] now — the live edit while one
  /// is set, else the cooked record — as its document fields.
  Map<String, Object?> material(String material) =>
      (_call('material', {'material': material})['material'] as Map).cast<String, Object?>();

  /// The cooked [material] again (a cancelled edit).
  void restoreMaterial(String material) {
    _call('restoreMaterial', {'material': material});
  }

  /// The volume profile Inspector's live edit: [data] (the profile
  /// document's fields) blended in place of the cooked [volume] in every
  /// open preview, the file untouched.
  void setVolume(String volume, Map<String, Object?> data) {
    _call('setVolume', {'volume': volume, 'data': data});
  }

  /// What this preview blends for [volume] now — the live edit while one
  /// is set, else the cooked profile — as its present fields.
  Map<String, Object?> volume(String volume) =>
      (_call('volume', {'volume': volume})['volume'] as Map).cast<String, Object?>();

  /// The cooked [volume] again (a cancelled edit).
  void restoreVolume(String volume) {
    _call('restoreVolume', {'volume': volume});
  }

  /// The incremental sync now (a no-op when the world already matches the
  /// document).
  PreviewSyncReport resync() {
    final r = _call('resync');
    stats = PreviewStats.fromJson(_mapOf(r['stats']));
    return PreviewSyncReport.fromJson(r);
  }

  /// The user's Reload (and the fallback's path): the world emptied and
  /// the document loaded fresh.
  PreviewStats reload() {
    final r = _call('reload');
    stats = PreviewStats.fromJson(_mapOf(r['stats']));
    diagnostics = Diagnostic.listOf(r['diagnostics']);
    return stats;
  }

  /// Cooks the project again (`fynkel cook` in process), reopens the cache
  /// and reloads every preview of the session over it — after a source or
  /// material changed on disk. Project-wide; the report is returned and
  /// kept in [cook]. [live]: the documents with unsaved edits cook as
  /// the editor holds them, their files untouched — what Play runs
  /// (ADR-0021 P5).
  CookReport recook({bool live = false}) {
    final r = _call('recook', {if (live) 'live': true});
    final report = CookReport.fromJson(_mapOf(r['cook']));
    for (final entry in r['previews'] as List? ?? const []) {
      final e = _mapOf(entry);
      final p = session._documents[e['handle']]?._preview;
      if (p == null) continue;
      p.cook = report;
      p.stats = PreviewStats.fromJson(_mapOf(e['stats']));
      p.diagnostics = Diagnostic.listOf(e['diagnostics']);
    }
    return report;
  }

  /// The world as `fynkel state dump` prints it, in document order.
  Map<String, Object?> dump() => _mapOf(_call('dump')['dump']);

  /// The counts now.
  PreviewStats refreshStats() {
    final r = _call('stats');
    diagnostics = Diagnostic.listOf(r['diagnostics']);
    return stats = PreviewStats.fromJson(_mapOf(r['stats']));
  }

  /// Drops the world; the document stays open. [worldAddress] is 0 from
  /// here on — stop rendering it first.
  void close() {
    if (_closed) return;
    session._call('previewClose', () => n.fynkel_authoring_preview_close(session._ptr, handle));
    _closed = true;
    document._preview = null;
  }

  Map<String, Object?> _call(String kind, [Map<String, Object?> args = const {}]) {
    document._check();
    if (_closed) throw StateError('the preview of document $handle is closed');
    return session._kindAndArgs(
      'preview $kind',
      kind,
      args,
      (ptr, k, kl, a, al) => n.fynkel_authoring_preview_call(ptr, handle, k, kl, a, al),
    );
  }
}
