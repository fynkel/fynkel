import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';
import 'package:fynkel/fynkel.dart' show Texture2D;

/// A widget drawn into a [Texture2D] (stage 15, decision 4): a sign, a
/// poster or a scoreboard in the scene with the game's own look, shown by
/// a `SpriteRenderer` or a material.
extension WidgetTexture on Widget {
  /// Draws this widget once, [size] logical pixels at [pixelRatio], into a
  /// texture `size × pixelRatio` pixels: off the screen, round it only a
  /// [Directionality] (left to right) and a [MediaQuery] of [size], so a
  /// [Text] needs no app. It isn't live and takes no taps: to change it,
  /// draw it again (a scoreboard once a second, not every frame). Images
  /// in it must be loaded first (`precacheImage`), as nothing waits for
  /// them.
  Future<Texture2D> toTexture({required Size size, double pixelRatio = 1}) async {
    if (!(size.width > 0 && size.height > 0 && size.isFinite)) {
      throw ArgumentError.value(size, 'size', 'must be more than 0 each way');
    }
    if (!(pixelRatio > 0 && pixelRatio.isFinite)) {
      throw ArgumentError.value(pixelRatio, 'pixelRatio', 'must be more than 0');
    }
    final dispatcher = WidgetsBinding.instance.platformDispatcher;
    final flutterView = dispatcher.implicitView ?? dispatcher.views.first;
    final boundary = RenderRepaintBoundary();
    final renderView = RenderView(
      view: flutterView,
      configuration: ViewConfiguration(logicalConstraints: BoxConstraints.tight(size), devicePixelRatio: pixelRatio),
      child: RenderPositionedBox(child: boundary),
    );
    final pipeline = PipelineOwner()..rootNode = renderView;
    renderView.prepareInitialFrame();
    final build = BuildOwner(focusManager: FocusManager());
    final root = RenderObjectToWidgetAdapter<RenderBox>(
      container: boundary,
      child: MediaQuery(
        data: MediaQueryData(size: size, devicePixelRatio: pixelRatio),
        child: Directionality(
          textDirection: TextDirection.ltr,
          child: SizedBox.fromSize(size: size, child: this),
        ),
      ),
    ).attachToRenderTree(build);
    try {
      build
        ..buildScope(root)
        ..finalizeTree();
      pipeline
        ..flushLayout()
        ..flushCompositingBits()
        ..flushPaint();
      final image = await boundary.toImage(pixelRatio: pixelRatio);
      try {
        // Straight alpha: the engine multiplies it in when it draws.
        final bytes = await image.toByteData(format: ui.ImageByteFormat.rawStraightRgba);
        return Texture2D.rgba(image.width, image.height, bytes!.buffer.asUint8List());
      } finally {
        image.dispose();
      }
    } finally {
      // Takes the widgets down, so their states are disposed: the tree
      // again with nothing in it.
      RenderObjectToWidgetAdapter<RenderBox>(container: boundary).attachToRenderTree(build, root);
      build.finalizeTree();
      pipeline.rootNode = null;
    }
  }
}
