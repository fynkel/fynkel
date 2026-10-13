/// A colour as people pick it: sRGB, 0–1 per channel. The engine converts
/// to linear for lighting.
final class Color {
  /// From `0xAARRGGBB`, the way Flutter's `Color(0xFF…)` writes it.
  const Color(int argb)
    : a = ((argb >> 24) & 0xFF) / 255,
      r = ((argb >> 16) & 0xFF) / 255,
      g = ((argb >> 8) & 0xFF) / 255,
      b = (argb & 0xFF) / 255;

  /// From channels, each 0–1 sRGB: `Color.rgb(1, 0.5, 0)` is orange.
  /// [a] is 1, opaque, unless given.
  const Color.rgb(this.r, this.g, this.b, [this.a = 1]);

  /// Red, 0–1 sRGB.
  final double r;

  /// Green, 0–1 sRGB.
  final double g;

  /// Blue, 0–1 sRGB.
  final double b;

  /// Alpha, 0–1: 1 is opaque, 0 is fully see-through.
  final double a;

  /// Opaque white.
  static const white = Color.rgb(1, 1, 1);

  /// Opaque black.
  static const black = Color.rgb(0, 0, 0);

  /// The colour [t] of the way from [a] to [b], channel by channel as
  /// they're picked (sRGB): [a] at 0, [b] at 1.
  static Color lerp(Color a, Color b, double t) =>
      Color.rgb(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t);

  @override
  bool operator ==(Object other) => other is Color && other.r == r && other.g == g && other.b == b && other.a == a;

  @override
  int get hashCode => Object.hash(r, g, b, a);

  @override
  String toString() => 'Color.rgb($r, $g, $b, $a)';
}
