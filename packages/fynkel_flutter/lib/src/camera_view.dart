// 3D in the UI (ADR-0062 amendment 1, step 3): what one of the world's
// cameras sees, as a widget — Unity's camera with a render texture shown
// in a RawImage. A minimap, a model preview in an inventory, a rear-view
// mirror, a security-camera screen: put a `CameraView` anywhere in the
// game's widget tree and it draws every frame through that camera, at the
// widget's size.

import 'package:flutter/scheduler.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/gameplay.dart';

import 'viewport.dart';
import 'viewport_controller.dart';

/// Shows what [camera] — an object of [world] with a `Camera` component,
/// or its entity — sees, drawn every frame at this widget's size.
///
/// The camera is an ordinary scene camera. The game's main picture is
/// drawn through the camera with the highest `priority`, so give this one
/// a lower priority than the main camera, or it takes over the screen.
/// Its `cullingMask` chooses what it draws: a model preview puts the model
/// (and a light whose `cullingMask` reaches it) on a layer only this
/// camera draws, and takes that layer out of the main camera's mask.
///
/// The picture uses the camera's pose from the frame (interpolated like
/// the main picture), its projection, exposure and culling mask, and the
/// world's assets. While the camera is destroyed, inactive or not yet
/// created, the panel keeps its last picture. It draws while its route is
/// visible (a hidden route's tickers are muted) and takes no keyboard
/// focus, so a menu around it navigates as if it were not there.
class CameraView extends StatefulWidget {
  const CameraView({
    super.key,
    required this.world,
    required this.camera,
    this.controller,
    this.resolutionScale = 1.0,
    this.placeholder,
  });

  final World world;

  /// A `GameObject` or an `EntityRef` with a `Camera` component.
  final Object camera;

  /// The viewport to draw into (its quality profile, light budget, …);
  /// by default the view makes its own, without placeholder cubes.
  final FynkelViewportController? controller;

  /// As [FynkelViewport.resolutionScale].
  final double resolutionScale;

  /// Shown until the first picture.
  final Widget? placeholder;

  @override
  State<CameraView> createState() => CameraViewState();
}

class CameraViewState extends State<CameraView> with SingleTickerProviderStateMixin {
  FynkelViewportController? _owned;
  FynkelViewportController get controller =>
      widget.controller ?? (_owned ??= FynkelViewportController(placeholders: false));

  late final Ticker _ticker = createTicker((_) => _draw());
  final FocusNode _focus = FocusNode(debugLabel: 'CameraView', canRequestFocus: false, skipTraversal: true);

  /// Pictures drawn.
  int frames = 0;

  /// What the last draw answered: a slot of the ring (≥ 0), or one of the
  /// `render*` codes (`renderNoCamera` while the camera is gone).
  int? lastResult;

  /// The slot of the last picture drawn (`controller.readFrame(lastSlot!)`).
  int? lastSlot;

  @override
  void initState() {
    super.initState();
    _ticker.start();
  }

  @override
  void didUpdateWidget(CameraView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.controller != widget.controller && oldWidget.controller == null) {
      _owned?.dispose();
      _owned = null;
    }
  }

  EntityRef? _entity() {
    final c = widget.camera;
    if (c is EntityRef) return c;
    if (c is GameObject) return c.isPending || !c.isAlive ? null : c.entity;
    throw ArgumentError.value(c, 'camera', 'a GameObject or an EntityRef');
  }

  void _draw() {
    final c = controller;
    if (!c.isAttached || c.state == ViewportState.resizing) return;
    final entity = _entity();
    if (entity == null) {
      lastResult = renderNoCamera;
      return;
    }
    c.viewCamera = entity;
    final slot = c.render(widget.world);
    lastResult = slot;
    if (slot != null && slot >= 0) {
      lastSlot = slot;
      frames++;
    }
  }

  @override
  void dispose() {
    _ticker.dispose();
    _focus.dispose();
    _owned?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => FynkelViewport(
    controller: controller,
    focusNode: _focus,
    resolutionScale: widget.resolutionScale,
    filterQuality: FilterQuality.low,
    placeholder: widget.placeholder,
  );
}
