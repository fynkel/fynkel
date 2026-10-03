// Live Play mode's switch (ADR-0021 P7 L2): while the game runs, the
// Hierarchy and the Inspector show its world (a `PlayDocument` as the
// tabs' `live` document); on Stop they return to the scene tab. The
// selection goes with them both ways, for every entity that came from the
// scene — Unity's rule. A game that loads the scene a frame or more after
// its world shows up (the reference game's Menu → Playground) gets the
// selection when those entities appear, unless something else was
// selected first.
import 'package:fynkel/gameplay.dart' as fynkel;
import 'package:fynkel_authoring/fynkel_authoring.dart' show AuthoringException;

import '../documents/document_tabs.dart';
import '../documents/open_document.dart';
import 'play_document.dart';

final class LivePlay {
  LivePlay({required this.tabs, LiveBackend Function(fynkel.World world)? backend, this.written})
    : _backend = backend ?? WorldLiveBackend.new;

  final DocumentTabs tabs;

  /// Called after an edit wrote the running world (the app redraws a
  /// paused Game view: `FynkelPlayControl.redraw`).
  final void Function()? written;
  final LiveBackend Function(fynkel.World world) _backend;

  fynkel.World? _world;

  /// The scene tab Play started from (its selection comes back to it).
  OpenDocument? _scene;

  /// The scene's selection, waiting for the running world to have it.
  List<String>? _wanted;

  PlayDocument? get document => tabs.live?.document as PlayDocument?;

  /// A data asset's Inspector edit, saved (ADR-0027 §7): the running
  /// game's shared instance takes [data] too, as Unity's does. False when
  /// no game runs or it has not made that asset yet.
  bool dataEdited(String id, Map<String, Object?> data) {
    final world = _world;
    if (world == null) return false;
    final applied = world.applyDataEdit(id, data);
    if (applied) written?.call();
    return applied;
  }

  /// A material or volume profile's Inspector edit (saved at once, as in
  /// Unity): the running game draws it too, from its next frame — Unity's
  /// material asset changes in Play the same way. False when no game runs
  /// or [kind] is not drawn.
  ///
  /// A material the game's code changes (`setMaterialValue`) is drawn from
  /// the code's copy, which takes the edit with the code's values on top
  /// ([fynkel.World.applyMaterialEdit]) — Unity's rule too.
  bool assetEdited(String kind, String id, Map<String, Object?> fields) {
    final world = _world;
    final doc = document;
    if (world == null || doc == null) return false;
    if (kind == 'material') {
      try {
        world.applyMaterialEdit(id, fields);
      } on ArgumentError {
        // A value the code set no longer fits the saved material: the
        // code's copy keeps its last good values.
      }
    }
    final applied = doc.backend.setAsset(kind, id, fields);
    if (applied) written?.call();
    return applied;
  }

  /// The running object the live document names [id] (an authoring id,
  /// or `runtime:<handle>`), or null: no game runs, or it is gone.
  fynkel.GameObject? objectOf(String id) {
    final world = _world;
    final backend = document?.backend;
    if (world == null || backend is! WorldLiveBackend) return null;
    try {
      final handle = backend.live.entity(id).handle;
      return world.byEntity(handle) ?? world.attach(handle);
    } on AuthoringException {
      return null;
    }
  }

  /// Shows [world] — the running game's, or null when none runs.
  void follow(fynkel.World? world) {
    if (identical(world, _world)) return;
    _end();
    if (world != null) _begin(world);
  }

  void _begin(fynkel.World world) {
    final focused = tabs.focused;
    final scene = focused != null && focused.kind == 'scene' ? focused : null;
    final doc = PlayDocument(backend: _backend(world), scene: scene?.document, written: written);
    final live = OpenDocument(doc, project: tabs.project);
    doc.watched = () => live.selection;
    final wanted = [...?scene?.selection];
    _world = world;
    _scene = scene;
    _wanted = wanted.isEmpty ? null : wanted;
    _carry(live);
    if (_wanted != null) live.addListener(_onLive);
    tabs.live = live;
  }

  /// Selects what the scene had selected, once the running world has some
  /// of it; gives up when something else gets selected first.
  void _carry(OpenDocument live) {
    final wanted = _wanted;
    if (wanted == null) return;
    if (live.selection.isNotEmpty) {
      _wanted = null;
      return;
    }
    final carried = [
      for (final id in wanted)
        if (live.tree.contains(id)) id,
    ];
    if (carried.isEmpty) return;
    _wanted = null;
    live.setSelection(carried);
  }

  void _onLive() {
    final live = tabs.live;
    if (live == null) return;
    _carry(live);
    if (_wanted == null) live.removeListener(_onLive);
  }

  void _end() {
    final live = tabs.live;
    final scene = _scene;
    _world = null;
    _scene = null;
    _wanted = null;
    if (live == null) return;
    live.removeListener(_onLive);
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
