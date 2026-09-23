// Live Play mode's switch (ADR-0021 P7 L2): while the game runs, the
// Hierarchy and the Inspector show its world (a `PlayDocument` as the
// tabs' `live` document); on Stop they return to the scene tab. The
// selection goes with them both ways, for every entity that came from the
// scene — Unity's rule.
import 'package:fynkel/fynkel.dart' as fynkel;

import '../documents/document_tabs.dart';
import '../documents/open_document.dart';
import 'play_document.dart';

final class LivePlay {
  LivePlay({required this.tabs, LiveBackend Function(fynkel.World world)? backend})
    : _backend = backend ?? WorldLiveBackend.new;

  final DocumentTabs tabs;
  final LiveBackend Function(fynkel.World world) _backend;

  fynkel.World? _world;

  /// The scene tab Play started from (its selection comes back to it).
  OpenDocument? _scene;

  PlayDocument? get document => tabs.live?.document as PlayDocument?;

  /// Shows [world] — the running game's, or null when none runs.
  void follow(fynkel.World? world) {
    if (identical(world, _world)) return;
    _end();
    if (world != null) _begin(world);
  }

  void _begin(fynkel.World world) {
    final focused = tabs.focused;
    final scene = focused != null && focused.kind == 'scene' ? focused : null;
    final doc = PlayDocument(backend: _backend(world), scene: scene?.document);
    final live = OpenDocument(doc, project: tabs.project);
    doc.watched = () => live.selection;
    final carried = [
      for (final id in scene?.selection ?? const <String>{})
        if (live.tree.contains(id)) id,
    ];
    if (carried.isNotEmpty) live.setSelection(carried);
    _world = world;
    _scene = scene;
    tabs.live = live;
  }

  void _end() {
    final live = tabs.live;
    final scene = _scene;
    _world = null;
    _scene = null;
    if (live == null) return;
    if (scene != null && tabs.open.contains(scene)) {
      final back = [
        for (final id in live.selection)
          if (scene.tree.contains(id)) id,
      ];
      if (back.isNotEmpty) scene.setSelection(back);
    }
    tabs.live = null;
    live.close();
    live.dispose();
  }

  void dispose() => _end();
}
