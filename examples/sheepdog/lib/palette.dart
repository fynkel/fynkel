// The palette, written once: the app's themes are grown from one colour,
// and the game takes its colours from the theme (SheepdogGame.wear), so
// the light theme is a day in the field and the dark one a night.

import 'package:flutter/material.dart';

/// The colour everything is grown from: grass.
const seed = Color(0xFF6BA34A);

/// The app's theme, light or dark.
ThemeData sheepdogTheme(Brightness brightness) => ThemeData(
  colorScheme: ColorScheme.fromSeed(seedColor: seed, brightness: brightness),
);
