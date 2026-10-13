import 'dart:ui' as ui;

import 'package:fynkel/fynkel.dart' as fynkel;

/// A Flutter colour as a Fynkel one, for a palette shared by the app's
/// theme and the game: `Theme.of(context).colorScheme.primary.toFynkel()`.
extension FlutterColorToFynkel on ui.Color {
  /// This colour, in Fynkel's 0–1 sRGB channels.
  ///
  /// A wide-gamut colour (Display P3) is brought to sRGB, and what sRGB
  /// can't show is cut to it.
  fynkel.Color toFynkel() {
    if (colorSpace == ui.ColorSpace.sRGB) return fynkel.Color.rgb(r, g, b, a);
    // Flutter turns P3 into extended sRGB (P3 straight to sRGB keeps the
    // numbers), which is sRGB past 0–1.
    final c = withValues(colorSpace: ui.ColorSpace.extendedSRGB);
    return fynkel.Color.rgb(c.r.clamp(0, 1), c.g.clamp(0, 1), c.b.clamp(0, 1), a);
  }
}

/// A Fynkel colour as a Flutter one, for a widget in a game's colour.
extension FynkelColorToFlutter on fynkel.Color {
  /// This colour, as Flutter's sRGB [ui.Color].
  ui.Color toFlutter() => ui.Color.from(alpha: a, red: r, green: g, blue: b);
}
