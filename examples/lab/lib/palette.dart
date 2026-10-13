// The lab's palette, written once: the app's themes are made from it, and
// scenes take their colours from the theme (world.theme).

import 'package:flutter/material.dart';

/// The colour everything is grown from.
const seed = Color(0xFF4CA6E0);

/// The lab's theme, light or dark.
ThemeData labTheme(Brightness brightness) => ThemeData(
  colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: brightness),
  useMaterial3: true,
);

/// Light or dark, for the whole lab: a scene's switch changes it.
final themeMode = ValueNotifier(ThemeMode.dark);
