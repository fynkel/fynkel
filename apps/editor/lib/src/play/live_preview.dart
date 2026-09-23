// The Scene view on the running game (ADR-0021 P7 L5): the game's world
// as a `PreviewBackend`, so the Scene view draws it, picks in it and
// outlines its selection with the code it uses for a scene's edit
// preview. The world is the game's: the Scene view never sets its camera
// on it (the viewport carries its own, `FynkelViewportController.view`),
// hides nothing in it and does not step it; a drag's transform preview,
// a resync, a reload or a recook have no meaning here and are refused.
import 'dart:async';

import 'package:fynkel_authoring/fynkel_authoring.dart';

import '../documents/editor_document.dart';

final class LivePreview implements PreviewBackend {
  LivePreview(this.live, {bool Function()? alive}) : _alive = alive;

  final LiveWorld live;

  /// Whether the game's world still exists. Play's end reaches the editor
  /// a microtask after the game tore its world down — after that frame's
  /// post-frame callbacks, where the Scene view draws — so the view asks
  /// here before every frame instead.
  final bool Function()? _alive;
  bool _closed = false;
  PreviewStats _stats = const PreviewStats.zero();

  @override
  int get worldAddress => isClosed ? 0 : live.worldAddress;
  @override
  bool get isClosed => _closed || !(_alive?.call() ?? true);
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
  PreviewLines lines({Iterable<String> outline = const [], List<double>? outlineColor, double? iconsAspect}) =>
      live.lines(outline: outline, outlineColor: outlineColor, iconsAspect: iconsAspect);
  @override
  Map<String, List<double>> worldMatrices(Iterable<String> ids) => live.worldMatrices(ids);

  @override
  void setTransforms(Iterable<PreviewTransform> transforms) => throw _refused('setTransforms');
  @override
  void restoreTransforms(Iterable<String> ids) => throw _refused('restoreTransforms');
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
