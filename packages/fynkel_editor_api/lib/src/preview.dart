import 'package:flutter/widgets.dart';

import 'assets.dart';
import 'context.dart';

/// What a preview's builders are given (ADR-0030): the asset in the
/// Inspector's preview area, a little state its toolbar, picture and info
/// line share, and the editor's own building blocks.
abstract interface class PreviewContext {
  /// The asset being previewed.
  EditorAsset get asset;

  /// The asset's file, an absolute path.
  String get file;

  /// A source's import settings, as its sidecar has them
  /// (`{colorSpace: linear}`); empty for a document.
  Map<String, Object?> get importSettings;

  /// The editor, as a menu item's callback gets it: read assets, log.
  /// A preview shows; it never edits (ADR-0030 §2).
  EditorContext get editor;

  /// A value the toolbar, the picture and the info line share, by [key]:
  /// [initial] until [set] changes it. It lasts while the same asset stays
  /// in the preview.
  T value<T>(String key, T initial);

  /// Changes a shared value and draws the preview again.
  void set(String key, Object? value);

  /// The editor's image view (the texture preview's): [file] fitted,
  /// zoomed with the wheel, panned with a drag, F to fit again, a
  /// checkerboard behind transparency. [channel] is `rgb`, `r`, `g`, `b`
  /// or `a` (one channel shown as grey); [mip] draws the image at that
  /// mip level's size. [onSize] gets the image's full size once it is
  /// read.
  Widget image(String file, {String channel = 'rgb', int mip = 0, void Function(Size size)? onSize});
}
