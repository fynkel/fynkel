/// A colour as people pick it: sRGB, 0–1 per channel. The engine converts
/// to linear for lighting.
final class Color {
  /// From `0xAARRGGBB`, the way Flutter's `Color(0xFF…)` writes it.
  const Color(int argb)
    : a = ((argb >> 24) & 0xFF) / 255,
      r = ((argb >> 16) & 0xFF) / 255,
      g = ((argb >> 8) & 0xFF) / 255,
      b = (argb & 0xFF) / 255;

  const Color.rgb(this.r, this.g, this.b, [this.a = 1]);

  final double r;
  final double g;
  final double b;
  final double a;

  static const white = Color.rgb(1, 1, 1);
  static const black = Color.rgb(0, 0, 0);

  @override
  bool operator ==(Object other) => other is Color && other.r == r && other.g == g && other.b == b && other.a == a;

  @override
  int get hashCode => Object.hash(r, g, b, a);

  @override
  String toString() => 'Color.rgb($r, $g, $b, $a)';
}
