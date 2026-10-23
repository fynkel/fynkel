/// A colour each particle starts with (Unity's `MinMaxGradient`): a
/// [Color], the same for all, or a [ColorRange], one at random between
/// two.
sealed class ParticleColor {
  /// For [Color] and [ColorRange].
  const ParticleColor();
}

/// A colour for each particle, at random between [a] and [b] (Unity's
/// *Random Between Two Colors*): `ColorRange(Color(0xFFFFD34D), Color(0xFFFF8A2B))`.
final class ColorRange extends ParticleColor {
  /// Between [a] and [b].
  const ColorRange(this.a, this.b);

  /// One end.
  final Color a;

  /// The other end.
  final Color b;

  @override
  bool operator ==(Object other) => other is ColorRange && other.a == a && other.b == b;

  @override
  int get hashCode => Object.hash(a, b);

  @override
  String toString() => 'ColorRange($a, $b)';
}

/// Colours along 0 to 1, for a particle's colour over its life (Unity's
/// `Gradient`): each stop a time and a colour, blended between as they're
/// picked (sRGB), the first before it and the last after. Alpha is a
/// channel like the others: [ColorGradient.fadeOut] fades by it.
final class ColorGradient {
  /// [stops] in order of their times, each 0 to 1; at least one.
  ColorGradient(List<(double, Color)> stops) : stops = List.unmodifiable(stops) {
    if (stops.isEmpty) throw ArgumentError('a gradient has at least one stop');
    var last = 0.0;
    for (final (t, _) in stops) {
      if (!(t >= last && t <= 1)) {
        throw ArgumentError('a gradient\'s stops are 0 to 1, in order; got ${stops.map((s) => s.$1).toList()}');
      }
      last = t;
    }
  }

  /// [color] fading from opaque to nothing: smoke, dust, sparks.
  factory ColorGradient.fadeOut([Color color = Color.white]) =>
      ColorGradient([(0, color), (1, Color.rgb(color.r, color.g, color.b, 0))]);

  /// [color] fading in over the first [fadeIn] of a life and out over the
  /// last [fadeOut], so particles neither pop in nor out.
  factory ColorGradient.fadeInOut([Color color = Color.white, double fadeIn = 0.2, double fadeOut = 0.4]) {
    final clear = Color.rgb(color.r, color.g, color.b, 0);
    return ColorGradient([(0, clear), (fadeIn, color), (1 - fadeOut, color), (1, clear)]);
  }

  /// Its stops: each a time, 0 to 1, and a colour.
  final List<(double, Color)> stops;

  /// Its colour at [t], 0 to 1.
  Color at(double t) {
    if (t <= stops.first.$1) return stops.first.$2;
    for (var i = 1; i < stops.length; i++) {
      final (t1, c1) = stops[i];
      if (t <= t1) {
        final (t0, c0) = stops[i - 1];
        return t1 == t0 ? c1 : Color.lerp(c0, c1, (t - t0) / (t1 - t0));
      }
    }
    return stops.last.$2;
  }

  @override
  String toString() => 'ColorGradient($stops)';
}

/// A colour as people pick it: sRGB, 0–1 per channel. The engine converts
/// to linear for lighting.
final class Color extends ParticleColor {
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
