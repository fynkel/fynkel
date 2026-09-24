// The Scene view on the running game (ADR-0021 P7 L5): the game's world
// as a `PreviewBackend`, so the Scene view draws it, picks in it and
// outlines its selection with the code it uses for a scene's edit
// preview. The world is the game's: the Scene view never sets its camera
// on it (the viewport carries its own, `FynkelViewportController.view`),
// hides nothing in it and does not step it. A gizmo drag moves the live
// Transforms (L3) and puts them back before the drag's transaction
// writes the result; a resync, a reload or a recook have no meaning here
// and are refused.
import 'dart:async';

import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/editor_document.dart';

final class LivePreview implements PreviewBackend {
  LivePreview(this.live, {this.alive});

  final LiveWorld live;

  /// Whether the game's world still exists. Play's end reaches the editor
  /// a microtask after the game tore its world down — after that frame's
  /// post-frame callbacks, where the Scene view draws — so the view asks
  /// here before every frame instead.
  final bool Function()? alive;
  /// Called after the drag wrote the world (a paused Game view draws it
  /// again).
  void Function()? written;

  /// The Transforms a drag moved, as they were before it.
  final Map<String, Map<String, Object?>> _before = {};
  bool _closed = false;
  PreviewStats _stats = const PreviewStats.zero();

  @override
  int get worldAddress => isClosed ? 0 : live.worldAddress;
  @override
  bool get isClosed => _closed || !(alive?.call() ?? true);
  @override
  PreviewStats get stats => _stats;
  @override
  List<Diagnostic> get diagnostics => const [];
  @override
  CookReport? get cook => null;
  @override
  Stream<PreviewSyncReport> get syncs => const Stream.empty();

  /// The Scene view gives the viewport its camera instead: the game's
  /// world keeps its own.
  @override
  void setCamera(PreviewCamera camera) {}

  /// Nothing hides in a game's world (only an edit world can).
  @override
  int setHidden(Iterable<String> ids) => 0;

  @override
  PreviewPick pick(PreviewCamera camera, double aspect, List<double> ndc) => live.pick(camera, aspect, ndc);
  @override
  List<String> pickRect(PreviewCamera camera, double aspect, List<double> min, List<double> max) =>
      live.pickRect(camera, aspect, min, max);
  @override
  PreviewLines lines({
    Iterable<String> outline = const [],
    List<double>? outlineColor,
    bool outlineBoxes = true,
    double? iconsAspect,
  }) => live.lines(outline: outline, outlineColor: outlineColor, outlineBoxes: outlineBoxes, iconsAspect: iconsAspect);
  @override
  List<int> entities(Iterable<String> ids) => live.handles(ids);
  @override
  Map<String, List<double>> worldMatrices(Iterable<String> ids) => live.worldMatrices(ids);

  @override
  void setTransforms(Iterable<PreviewTransform> transforms) {
    for (final t in transforms) {
      final before = _before[t.id] ??= live.entity(t.id).component(_transform) ?? const {};
      _set(t.id, {...before, 'position': t.position, 'rotation': t.rotation, 'scale': t.scale});
    }
    written?.call();
  }

  @override
  void restoreTransforms(Iterable<String> ids) {
    for (final id in ids) {
      final before = _before.remove(id);
      if (before != null) _set(id, before);
    }
    written?.call();
  }

  /// The Inspector's live edit never previews here: in Play its edits
  /// write the running game directly.
  @override
  int setComponents(Iterable<({String id, String type, Map<String, Object?> data})> components) => 0;
  @override
  void restoreComponents(Iterable<String> ids) {}

  /// A material edited during Play is saved; the running game draws it
  /// from its next Play.
  @override
  void setMaterial(String material, Map<String, Object?> data) {}
  @override
  void restoreMaterial(String material) {}
  @override
  Map<String, Object?> material(String material) => throw _refused('material');
  @override
  void setVolume(String volume, Map<String, Object?> data) {}
  @override
  void restoreVolume(String volume) {}
  @override
  Map<String, Object?> volume(String volume) => throw _refused('volume');

  static const _transform = 'fynkel.Transform';

  void _set(String id, Map<String, Object?> data) {
    final refused = live.set(id, _transform, data);
    if (refused.isNotEmpty) {
      throw AuthoringException('live set', AuthoringStatus.invalidArgument, [
        for (final m in refused) Diagnostic(code: 'FYN-T0003', severity: Severity.error, message: m),
      ]);
    }
  }
  @override
  PreviewSyncReport resync() => throw _refused('resync');
  @override
  PreviewStats reload() => refreshStats();
  @override
  CookReport recook({bool live = false}) => throw _refused('recook');
  @override
  Map<String, Object?> dump() => const {};

  @override
  PreviewStats refreshStats() {
    final n = live.entities().length;
    return _stats = PreviewStats.fromJson({'entities': n, 'live': n, 'tick': live.tick});
  }

  @override
  void close() => _closed = true;

  AuthoringException _refused(String what) => AuthoringException('live $what', AuthoringStatus.invalidArgument, [
    Diagnostic(code: 'FYN-T0003', severity: Severity.error, message: '$what: not on the running game'),
  ]);
}
