// The Game view between runs (ADR-0021 P4b): what the scene's active
// camera sees, live, as in Unity. It is the Scene view's preview world of
// the focused scene — the same world, no second load — drawn through the
// camera the game would start with, in its own viewport with no grid, no
// gizmos and no icons. It draws again when a view changes the world (an
// edit's sync, a drag's preview transforms, a reload), when the focused
// document changes, and when its viewport attaches or resizes; while its
// tab is hidden it draws nothing (a hidden tab is never painted) and
// catches up when shown.
//
// The active camera is the runtime's rule: the enabled `fynkel.Camera`
// with the highest `priority`, the first in the document on a tie. A
// camera a script moves at start-up is shown where the scene puts it.

import 'dart:async';

import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel_authoring/fynkel_authoring.dart';
import 'package:fynkel_flutter/fynkel_flutter.dart';

import '../documents/document_tabs.dart';
import '../documents/editor_document.dart';
import '../documents/open_document.dart';
import '../scene_view/scene_view_model.dart';

/// The scene's active camera: its entity, name and `fynkel.Camera` data.
typedef ActiveCamera = ({String id, String name, Map<String, Object?> data});

/// The camera a game would render the scene through, or null.
ActiveCamera? activeCamera(OpenDocument tab) {
  ActiveCamera? best;
  var bestPriority = 0;
  for (final node in tab.tree.nodes.values) {
    if (!node.hasComponent('fynkel.Camera')) continue;
    final Object? components;
    try {
      components = tab.document.entities([node.id]).entities[node.id]?['components'];
    } on AuthoringException {
      continue;
    }
    if (components is! Map) continue;
    for (final c in components.values) {
      if (c is! Map || c['type'] != 'fynkel.Camera') continue;
      if (c['enabled'] == false) continue;
      final data = c['data'] is Map<String, Object?> ? c['data'] as Map<String, Object?> : <String, Object?>{};
      final priority = data['priority'] is num ? (data['priority'] as num).toInt() : 0;
      if (best == null || priority > bestPriority) {
        best = (id: node.id, name: node.name, data: data);
        bestPriority = priority;
      }
    }
  }
  return best;
}

/// The camera's lens and its world placement, as the preview takes it.
PreviewCamera previewCameraOf(ActiveCamera camera, List<double> world) {
  final d = camera.data;
  double? n(String k) => d[k] is num ? (d[k] as num).toDouble() : null;
  final ortho = d['projection'] == 'orthographic';
  return PreviewCamera(
    world: world,
    orthographic: ortho,
    fovY: n('fov_y') ?? 1.0471975511965976,
    orthoHeight: n('ortho_height') ?? 10,
    near: n('near') ?? 0.1,
    far: n('far') ?? 1000,
    manualExposure: d['exposure_mode'] == 'manual' ? n('exposure') : null,
  );
}

/// Draws the focused scene through its active camera into [controller].
final class GamePreview extends ChangeNotifier {
  GamePreview({required this.tabs, required this.scene, required this.views, FynkelViewportController? controller})
    : controller = controller ?? FynkelViewportController(placeholders: false) {
    tabs.addListener(_onTabs);
    views?.worldListeners.add(_onWorld);
    this.controller.addListener(invalidate);
    _follow();
  }

  final DocumentTabs tabs;

  /// The Scene view whose worlds this draws (the active copy).
  final SceneViewModel? Function() scene;
  final SceneViews? views;
  final FynkelViewportController controller;

  /// Why nothing is drawn (no scene open, no camera in it); null while
  /// the picture is the camera's.
  String? message = 'Open a scene to see what its camera sees.';

  /// The camera drawn, for the panel's caption.
  ActiveCamera? camera;

  /// Frames drawn (tests, and a check that a hidden tab draws nothing).
  int frames = 0;

  bool _dirty = true;
  bool _scheduled = false;
  bool _disposed = false;
  PreviewBackend? _preview;
  SceneViewModel? _model;
  StreamSubscription<Object?>? _syncs;

  /// Something changed: the next paint of the panel draws again.
  void invalidate() {
    if (_disposed) return;
    _dirty = true;
    notifyListeners();
  }

  void _onWorld(PreviewBackend? preview) {
    if (preview == null || preview == _preview) invalidate();
  }

  void _onTabs() {
    _follow();
    invalidate();
  }

  /// Keeps listening to the focused scene's world and to the Scene view
  /// that opens it (the preview opens after the tab focuses).
  void _follow() {
    final model = scene();
    if (!identical(model, _model)) {
      _model?.removeListener(_onModel);
      _model = model;
      model?.addListener(_onModel);
    }
    final p = model?.current?.preview;
    if (identical(p, _preview)) return;
    _preview = p;
    unawaited(_syncs?.cancel());
    _syncs = p?.syncs.listen((_) => invalidate());
  }

  void _onModel() {
    final before = _preview;
    _follow();
    if (!identical(before, _preview)) invalidate();
  }

  /// The panel was painted (it is on screen): draw after this frame when
  /// something changed.
  void painted() {
    if (!_dirty || _scheduled || _disposed) return;
    _scheduled = true;
    SchedulerBinding.instance.addPostFrameCallback((_) {
      _scheduled = false;
      if (!_disposed) render();
    });
    SchedulerBinding.instance.ensureVisualUpdate();
  }

  /// Draws now. Public for tests.
  void render() {
    _follow();
    final t = scene()?.current;
    String? why;
    ActiveCamera? cam;
    if (t == null || t.tab.kind != 'scene') {
      why = 'Open a scene to see what its camera sees.';
    } else if (!t.hasPreview) {
      why = t.openError ?? 'The scene has no preview yet.';
    } else {
      cam = activeCamera(t.tab);
      if (cam == null) why = '${t.tab.document.name} has no camera. Add one (a Camera component) to see the game here.';
    }
    if (why == null) {
      final p = t!.preview!;
      final world = p.worldMatrices([cam!.id])[cam.id];
      if (world == null) {
        why = 'The camera ${cam.name} is not in the world.';
      } else if (controller.isAttached) {
        p.setCamera(previewCameraOf(cam, world));
        releaseWorldCamera(p);
        controller.overlay(ViewportOverlay());
        final slot = controller.renderHandle(p.worldAddress);
        if (slot == null && controller.state == ViewportState.resizing) {
          // Asked again once the ring is rebuilt (the controller notifies).
          _changed(why, cam);
          return;
        }
        frames++;
      }
    }
    _dirty = false;
    _changed(why, cam);
  }

  void _changed(String? why, ActiveCamera? cam) {
    if (why == message && cam?.id == camera?.id && cam?.name == camera?.name) return;
    message = why;
    camera = cam;
    notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    tabs.removeListener(_onTabs);
    views?.worldListeners.remove(_onWorld);
    _model?.removeListener(_onModel);
    unawaited(_syncs?.cancel());
    controller.removeListener(invalidate);
    controller.dispose();
    super.dispose();
  }
}

/// The preview's picture, with the camera's name and why there is none.
class GamePreviewView extends StatelessWidget {
  const GamePreviewView({super.key, required this.preview, this.showViewport = true, required this.placeholder});

  final GamePreview preview;

  /// False in widget tests (no Metal texture).
  final bool showViewport;

  /// What stands in while there is no picture (the panel's idle text).
  final Widget Function(String message) placeholder;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: preview,
      builder: (context, _) {
        final message = preview.message;
        return _PaintProbe(
          onPaint: preview.painted,
          revision: Object(),
          child: Stack(
            fit: StackFit.expand,
            children: [
              if (showViewport) FynkelViewport(controller: preview.controller, placeholder: const SizedBox.shrink()),
              if (message != null) placeholder(message),
            ],
          ),
        );
      },
    );
  }
}

/// Calls [onPaint] whenever it is painted — only while on screen: a
/// hidden dock tab is offstage and never painted.
class _PaintProbe extends SingleChildRenderObjectWidget {
  const _PaintProbe({required this.onPaint, required this.revision, super.child});

  final VoidCallback onPaint;

  /// A new value repaints (a rebuild after a change).
  final Object revision;

  @override
  RenderObject createRenderObject(BuildContext context) => _RenderPaintProbe(onPaint);

  @override
  void updateRenderObject(BuildContext context, _RenderPaintProbe renderObject) {
    renderObject
      ..onPaint = onPaint
      ..markNeedsPaint();
  }
}

class _RenderPaintProbe extends RenderProxyBox {
  _RenderPaintProbe(this.onPaint);

  VoidCallback onPaint;

  @override
  void paint(PaintingContext context, Offset offset) {
    onPaint();
    super.paint(context, offset);
  }
}
