part of 'world.dart';

// Layers (docs/stage-4.md §3.9), as Unity's: each object is on one of 32,
// and cameras, rays and taps take a mask of the layers they see.

/// Throws unless [layer] is 0 to 31.
int _checkLayer(int layer) =>
    (layer >= 0 && layer < 32) ? layer : throw RangeError.range(layer, 0, 31, 'layer', 'layers are 0 to 31');

/// A set of layers (Unity's `LayerMask`): which ones a camera draws
/// ([Camera.cullingMask]), a ray hits ([World.raycast]) or a finger
/// touches ([Input.layers]). Name your layers with constants:
///
/// ```dart
/// const enemies = 8;
/// world.raycast(ray, layers: LayerMask.of([enemies]));
/// ```
final class LayerMask {
  const LayerMask._(this.value);

  /// Every layer.
  static const all = LayerMask._(0xFFFFFFFF);

  /// No layer.
  static const none = LayerMask._(0);

  /// Just [layers].
  factory LayerMask.of(Iterable<int> layers) {
    var value = 0;
    for (final layer in layers) {
      value |= 1 << _checkLayer(layer);
    }
    return LayerMask._(value);
  }

  /// Every layer but [layers].
  factory LayerMask.except(Iterable<int> layers) => LayerMask._(~LayerMask.of(layers).value & all.value);

  /// One bit per layer: bit n for layer n.
  final int value;

  /// Whether [layer] is in it. Throws a [RangeError] for a layer outside
  /// 0–31.
  bool contains(int layer) => (value >> _checkLayer(layer)) & 1 == 1;

  /// This with [layer] too.
  LayerMask plus(int layer) => LayerMask._(value | 1 << _checkLayer(layer));

  /// This without [layer].
  LayerMask minus(int layer) => LayerMask._(value & ~(1 << _checkLayer(layer)));

  /// The layers in either.
  LayerMask operator |(LayerMask other) => LayerMask._(value | other.value);

  /// The layers in both.
  LayerMask operator &(LayerMask other) => LayerMask._(value & other.value);

  /// The layers in it, lowest first.
  List<int> get layers => [
    for (var i = 0; i < 32; i++)
      if ((value >> i) & 1 == 1) i,
  ];

  @override
  bool operator ==(Object other) => other is LayerMask && other.value == value;

  @override
  int get hashCode => value.hashCode;

  @override
  String toString() => switch (value) {
    0xFFFFFFFF => 'LayerMask.all',
    0 => 'LayerMask.none',
    _ when value.bitCount > 16 => 'LayerMask.except(${LayerMask._(~value & all.value).layers})',
    _ => 'LayerMask.of($layers)',
  };
}

extension on int {
  int get bitCount {
    var n = 0;
    for (var v = this; v != 0; v &= v - 1) {
      n++;
    }
    return n;
  }
}
