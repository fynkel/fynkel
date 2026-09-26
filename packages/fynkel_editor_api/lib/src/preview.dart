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

  /// The editor's 3D view (the material, model and prefab previews'):
  /// an asset alone on a stage, as the Project's tiles show it — lit by
  /// the stage's sun, framed from the front-right. Drag to orbit, the
  /// wheel to zoom, the middle button (or Shift) to pan, the right button
  /// to turn the light, F or a double-click to frame it again.
  ///
  /// [asset] is this preview's by default; [sub] a part of it
  /// (`mesh/Body`, `material/Skin`). A material is shown on [shape]:
  /// `sphere`, `cube`, `cylinder`, `capsule`, `plane`, or a mesh as
  /// `{'asset': id, 'sub': 'mesh/…'}`. [onStats] gets what is drawn once
  /// it is staged.
  Widget stage({String? asset, String? sub, Object? shape, void Function(PreviewStageStats stats)? onStats});

  /// The editor's sound view (the sound preview's): the waveform of
  /// [asset] (this preview's by default), one lane per channel, with a
  /// playhead. Clicking it plays from that point. [playing] and [loop]
  /// start, stop and loop the playback (a toolbar's buttons);
  /// [onPlaying] is told when it starts or stops by itself (a click, the
  /// end). [onInfo] gets the sound's format once it is read.
  Widget sound({
    String? asset,
    bool playing = false,
    bool loop = false,
    void Function(bool playing)? onPlaying,
    void Function(PreviewSoundInfo info)? onInfo,
  });
}

/// A sound's format, for a sound preview's info line.
final class PreviewSoundInfo {
  const PreviewSoundInfo({
    required this.sampleRate,
    required this.channels,
    required this.duration,
    required this.codec,
    this.streamed = false,
  });

  final int sampleRate, channels;

  /// Seconds.
  final double duration;

  /// `pcm` or `vorbis`.
  final String codec;

  /// Decoded as it plays, not held in memory.
  final bool streamed;
}

/// What a [PreviewContext.stage] draws: its meshes, and their size.
final class PreviewStageStats {
  const PreviewStageStats({this.meshes = 0, this.vertices = 0, this.triangles = 0, this.skinned = 0, this.size});

  final int meshes, vertices, triangles;

  /// Of the meshes, the ones a skin deforms.
  final int skinned;

  /// The bounds' width, height and depth in metres; null when nothing
  /// measurable is drawn.
  final List<double>? size;
}
