// The overlay a viewport draws over the next render (viewport ABI v6,
// ADR-0019): anti-aliased lines with a width, filled triangles and the
// analytic ground grid, drawn after post — never exposed or tone mapped —
// and (v8) the selection outline around what the listed entities draw.

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
  final List<int> selected, children;
  final List<double> selectedColor, childrenColor;
  final double width;

  /// The 9 floats of the ABI: selected RGBA, children RGBA, width.
  Float32List packStyle() => Float32List.fromList([...selectedColor, ...childrenColor, width]);
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

  int get lineCount => _lineCount;
  int get triangleCount => _triangleCount;
  bool get isEmpty => _lineCount == 0 && _triangleCount == 0 && grid == null && _outline == null;

  /// `ax ay az bx by bz r g b a width on_top` per line.
  Float32List get lines => Float32List.sublistView(_lines, 0, _lineCount * lineFloats);

  /// `ax ay az bx by bz cx cy cz r g b a on_top` per triangle.
  Float32List get triangles => Float32List.sublistView(_triangles, 0, _triangleCount * triangleFloats);

  void clear() {
    _lineCount = 0;
    _triangleCount = 0;
    grid = null;
    _outline = null;
  }

  /// Unity's selection outline (viewport ABI v8): a ring just outside the
  /// silhouette of what the [selected] entities draw in [selectedColor]
  /// (orange) and of their [children]'s in [childrenColor] (blue), [width]
  /// physical pixels wide — over everything, hidden parts too, under the
  /// lines and triangles. Entities are renderer handles (`Preview.entities`,
  /// `LiveWorld.handles`); one in both lists is selected. Replaces an
  /// earlier outline; empty lists draw none.
  void outline(
    List<int> selected,
    List<int> children, {
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
