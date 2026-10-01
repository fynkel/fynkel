import 'package:fynkel/fynkel.dart' show EntityRef;
// The overlay a viewport draws over the next render (viewport ABI v6,
// ADR-0019): anti-aliased lines with a width, filled triangles and the
// analytic ground grid, drawn after post — never exposed or tone mapped —
// and (v8) the selection outline around what the listed entities draw;
// (v9) other worlds drawn into the frame (Prefab Mode in context,
// multi-scene editing).

import 'dart:typed_data';

/// The ground grid's look (the renderer's `Grid`). Colours are linear RGBA;
/// [cell] and [fade] are metres, 0 = adaptive to the camera.
class OverlayGrid {
  const OverlayGrid({
    this.minor = const [1, 1, 1, 0.10],
    this.major = const [1, 1, 1, 0.22],
    this.xAxis = const [0.90, 0.25, 0.25, 0.8],
    this.zAxis = const [0.25, 0.45, 0.95, 0.8],
    this.cell = 0,
    this.fade = 0,
  });
  final List<double> minor, major, xAxis, zAxis;
  final double cell, fade;

  /// The 18 floats of the ABI: minor, major, X-axis, Z-axis RGBA, cell, fade.
  Float32List pack() => Float32List.fromList([...minor, ...major, ...xAxis, ...zAxis, cell, fade]);
}

/// The selection outline queued with a [ViewportOverlay]: the renderer's
/// entity handles (`Entity::to_bits` — the authoring preview's `entities`,
/// the live world's `handles`) of what is [selected] and of its
/// [children], their colours (linear RGBA, straight alpha) and the ring's
/// [width] in physical (target) pixels.
class OverlayOutline {
  const OverlayOutline(this.selected, this.children, this.selectedColor, this.childrenColor, this.width);
  final List<EntityRef> selected, children;
  final List<double> selectedColor, childrenColor;
  final double width;

  /// The 9 floats of the ABI: selected RGBA, children RGBA, width.
  Float32List packStyle() => Float32List.fromList([...selectedColor, ...childrenColor, width]);
}

/// Another world drawn into the frame with a [ViewportOverlay] (viewport
/// ABI v9): the native `FynkelWorld*` at [worldAddress]
/// (`ScenePreview.worldAddress`, `LiveWorld.worldAddress`,
/// `World.native.handleAddress`), placed by [transform] — sixteen numbers,
/// column-major, its world → the rendered world's — and drawn [gray]
/// (desaturated to its luminance and halved: Unity's Prefab Mode context in
/// "Gray") or as it is.
class OverlayExtraWorld {
  const OverlayExtraWorld(this.worldAddress, this.transform, this.gray);
  final int worldAddress;
  final List<double> transform;
  final bool gray;

  /// The ABI's flag bit for [gray] (`FYNKEL_APPLE_EXTRA_WORLD_GRAY`).
  static const grayFlag = 1;

  /// Column-major identity: the world drawn where it stands.
  static const identity = <double>[1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

  int get flags => gray ? grayFlag : 0;
}

/// One frame's overlay, built on the host and handed to
/// `FynkelViewportController.overlay` before each render (the renderer
/// empties it with every render). Positions are world metres, colours
/// linear RGBA, widths physical (target) pixels. Reuse one and [clear] it
/// per frame: its buffers only grow.
class ViewportOverlay {
  static const lineFloats = 12, triangleFloats = 14;

  Float32List _lines = Float32List(lineFloats * 64);
  Float32List _triangles = Float32List(triangleFloats * 16);
  int _lineCount = 0, _triangleCount = 0;

  /// The ground grid, or null for none.
  OverlayGrid? grid;

  OverlayOutline? _outline;

  /// The selection outline set by [outline], or null for none.
  OverlayOutline? get selectionOutline => _outline;

  final List<OverlayExtraWorld> _extraWorlds = [];

  /// The worlds added by [extraWorld], in order.
  List<OverlayExtraWorld> get extraWorlds => List.unmodifiable(_extraWorlds);

  int get lineCount => _lineCount;
  int get triangleCount => _triangleCount;
  bool get isEmpty =>
      _lineCount == 0 && _triangleCount == 0 && grid == null && _outline == null && _extraWorlds.isEmpty;

  /// `ax ay az bx by bz r g b a width on_top` per line.
  Float32List get lines => Float32List.sublistView(_lines, 0, _lineCount * lineFloats);

  /// `ax ay az bx by bz cx cy cz r g b a on_top` per triangle.
  Float32List get triangles => Float32List.sublistView(_triangles, 0, _triangleCount * triangleFloats);

  void clear() {
    _lineCount = 0;
    _triangleCount = 0;
    grid = null;
    _outline = null;
    _extraWorlds.clear();
  }

  /// Draws the world at [worldAddress] (a native `FynkelWorld*`:
  /// `ScenePreview.worldAddress` for an edit preview) into the frame with
  /// the one the viewport renders (viewport ABI v9) — the editor's Prefab
  /// Mode in context (the scene's preview around the prefab's, [transform]
  /// the inverse of the instance's world matrix, [gray] for Unity's "Gray")
  /// and multi-scene editing (the other loaded scenes' previews, identity).
  /// [transform] is sixteen numbers, column-major, the world's frame → the
  /// rendered world's (null: identity). Its meshes share the frame's depth
  /// and shadows, its point and spot lights light the frame, its skinned
  /// meshes are posed as in their world; the rendered world's camera,
  /// environment and look stay the frame's, and its first directional light
  /// is the sun only when the rendered world has none. It draws from the
  /// rendered world's asset cache (every preview of one session shares it)
  /// and is never outlined. The world is read when the overlay is queued,
  /// just before the render; adding more worlds draws them all.
  void extraWorld(int worldAddress, {List<double>? transform, bool gray = false}) {
    final t = transform ?? OverlayExtraWorld.identity;
    if (t.length != 16) {
      throw ArgumentError.value(t.length, 'transform', 'sixteen numbers, column-major');
    }
    _extraWorlds.add(OverlayExtraWorld(worldAddress, List.of(t), gray));
  }

  /// The [extraWorlds]' transforms back to back (sixteen floats each) for
  /// the ABI.
  Float32List packExtraTransforms() =>
      Float32List.fromList([for (final w in _extraWorlds) ...w.transform]);

  /// Unity's selection outline (viewport ABI v8): a ring just outside the
  /// silhouette of what the [selected] entities draw in [selectedColor]
  /// (orange) and of their [children]'s in [childrenColor] (blue), [width]
  /// physical pixels wide — over everything, hidden parts too, under the
  /// lines and triangles. Entities are renderer handles (`Preview.entities`,
  /// `LiveWorld.handles`); one in both lists is selected. Replaces an
  /// earlier outline; empty lists draw none.
  void outline(
    List<EntityRef> selected,
    List<EntityRef> children, {
    List<double> selectedColor = const [1, 0.4, 0, 1],
    List<double> childrenColor = const [0.37, 0.47, 0.61, 1],
    double width = 2,
  }) {
    _outline = selected.isEmpty && children.isEmpty
        ? null
        : OverlayOutline(List.of(selected), List.of(children), selectedColor, childrenColor, width);
  }

  /// A segment [a]–[b]. [onTop] draws it over the scene; otherwise the part
  /// behind geometry fades to a quarter of its alpha.
  void line(List<double> a, List<double> b, List<double> rgba, {double width = 1.5, bool onTop = false}) {
    _lines = _grow(_lines, (_lineCount + 1) * lineFloats);
    final o = _lineCount++ * lineFloats;
    _lines
      ..setRange(o, o + 3, a)
      ..setRange(o + 3, o + 6, b)
      ..setRange(o + 6, o + 10, rgba);
    _lines[o + 10] = width;
    _lines[o + 11] = onTop ? 1 : 0;
  }

  /// Debug-line segments (ten floats each, `ax ay az bx by bz r g b a` —
  /// the authoring ABI's outline and icon lines) at one [width].
  void segments(Float32List data, {double width = 1.5, bool onTop = false}) {
    final n = data.length ~/ 10;
    _lines = _grow(_lines, (_lineCount + n) * lineFloats);
    for (var i = 0; i < n; i++) {
      final o = _lineCount++ * lineFloats;
      _lines.setRange(o, o + 10, data, i * 10);
      _lines[o + 10] = width;
      _lines[o + 11] = onTop ? 1 : 0;
    }
  }

  /// A filled triangle, anti-aliased at its edges.
  void triangle(List<double> a, List<double> b, List<double> c, List<double> rgba, {bool onTop = false}) {
    _triangles = _grow(_triangles, (_triangleCount + 1) * triangleFloats);
    final o = _triangleCount++ * triangleFloats;
    _triangles
      ..setRange(o, o + 3, a)
      ..setRange(o + 3, o + 6, b)
      ..setRange(o + 6, o + 9, c)
      ..setRange(o + 9, o + 13, rgba);
    _triangles[o + 13] = onTop ? 1 : 0;
  }

  static Float32List _grow(Float32List list, int needed) {
    if (needed <= list.length) return list;
    var n = list.length * 2;
    while (n < needed) {
      n *= 2;
    }
    return Float32List(n)..setAll(0, list);
  }
}
